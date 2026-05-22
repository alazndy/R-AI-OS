use std::path::Path;

pub(super) fn cmd_disk(project: Option<String>, dev_ops: &Path, json: bool) {
    use crate::core::disk;
    let reports = if project.is_none() {
        disk::analyze_all(dev_ops)
    } else {
        vec![disk::analyze(&super::resolve_project_path(project, dev_ops))]
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap_or_default());
        return;
    }
    println!("{:<32} {:>10} {:>10} {:>10} {:>6}", "PROJECT", "TOTAL", "SOURCE", "CACHE", "FILES");
    println!("{}", "─".repeat(72));
    for r in &reports {
        let name = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| r.path.display().to_string());
        println!("{:<32} {:>10} {:>10} {:>10} {:>6}", &name[..name.len().min(31)], disk::human_size(r.total_bytes), disk::human_size(r.source_bytes), disk::human_size(r.cache_bytes), r.file_count);
        for c in &r.cache_dirs {
            println!("  ↳ {:.<28} {:>10}  ({})", c.path.file_name().unwrap_or_default().to_string_lossy(), disk::human_size(c.bytes), c.kind);
        }
    }
    let total_cache: u64 = reports.iter().map(|r| r.cache_bytes).sum();
    if total_cache > 0 {
        println!("\n  Total reclaimable cache: {}", disk::human_size(total_cache));
        println!("  Run `raios clean --all` to free it");
    }
}

pub(super) fn cmd_clean(project: Option<String>, dry_run: bool, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::disk;
    let paths: Vec<std::path::PathBuf> = if all {
        crate::db::open_db().and_then(|conn| crate::db::load_all_projects(&conn))
            .map(|ps| ps.iter().map(|p| std::path::PathBuf::from(&p.path)).filter(|p| p.exists()).collect())
            .unwrap_or_default()
    } else {
        vec![super::resolve_project_path(project, dev_ops)]
    };
    let mut total_freed = 0u64;
    let prefix = if dry_run { "DRY RUN" } else { "CLEAN" };
    for path in &paths {
        let result = disk::clean(path, dry_run);
        total_freed += result.freed_bytes;
        if json {
            println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
        } else {
            for dir in &result.cleaned_dirs {
                let rel = dir.strip_prefix(path).unwrap_or(dir);
                println!("[{}] {} — {}", prefix, rel.display(), disk::human_size(result.freed_bytes / result.cleaned_dirs.len().max(1) as u64));
            }
            for e in &result.errors { eprintln!("  ✗ {}", e); }
        }
    }
    if !json {
        let action = if dry_run { "Would free" } else { "Freed" };
        println!("\n✓ {} {}", action, disk::human_size(total_freed));
    }
}

pub(super) fn cmd_ps(show_procs: bool, top: usize, json: bool) {
    use crate::core::process;
    let ports = process::list_ports();
    if json {
        println!("{}", serde_json::to_string_pretty(&ports).unwrap_or_default());
    } else {
        println!("{:<8} {:<10} {:<8} PROCESS", "PORT", "PID", "PROTO");
        println!("{}", "─".repeat(50));
        for p in &ports {
            let pid_s = p.pid.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
            let name = p.process_name.as_deref().unwrap_or("—");
            println!("{:<8} {:<10} {:<8} {}", p.port, pid_s, p.protocol, name);
        }
        if ports.is_empty() { println!("  No listening ports found"); }
    }
    if show_procs {
        let procs = process::list_processes(top);
        if json {
            println!("{}", serde_json::to_string_pretty(&procs).unwrap_or_default());
        } else {
            println!("\n{:<8} {:<6} {:<8} PROCESS", "PID", "CPU%", "MEM MB");
            println!("{}", "─".repeat(50));
            for p in &procs {
                let cpu = p.cpu_pct.map(|c| format!("{:.1}", c)).unwrap_or_else(|| "—".into());
                let mem = p.mem_mb.map(|m| format!("{:.1}", m)).unwrap_or_else(|| "—".into());
                println!("{:<8} {:<6} {:<8} {}", p.pid, cpu, mem, p.name);
            }
        }
    }
}

pub(super) fn cmd_kill_port(port: u16, json: bool) {
    let r = crate::core::process::kill_port(port);
    if json { println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default()); }
    else if r.ok { println!("✓ {}", r.message); } else { eprintln!("✗ {}", r.message); }
}

