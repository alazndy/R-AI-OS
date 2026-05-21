use std::path::Path;

pub(super) fn cmd_security(
    target: Option<String>,
    full: bool,
    watch: bool,
    dev_ops: &Path,
    json: bool,
) {
    // Resolve target: existing filesystem path takes priority, else treat as project name filter
    let (scan_path, project_filter): (Option<std::path::PathBuf>, Option<String>) =
        match &target {
            None => (None, None),
            Some(t) => {
                let p = std::path::PathBuf::from(t);
                if p.exists() {
                    (Some(p), None)
                } else {
                    (None, Some(t.clone()))
                }
            }
        };

    if watch {
        let watch_target = scan_path
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        if !watch_target.exists() {
            eprintln!("Path does not exist: {}", watch_target.display());
            std::process::exit(1);
        }
        if let Err(e) = cmd_security_watch(&watch_target, json) {
            eprintln!("Guard error: {e}");
            std::process::exit(1);
        }
        return;
    }
    use crate::security::{scan_project, Severity};

    let targets: Vec<(String, std::path::PathBuf)> = if let Some(path) = scan_path {
        vec![("custom".into(), path)]
    } else {
        let projects = crate::entities::load_entities(dev_ops);
        if let Some(ref q) = project_filter {
            let q = q.to_lowercase();
            projects.into_iter()
                .filter(|p| p.name.to_lowercase().contains(&q))
                .map(|p| (p.name, p.local_path)).collect()
        } else {
            projects.into_iter().map(|p| (p.name, p.local_path)).collect()
        }
    };

    if targets.is_empty() { eprintln!("No projects found."); return; }

    let mut all_reports = Vec::new();
    for (name, path) in &targets {
        if !json { eprint!("  scanning {}...", name); }
        let report = scan_project(path);
        if !json { eprintln!(" {} ({}/100)", report.grade, report.score); }
        all_reports.push((name.clone(), path.clone(), report));
    }

    if json {
        #[derive(serde::Serialize)]
        struct Row<'a> { name: &'a str, path: String, score: u8, grade: &'a str, issues: usize, critical: usize }
        let rows: Vec<Row> = all_reports.iter()
            .map(|(n, p, r)| Row { name: n, path: p.display().to_string(), score: r.score, grade: r.grade, issues: r.issues.len(), critical: r.critical_count() })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return;
    }

    println!();
    println!("Security Scan Results");
    println!("{}", "─".repeat(72));
    println!("{:<28} {:>5}  {:>5}  {:>4}  {:>4}  {:>4}  {:>4}", "Project", "Score", "Grade", "Crit", "High", "Med", "Low");
    println!("{}", "─".repeat(72));

    let mut total_score: u32 = 0;
    let mut total_crit = 0usize;

    for (name, _, report) in &all_reports {
        let crit = report.issues.iter().filter(|i| i.severity == Severity::Critical).count();
        let high = report.issues.iter().filter(|i| i.severity == Severity::High).count();
        let med  = report.issues.iter().filter(|i| i.severity == Severity::Medium).count();
        let low  = report.issues.iter().filter(|i| i.severity == Severity::Low).count();
        let name_trunc: String = name.chars().take(27).collect();
        println!("{:<28} {:>5}  {:>5}  {:>4}  {:>4}  {:>4}  {:>4}", name_trunc, report.score, report.grade, crit, high, med, low);
        total_score += report.score as u32;
        total_crit += crit;
        if full && !report.issues.is_empty() {
            print_report(report, false);
            println!();
        }
    }

    println!("{}", "─".repeat(72));
    let avg = if all_reports.is_empty() { 0 } else { total_score as usize / all_reports.len() };
    println!("Average score: {}/100   Total critical issues: {}", avg, total_crit);
    if !full && all_reports.iter().any(|(_, _, r)| !r.issues.is_empty()) {
        println!("\nUse --full to see individual issues.");
    }
}

pub(super) fn cmd_security_watch(path: &Path, json: bool) -> anyhow::Result<()> {
    use crate::security::{scan_file, WATCHED_EXTS};
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc::channel;

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| { let _ = tx.send(res); })?;
    watcher.watch(path, RecursiveMode::Recursive)?;
    eprintln!("Guard watching {} (Ctrl+C to stop)", path.display());

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                use notify::EventKind;
                if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) { continue; }
                for file_path in &event.paths {
                    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !WATCHED_EXTS.contains(&ext) { continue; }
                    let issues = scan_file(file_path);
                    print_guard_result(file_path, &issues, json);
                    if !issues.is_empty() { send_toast(file_path, &issues); }
                }
            }
            Ok(Err(e)) => eprintln!("[guard] watcher error: {e}"),
            Err(_) => break,
        }
    }
    Ok(())
}

fn print_report(report: &crate::security::SecurityReport, json: bool) {
    if json {
        let issues: Vec<serde_json::Value> = report.issues.iter().map(|i| serde_json::json!({
            "owasp": i.owasp, "severity": i.severity.label(), "title": i.title,
            "file": i.file.as_ref().map(|p| p.display().to_string()), "line": i.line, "snippet": i.snippet
        })).collect();
        match serde_json::to_string_pretty(&issues) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }
    println!("Security scan: score={}/100 grade={}", report.score, report.grade);
    if report.issues.is_empty() { println!("No issues found"); return; }
    for issue in &report.issues {
        let file_info = issue.file.as_ref()
            .map(|p| format!(" — {}:{}", p.display(), issue.line.unwrap_or(0)))
            .unwrap_or_default();
        println!("⚠ {} [{}] {}{}", issue.severity.label(), issue.owasp, issue.title, file_info);
        if let Some(ref s) = issue.snippet { println!("   \"{}\"", s); }
    }
}

fn print_guard_result(path: &Path, issues: &[crate::security::SecurityIssue], json: bool) {
    if json {
        if issues.is_empty() { return; }
        let out: Vec<serde_json::Value> = issues.iter().map(|i| serde_json::json!({
            "file": path.display().to_string(), "line": i.line, "owasp": i.owasp,
            "severity": i.severity.label(), "title": i.title, "snippet": i.snippet
        })).collect();
        match serde_json::to_string_pretty(&out) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }
    if issues.is_empty() { eprintln!("  {} clean", path.display()); return; }
    for issue in issues {
        eprintln!("⚠ {} [{}] {} — {}:{}", issue.severity.label(), issue.owasp, issue.title, path.display(), issue.line.unwrap_or(0));
        if let Some(ref s) = issue.snippet { eprintln!("   \"{}\"", s); }
    }
}

fn send_toast(path: &Path, issues: &[crate::security::SecurityIssue]) {
    let top = match issues.first() { Some(i) => i, None => return };
    let filename = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let body = format!("{} · {} in {}:{}", top.owasp, top.title, filename, top.line.unwrap_or(0));
    let summary = format!("[RAIOS GUARD] {}", top.severity.label());
    if let Err(e) = notify_rust::Notification::new().summary(&summary).body(&body).show() {
        eprintln!("[guard] toast failed (non-fatal): {e}");
    }
}
