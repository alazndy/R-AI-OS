use crate::security::license::{scan_licenses, LicenseReport};
use crate::security::quarantine;
use crate::security::secret_lease;
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
        struct IssueJson<'a> {
            owasp: &'a str,
            severity: &'a str,
            title: &'a str,
            file: Option<String>,
            line: Option<usize>,
            snippet: Option<&'a String>,
        }
        #[derive(serde::Serialize)]
        struct Row<'a> {
            schema_version: u8,
            name: &'a str,
            path: String,
            score: u8,
            grade: &'a str,
            critical_count: usize,
            high_count: usize,
            issues: Vec<IssueJson<'a>>,
        }
        let rows: Vec<Row> = all_reports.iter()
            .map(|(n, p, r)| Row {
                schema_version: 1,
                name: n,
                path: p.display().to_string(),
                score: r.score,
                grade: r.grade,
                critical_count: r.critical_count(),
                high_count: r.high_count(),
                issues: r.issues.iter().map(|i| IssueJson {
                    owasp: i.owasp,
                    severity: i.severity.label(),
                    title: i.title,
                    file: i.file.as_ref().map(|p| p.display().to_string()),
                    line: i.line,
                    snippet: i.snippet.as_ref(),
                }).collect(),
            })
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

pub(super) fn cmd_license(project: Option<String>, dev_ops: &Path, json_out: bool) {
    let path = crate::cli::resolve_project_path(project, dev_ops);
    let report = scan_licenses(&path);

    if json_out {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return;
    }

    print_license_report(&report);
}

fn print_license_report(report: &LicenseReport) {
    println!("\n  License Compliance — {}", report.project_path.display());
    println!("  {}", "─".repeat(60));
    println!("  Total dependencies scanned : {}", report.total);

    if report.copyleft_count > 0 {
        println!("  Copyleft (GPL/AGPL/LGPL)   : {} ⚠", report.copyleft_count);
    } else {
        println!("  Copyleft (GPL/AGPL/LGPL)   : 0 ✓");
    }

    if report.unknown_count > 0 {
        println!("  Unknown license            : {} ⚠", report.unknown_count);
    } else {
        println!("  Unknown license            : 0 ✓");
    }

    if report.copyleft_count > 0 || report.unknown_count > 0 {
        println!("\n  Issues:");
        for dep in report.deps.iter().filter(|d| d.is_copyleft || d.is_unknown) {
            let tag = if dep.is_copyleft { "COPYLEFT" } else { "UNKNOWN " };
            println!("    [{}] {} {} — {}", tag, dep.name, dep.version, dep.license);
        }
    }
    println!();
}

// ─── verify-chain ─────────────────────────────────────────────────────────────

pub(super) fn cmd_verify_chain(last: usize, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {}", e);
            std::process::exit(1);
        }
    };

    // Optionally show the last N entries before verification
    if last > 0 {
        match conn.prepare(
            "SELECT id, timestamp, event_type, actor, data FROM audit_log ORDER BY id DESC LIMIT ?1",
        ) {
            Ok(mut stmt) => {
                if let Ok(rows) = stmt.query_map([last as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                }) {
                    let entries: Vec<_> = rows.flatten().collect();
                    println!("Last {} audit entries:", last);
                    for (id, ts, ev, actor, data) in entries.iter().rev() {
                        println!("  [{id}] {ts} | {ev} | {actor} | {}", data.chars().take(60).collect::<String>());
                    }
                    println!();
                }
            }
            Err(e) => eprintln!("Warning: could not read entries: {}", e),
        }
    }

    match crate::security::verify_chain(&conn) {
        Ok(n) => {
            if json {
                println!("{{\"status\":\"ok\",\"entries_verified\":{}}}", n);
            } else {
                println!("✅ Audit chain OK — {} entries verified, no tampering detected.", n);
            }
        }
        Err(e) => {
            if json {
                println!("{{\"status\":\"broken\",\"error\":{:?}}}", e.to_string());
            } else {
                eprintln!("❌ Audit chain BROKEN: {}", e);
            }
            std::process::exit(2);
        }
    }
}