pub(super) fn cmd_env(project: Option<String>, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::env;
    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() { continue; }
                    let r = env::check(path);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let status = if r.ok { "✓" } else { "✗" };
                        let detail = match (r.missing_keys.len(), r.empty_keys.len()) {
                            (0, 0) if !r.has_env => "no .env".to_string(),
                            (0, 0) => format!("{} keys OK", r.total_env_keys),
                            (m, e) => format!("{} missing  {} empty", m, e),
                        };
                        println!("{} {:<30}  {}", status, p.name, detail);
                    }
                }
            }
        }
        return;
    }
    let path = super::resolve_project_path(project, dev_ops);
    let r = env::check(&path);
    if json { println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default()); return; }
    println!("Env files:");
    for f in &r.files { if f.exists { println!("  ✓ {:<25} {} keys", f.name, f.key_count); } }
    if !r.has_env { println!("  ✗ .env              MISSING"); }
    if !r.has_example { println!("  ✗ .env.example      MISSING — new devs can't onboard"); }
    if !r.missing_keys.is_empty() {
        println!("\n⚠  Missing keys ({}) — in .env.example but not in .env:", r.missing_keys.len());
        for k in &r.missing_keys { println!("    - {}", k); }
    }
    if !r.empty_keys.is_empty() {
        println!("\n⚠  Empty values ({}):", r.empty_keys.len());
        for k in &r.empty_keys { println!("    - {}=", k); }
    }
    if !r.undocumented_keys.is_empty() {
        println!("\nℹ  Undocumented keys ({}) — in .env but not in .env.example:", r.undocumented_keys.len());
        for k in &r.undocumented_keys { println!("    - {}", k); }
    }
    if r.ok { println!("\n✓ All env keys present and set"); }
}

pub(super) fn cmd_deps(project: Option<String>, _audit_only: bool, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::deps;
    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() { continue; }
                    let r = deps::check(path);
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let cve = if r.cve_critical > 0 { format!("🔴 {} crit", r.cve_critical) } else if r.cve_count > 0 { format!("⚠  {} cve", r.cve_count) } else { "✓ 0 cve".into() };
                        let out = if r.outdated_count > 0 { format!("⚠  {} outdated", r.outdated_count) } else { "✓ current".into() };
                        println!("{:<30} {}  {}", p.name, cve, out);
                    }
                }
            }
        }
        return;
    }
    let path = super::resolve_project_path(project, dev_ops);
    let report = deps::check(&path);
    print_deps_report(&report, json);
}

fn print_deps_report(r: &crate::core::deps::DepsReport, json: bool) {
    if json { println!("{}", serde_json::to_string_pretty(r).unwrap_or_default()); return; }
    println!("── {} ──  lockfile: {}", r.project_type, if r.has_lockfile { "✓" } else { "✗ MISSING" });
    if r.cve_count > 0 {
        println!("  🔴 {} CVE ({} critical)", r.cve_count, r.cve_critical);
        for v in &r.cve_issues { println!("    [{:>8}] {} {} — {}", v.severity.to_uppercase(), v.package, v.version, v.description); }
    } else { println!("  🔒 No known CVEs"); }
    if r.outdated_count > 0 {
        println!("  ⚠  {} outdated", r.outdated_count);
        for d in r.outdated.iter().take(10) { println!("    {:<30} {} → {}", d.name, d.current, d.latest); }
        if r.outdated_count > 10 { println!("    … and {} more", r.outdated_count - 10); }
    } else { println!("  ✓  All deps up to date"); }
    for msg in &r.tool_missing { println!("  ℹ  Tool not found: {}", msg); }
}

