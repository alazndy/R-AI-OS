use std::path::Path;
use std::process::Command;

use crate::entities::EntityProject;

struct ProjectSnapshot {
    name: String,
    dirty_files: usize,
    last_commit_days: Option<u64>,
    has_readme: bool,
    has_memory: bool,
    has_sigmap: bool,
    memory_stale_days: Option<u64>,
}

pub fn cmd_reflect(dev_ops_path: &Path, json: bool) {
    let projects = crate::entities::discover_entities(dev_ops_path);
    if projects.is_empty() {
        eprintln!("No projects found in {}", dev_ops_path.display());
        return;
    }

    let snapshots: Vec<ProjectSnapshot> = projects
        .iter()
        .map(snapshot)
        .collect();

    if json {
        print_json(&snapshots);
    } else {
        print_report(&snapshots);
    }
}

fn snapshot(p: &EntityProject) -> ProjectSnapshot {
    let dirty_files = count_dirty_files(&p.local_path);
    let last_commit_days = git_days_since_last_commit(&p.local_path);
    let has_readme = p.local_path.join("README.md").exists();
    let has_memory = p.local_path.join("memory.md").exists();
    let has_sigmap = p.local_path.join("SIGMAP.md").exists();
    let memory_stale_days = if has_memory {
        file_age_days(&p.local_path.join("memory.md"))
    } else {
        None
    };

    ProjectSnapshot {
        name: p.name.clone(),
        dirty_files,
        last_commit_days,
        has_readme,
        has_memory,
        has_sigmap,
        memory_stale_days,
    }
}

fn count_dirty_files(dir: &Path) -> usize {
    let out = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "status", "--porcelain"])
        .output()
        .ok();
    out.map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count()
    })
    .unwrap_or(0)
}

fn git_days_since_last_commit(dir: &Path) -> Option<u64> {
    let out = Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "log", "-1", "--format=%ct"])
        .output()
        .ok()?;
    let ts: i64 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(((now - ts).max(0) / 86400) as u64)
}

fn file_age_days(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?;
    Some(age.as_secs() / 86400)
}

fn print_report(snaps: &[ProjectSnapshot]) {
    let total = snaps.len();
    let dirty_count = snaps.iter().filter(|s| s.dirty_files > 0).count();
    let stale_count = snaps
        .iter()
        .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        .count();

    let readme_ok = snaps.iter().filter(|s| s.has_readme).count();
    let memory_ok = snaps.iter().filter(|s| s.has_memory).count();
    let sigmap_ok = snaps.iter().filter(|s| s.has_sigmap).count();

    let score = calculate_score(snaps);

    println!();
    println!("━━━ WORKSPACE REFLECTION ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "  Projects: {}  │  dirty: {}  │  stale (>14d): {}",
        total, dirty_count, stale_count
    );
    println!();

    // Attention required
    let attention: Vec<_> = snaps
        .iter()
        .filter(|s| {
            s.dirty_files > 0
                || !s.has_readme
                || !s.has_memory
                || !s.has_sigmap
                || s.memory_stale_days.is_some_and(|d| d > 7)
                || s.last_commit_days.is_some_and(|d| d > 14)
        })
        .collect();

    if attention.is_empty() {
        println!("  ✓ All projects look healthy.");
    } else {
        println!("── ATTENTION REQUIRED ──────────────────────────────────────");
        for s in &attention {
            let mut flags = Vec::new();
            if s.dirty_files > 0 {
                flags.push(format!("dirty:{}", s.dirty_files));
            }
            if let Some(d) = s.last_commit_days {
                if d > 14 {
                    flags.push(format!("stale:{}d", d));
                }
            }
            if !s.has_readme {
                flags.push("no README.md".into());
            }
            if !s.has_memory {
                flags.push("no memory.md".into());
            }
            if !s.has_sigmap {
                flags.push("no SIGMAP.md".into());
            }
            if let Some(d) = s.memory_stale_days {
                if d > 7 {
                    flags.push(format!("memory stale:{}d", d));
                }
            }
            println!("  ● {:<24} {}", s.name, flags.join("  "));
        }
    }

    println!();
    println!("── DOCS COVERAGE ───────────────────────────────────────────");
    println!(
        "  README.md  {}/{} present{}",
        readme_ok,
        total,
        if readme_ok < total {
            format!("  ✗ {} missing", total - readme_ok)
        } else {
            "  ✓".into()
        }
    );
    println!(
        "  memory.md  {}/{} present{}",
        memory_ok,
        total,
        if memory_ok < total {
            format!("  ✗ {} missing", total - memory_ok)
        } else {
            "  ✓".into()
        }
    );
    println!(
        "  SIGMAP.md  {}/{} present{}",
        sigmap_ok,
        total,
        if sigmap_ok < total {
            format!("  ✗ {} missing", total - sigmap_ok)
        } else {
            "  ✓".into()
        }
    );

    if stale_count > 0 {
        println!();
        println!("── STALE PROJECTS (no commit > 14d) ────────────────────────");
        for s in snaps
            .iter()
            .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        {
            println!(
                "  ● {:<24} last commit: {}d ago",
                s.name,
                s.last_commit_days.unwrap_or(0)
            );
        }
    }

    println!();
    let bar_filled = (score / 10) as usize;
    let bar: String = "█".repeat(bar_filled) + &"░".repeat(10 - bar_filled);
    println!("── OVERALL SCORE ───────────────────────────────────────────");
    println!("  {}  {}/100", bar, score);

    // Recommendations
    let recs = build_recommendations(snaps);
    if !recs.is_empty() {
        println!();
        println!("── RECOMMENDATIONS ─────────────────────────────────────────");
        for (i, rec) in recs.iter().enumerate() {
            println!("  {}. {}", i + 1, rec);
        }
    }
    println!();
}

