use std::path::Path;

pub(super) fn cmd_health(project: Option<String>, dev_ops: &Path, json: bool) {
    let projects = crate::entities::load_entities(dev_ops);
    let mut results = Vec::new();

    if let Some(q) = project {
        let query = q.to_lowercase();
        for p in &projects {
            if p.name.to_lowercase().contains(&query)
                || p.local_path.to_string_lossy().to_lowercase().contains(&query)
            {
                results.push(crate::health::check_project(p));
            }
        }
    } else {
        for p in &projects {
            results.push(crate::health::check_project(p));
        }
    }

    if json {
        match serde_json::to_string_pretty(&results) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
    } else {
        for r in &results {
            let dirty = match r.git_dirty {
                Some(true) => "DIRTY",
                Some(false) => "CLEAN",
                None => "N/A",
            };
            let remote = r.remote_url.as_deref().unwrap_or("N/A");
            println!(
                "Project: {:<20} | Status: {:<10} | Git: {:<5} | Grade: {} | URL: {}",
                r.name, r.status, dirty, r.compliance_grade, remote
            );
            let suggestions = crate::instinct::suggest_from_health(r);
            if !suggestions.is_empty() {
                println!(
                    "  \u{1f4a1} {} instinct suggestion(s) — run: raios instinct suggest {}",
                    suggestions.len(),
                    r.name
                );
            }
        }
    }
}

pub(super) fn cmd_stats(_dev_ops: &Path, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => { eprintln!("DB error: {}", e); return; }
    };
    let s = match crate::db::query_stats(&conn) {
        Ok(s) => s,
        Err(e) => { eprintln!("Stats query failed: {}", e); return; }
    };

    let top_cats: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT category, COUNT(*) AS n FROM projects GROUP BY category ORDER BY n DESC LIMIT 8"
        ).unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };

    if json {
        let out = serde_json::json!({
            "total": s.total, "active": s.active, "archived": s.archived,
            "dirty": s.dirty, "no_memory": s.no_memory, "local_only": s.no_github,
            "avg_compliance": s.avg_compliance as u64, "avg_security": s.avg_security as u64,
            "grade_a": s.grade_a, "grade_b": s.grade_b, "grade_c": s.grade_c, "grade_d": s.grade_d,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    fn bar(n: i64, total: i64, width: usize) -> String {
        if total == 0 { return String::new(); }
        "█".repeat((n as usize * width) / total as usize)
    }
    #[allow(clippy::manual_checked_ops)]
    fn pct(n: i64, total: i64) -> i64 { if total > 0 { n * 100 / total } else { 0 } }

    println!("Portfolio Statistics — R-AI-OS v{}", env!("CARGO_PKG_VERSION"));
    println!("{}", "─".repeat(46));
    println!("Total projects:      {:>5}", s.total);
    println!("Active / Archived:   {:>5} / {}", s.active, s.archived);
    println!("Dirty (uncommitted): {:>5}", s.dirty);
    println!("No memory.md:        {:>5}", s.no_memory);
    println!("Local only (no GH):  {:>5}", s.no_github);
    println!("Avg compliance:      {:>4}/100", s.avg_compliance as u64);
    println!("Avg security:        {:>4}/100", s.avg_security as u64);
    println!();
    println!("Grade Distribution:");
    println!("  A (≥80): {:>4} projects  {} {}%", s.grade_a, bar(s.grade_a, s.total, 24), pct(s.grade_a, s.total));
    println!("  B (≥60): {:>4} projects  {} {}%", s.grade_b, bar(s.grade_b, s.total, 24), pct(s.grade_b, s.total));
    println!("  C (≥40): {:>4} projects  {} {}%", s.grade_c, bar(s.grade_c, s.total, 24), pct(s.grade_c, s.total));
    println!("  D  (<40): {:>4} projects  {} {}%", s.grade_d, bar(s.grade_d, s.total, 24), pct(s.grade_d, s.total));
    println!();
    println!("Top Categories:");
    for (cat, count) in &top_cats {
        println!("  {:<28} {}", cat.replace('_', " "), count);
    }
}

pub(super) fn cmd_commit(
    project: Option<String>,
    message: Option<String>,
    push: bool,
    dry_run: bool,
    dev_ops: &Path,
    json: bool,
) {
    use crate::filebrowser::{git_commit, git_is_dirty, git_push};

    let projects = crate::entities::load_entities(dev_ops);
    let commit_msg = message.as_deref().unwrap_or("chore: raios auto-sync");
    let candidates: Vec<_> = if let Some(q) = project {
        let q = q.to_lowercase();
        projects.into_iter().filter(|p| p.name.to_lowercase().contains(&q)).collect()
    } else {
        projects
    };

    #[derive(serde::Serialize)]
    struct CommitEntry { name: String, committed: bool, pushed: bool, note: String }
    let mut entries: Vec<CommitEntry> = Vec::new();
    let mut committed_count = 0usize;
    let mut skipped_count = 0usize;

    for p in &candidates {
        let dirty = git_is_dirty(&p.local_path).unwrap_or(false);
        if !dirty {
            skipped_count += 1;
            if !json { println!("  skip  {}", p.name); }
            continue;
        }
        if dry_run {
            if !json { println!("  would commit  {}", p.name); }
            entries.push(CommitEntry { name: p.name.clone(), committed: false, pushed: false, note: "dry-run".into() });
            continue;
        }
        let result = git_commit(&p.local_path, commit_msg);
        let mut pushed_ok = false;
        let mut note = result.message.clone();
        if result.committed && push {
            match git_push(&p.local_path) {
                Ok(()) => { pushed_ok = true; note = "committed + pushed".into(); }
                Err(e) => { note = format!("committed, push failed: {}", e); }
            }
        } else if result.committed {
            note = "committed".into();
        }
        if result.committed { committed_count += 1; } else { skipped_count += 1; }
        if !json {
            let status = if result.committed { if pushed_ok { "  ✓ push " } else { "  ✓ commit" } } else { "  - skip  " };
            println!("{} {} — {}", status, p.name, note);
        }
        entries.push(CommitEntry { name: p.name.clone(), committed: result.committed, pushed: pushed_ok, note });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap_or_default());
    } else {
        println!("\nDone — {} committed, {} skipped.", committed_count, skipped_count);
    }
}