pub(super) fn cmd_rate_status(json: bool) {
    let config = crate::security::PolicyConfig::try_load_default();

    match config.and_then(|c| c.rate_limits) {
        None => {
            if json {
                println!("{{\"enabled\":false,\"source\":\"no raios-policy.toml found or no [rate_limits] section\"}}");
            } else {
                println!("Rate limiting: DISABLED (no raios-policy.toml or no [rate_limits] section)");
                println!("Create raios-policy.toml with [rate_limits] to enable.");
            }
        }
        Some(cfg) => {
            if json {
                let rules: Vec<serde_json::Value> = cfg.rules.iter().map(|r| {
                    serde_json::json!({ "tool": r.tool, "max_calls": r.max_calls })
                }).collect();
                println!("{}", serde_json::json!({
                    "enabled": cfg.enabled,
                    "window_secs": cfg.window_secs,
                    "default_max": cfg.default_max,
                    "rules": rules,
                }));
            } else {
                let status = if cfg.enabled { "ENABLED" } else { "DISABLED" };
                println!("Rate limiting: {status}");
                println!("  Window:      {}s", cfg.window_secs);
                println!("  Default max: {} calls/window", cfg.default_max);
                if cfg.rules.is_empty() {
                    println!("  Rules:       (none — default applies to all tools)");
                } else {
                    println!("  Rules:");
                    for r in &cfg.rules {
                        let label = if r.tool == "*" { "(all tools)" } else { &r.tool };
                        println!("    {label:<30}  max {} calls/{}s", r.max_calls, cfg.window_secs);
                    }
                }
                println!();
                println!("Note: live call counts are tracked per MCP server process.");
            }
        }
    }
}

pub(super) fn cmd_pin_reset(json: bool) {
    match crate::security::tool_pin::reset_pin() {
        Ok(()) => {
            if json {
                println!("{{\"status\":\"ok\",\"message\":\"pin file removed — next mcp-server start will re-pin\"}}");
            } else {
                println!("Pin file removed. Next `raios mcp-server` start will re-pin the tool manifest.");
            }
        }
        Err(e) => {
            if json {
                println!("{{\"status\":\"error\",\"message\":{:?}}}", e.to_string());
            } else {
                eprintln!("pin-reset failed: {e}");
            }
            std::process::exit(1);
        }
    }
}

pub(super) fn cmd_pin_status(json: bool) {
    match crate::security::tool_pin::current_pin() {
        Some(hash) => {
            if json {
                println!("{{\"status\":\"pinned\",\"hash\":\"{hash}\"}}");
            } else {
                println!("Tool manifest is pinned.");
                println!("  SHA-256: {hash}");
                println!("  Run `raios pin-reset` to clear and re-pin on next start.");
            }
        }
        None => {
            if json {
                println!("{{\"status\":\"unpinned\",\"message\":\"no pin file found\"}}");
            } else {
                println!("No pin file found — manifest is not yet pinned.");
                println!("Start `raios mcp-server` to create the initial pin.");
            }
        }
    }
}