fn calculate_score(snaps: &[ProjectSnapshot]) -> u8 {
    if snaps.is_empty() {
        return 100;
    }
    let total = snaps.len() as f32;
    let dirty_penalty = snaps.iter().filter(|s| s.dirty_files > 0).count() as f32 * 3.0;
    let readme_penalty = snaps.iter().filter(|s| !s.has_readme).count() as f32 * 2.0;
    let memory_penalty = snaps.iter().filter(|s| !s.has_memory).count() as f32 * 2.0;
    let sigmap_penalty = snaps.iter().filter(|s| !s.has_sigmap).count() as f32 * 1.0;
    let stale_penalty = snaps
        .iter()
        .filter(|s| s.last_commit_days.is_some_and(|d| d > 14))
        .count() as f32
        * 2.0;
    let mem_stale_penalty = snaps
        .iter()
        .filter(|s| s.memory_stale_days.is_some_and(|d| d > 7))
        .count() as f32
        * 1.0;

    let total_penalty = dirty_penalty
        + readme_penalty
        + memory_penalty
        + sigmap_penalty
        + stale_penalty
        + mem_stale_penalty;

    let raw = 100.0 - (total_penalty / total * 10.0);
    raw.clamp(0.0, 100.0) as u8
}

fn build_recommendations(snaps: &[ProjectSnapshot]) -> Vec<String> {
    let mut recs = Vec::new();

    let dirty: Vec<_> = snaps
        .iter()
        .filter(|s| s.dirty_files > 0)
        .map(|s| s.name.as_str())
        .collect();
    if !dirty.is_empty() {
        recs.push(format!(
            "Commit or stash dirty changes: {}",
            dirty.join(", ")
        ));
    }

    let no_memory: usize = snaps.iter().filter(|s| !s.has_memory).count();
    if no_memory > 0 {
        recs.push(format!(
            "Create memory.md in {} project(s) — use standard template",
            no_memory
        ));
    }

    let no_sigmap: usize = snaps.iter().filter(|s| !s.has_sigmap).count();
    if no_sigmap > 0 {
        recs.push(format!(
            "Run `sigmap` in {} project(s) to generate SIGMAP.md",
            no_sigmap
        ));
    }

    let stale_mem: Vec<_> = snaps
        .iter()
        .filter(|s| s.memory_stale_days.is_some_and(|d| d > 7))
        .map(|s| s.name.as_str())
        .collect();
    if !stale_mem.is_empty() {
        recs.push(format!(
            "Update memory.md (>7d stale): {}",
            stale_mem.join(", ")
        ));
    }

    recs
}

fn print_json(snaps: &[ProjectSnapshot]) {
    let items: Vec<serde_json::Value> = snaps
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "dirty_files": s.dirty_files,
                "last_commit_days": s.last_commit_days,
                "has_readme": s.has_readme,
                "has_memory": s.has_memory,
                "has_sigmap": s.has_sigmap,
                "memory_stale_days": s.memory_stale_days,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "score": calculate_score(snaps),
            "projects": items,
        }))
        .unwrap_or_default()
    );
}