pub(super) fn cmd_build(project: Option<String>, release: bool, check: bool, dev_ops: &Path, json: bool) {
    use crate::core::build::{self, detect_type, ProjectType};
    let path = super::resolve_project_path(project, dev_ops);

    let result = match detect_type(&path) {
        ProjectType::Android if check => build::build_android_check(&path),
        ProjectType::Android if release => build::build_android_release(&path),
        ProjectType::Android => build::build_android(&path),
        _ => build::build(&path),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
        return;
    }
    let status = if result.ok { "✓ OK" } else { "✗ FAILED" };
    println!(
        "{} {} — {} in {}ms  ({} warnings, {} errors)",
        status, result.project_type, result.command, result.duration_ms,
        result.warnings, result.errors
    );
    for d in &result.diagnostics {
        let loc = d.line.map(|l| format!(":{}", l)).unwrap_or_default();
        println!("  [{}] {}{} — {}", d.level.to_uppercase(), d.file, loc, d.message);
    }
    if !result.ok && result.diagnostics.is_empty() {
        println!("{}", result.raw_output);
    }
}

pub(super) fn cmd_test(project: Option<String>, all: bool, dev_ops: &Path, json: bool) {
    use crate::core::build;
    if all {
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                let mut total_pass = 0usize;
                let mut total_fail = 0usize;
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() { continue; }
                    let r = build::test(path);
                    total_pass += r.passed; total_fail += r.failed;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let status = if r.ok { "✓" } else { "✗" };
                        println!("{} {:<30} {}/{} tests  {}ms", status, p.name, r.passed, r.passed + r.failed, r.duration_ms);
                        for f in &r.failures { println!("    ↳ {}", f); }
                    }
                }
                if !json { println!("\nTotal: {} passed, {} failed", total_pass, total_fail); }
            }
        }
        return;
    }
    let path = super::resolve_project_path(project, dev_ops);
    let result = build::test(&path);
    if json { println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default()); return; }
    let status = if result.ok { "✓" } else { "✗" };
    println!("{} {} — {} passed, {} failed, {} ignored  ({}ms)", status, result.command, result.passed, result.failed, result.ignored, result.duration_ms);
    for f in &result.failures { println!("  ↳ {}", f); }
    if !result.ok && result.failures.is_empty() { println!("{}", result.raw_output); }
}

pub(super) fn cmd_ci(project: Option<String>, dev_ops: &Path, json: bool) {
    use crate::core::ci::get_ci_status;
    let project_path = if let Some(ref name) = project {
        let projects = crate::entities::load_entities(dev_ops);
        let n = name.to_lowercase();
        let found = projects.into_iter().find(|p| p.name.to_lowercase().contains(&n) || p.local_path.to_string_lossy().to_lowercase().contains(&n));
        match found {
            Some(p) => p.local_path,
            None => { eprintln!("Project not found: {}", name); std::process::exit(1); }
        }
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    match get_ci_status(&project_path) {
        Ok(report) => print_ci_report(&report, json),
        Err(e) => eprintln!("CI error: {e}"),
    }
}

fn print_ci_report(report: &crate::core::ci::CiReport, json: bool) {
    if json {
        let out = serde_json::json!({
            "run": { "id": report.run.id, "workflow": report.run.workflow_name, "status": report.run.status, "conclusion": report.run.conclusion, "branch": report.run.branch, "created_at": report.run.created_at, "url": report.run.html_url },
            "jobs": report.jobs.iter().map(|j| serde_json::json!({"name": j.name, "status": j.status, "conclusion": j.conclusion, "duration_secs": j.duration_secs})).collect::<Vec<_>>()
        });
        match serde_json::to_string_pretty(&out) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }
    let status_icon = match report.run.conclusion.as_deref() {
        Some("success") => "✓", Some("failure") => "✗", Some("cancelled") => "○", _ => "…",
    };
    println!("CI: {} @ {} — {} {}  ({})", report.run.workflow_name, report.run.branch, status_icon,
        report.run.conclusion.as_deref().unwrap_or(&report.run.status),
        &report.run.created_at.get(..10).unwrap_or(&report.run.created_at));
    for job in &report.jobs {
        let icon = match job.conclusion.as_deref() { Some("success") => "✓", Some("failure") => "✗", Some("skipped") => "-", _ => "…" };
        let dur = match job.duration_secs { Some(s) if s >= 60 => format!("{}m {}s", s/60, s%60), Some(s) => format!("{}s", s), None => String::new() };
        if matches!(job.conclusion.as_deref(), Some("failure")) { println!("  {} {:<25} {}  ← FAILED", icon, job.name, dur); }
        else { println!("  {} {:<25} {}", icon, job.name, dur); }
    }
    if !report.run.html_url.is_empty() { println!("\n  {}", report.run.html_url); }
}