pub(super) fn cmd_quarantine(action: crate::cli::QuarantineAction, json: bool) {
    use crate::cli::QuarantineAction::*;

    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {e}");
            std::process::exit(1);
        }
    };
    let _ = quarantine::ensure_table(&conn);

    match action {
        List => {
            let items = quarantine::list_pending(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("No pending quarantine items.");
                return;
            }
            println!("{:<20}  {:<25}  {}", "ID", "TOOL", "CREATED");
            for i in &items {
                println!("{:<20}  {:<25}  {}", i.id, i.tool, i.created_at);
            }
        }
        All => {
            let items = quarantine::list_all(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("No quarantine items found.");
                return;
            }
            println!("{:<20}  {:<25}  {:<10}  {}", "ID", "TOOL", "STATUS", "CREATED");
            for i in &items {
                println!("{:<20}  {:<25}  {:<10}  {}", i.id, i.tool, i.status, i.created_at);
            }
        }
        Approve { id } => match quarantine::approve(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"approved\",\"id\":\"{id}\"}}");
                } else {
                    println!("Approved {id}. Agent may now retry the tool call.");
                }
            }
            Ok(false) => {
                eprintln!("No pending item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => { eprintln!("DB error: {e}"); std::process::exit(1); }
        },
        Deny { id } => match quarantine::deny(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"denied\",\"id\":\"{id}\"}}");
                } else {
                    println!("Denied {id}. Future calls for this tool will be blocked.");
                }
            }
            Ok(false) => {
                eprintln!("No active item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => { eprintln!("DB error: {e}"); std::process::exit(1); }
        },
        Clear { id } => match quarantine::clear(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"cleared\",\"id\":\"{id}\"}}");
                } else {
                    println!("Cleared {id} from quarantine queue.");
                }
            }
            Ok(false) => {
                eprintln!("No item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => { eprintln!("DB error: {e}"); std::process::exit(1); }
        },
    }
}

pub(super) fn cmd_secret(action: crate::cli::SecretAction, json: bool) {
    use crate::cli::SecretAction::*;

    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {e}");
            std::process::exit(1);
        }
    };
    let _ = secret_lease::ensure_table(&conn);

    match action {
        Grant { tool, env_var, ttl } => {
            let ttl_secs = match secret_lease::parse_ttl(&ttl) {
                Ok(s) => s,
                Err(e) => { eprintln!("Invalid TTL: {e}"); std::process::exit(1); }
            };
            match secret_lease::grant(&conn, &tool, &env_var, ttl_secs) {
                Ok(id) => {
                    if json {
                        println!("{{\"status\":\"granted\",\"id\":\"{id}\",\"tool\":\"{tool}\",\"env_var\":\"{env_var}\",\"ttl_secs\":{ttl_secs}}}");
                    } else {
                        println!("Lease granted.");
                        println!("  ID:      {id}");
                        println!("  Tool:    {tool}");
                        println!("  Env var: {env_var}");
                        println!("  TTL:     {ttl} ({ttl_secs}s)");
                        println!();
                        println!("The env var will be injected when '{tool}' is called via MCP.");
                        println!("Run `raios secret revoke {id}` to revoke early.");
                    }
                }
                Err(e) => { eprintln!("DB error: {e}"); std::process::exit(1); }
            }
        }
        List => {
            let leases = secret_lease::list_active(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&leases).unwrap_or_default());
                return;
            }
            if leases.is_empty() {
                println!("No active secret leases.");
                return;
            }
            println!("{:<20}  {:<25}  {:<20}  {}", "ID", "TOOL", "ENV_VAR", "EXPIRES");
            for l in &leases {
                println!("{:<20}  {:<25}  {:<20}  {}", l.id, l.tool, l.env_var, l.expires_at);
            }
        }
        All => {
            let leases = secret_lease::list_all(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&leases).unwrap_or_default());
                return;
            }
            if leases.is_empty() {
                println!("No secret leases found.");
                return;
            }
            println!("{:<20}  {:<25}  {:<20}  {:<10}  {}", "ID", "TOOL", "ENV_VAR", "STATUS", "EXPIRES");
            for l in &leases {
                println!("{:<20}  {:<25}  {:<20}  {:<10}  {}", l.id, l.tool, l.env_var, l.status, l.expires_at);
            }
        }
        Revoke { id } => match secret_lease::revoke(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"revoked\",\"id\":\"{id}\"}}");
                } else {
                    println!("Lease {id} revoked.");
                }
            }
            Ok(false) => {
                eprintln!("No active lease with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => { eprintln!("DB error: {e}"); std::process::exit(1); }
        },
    }
}

