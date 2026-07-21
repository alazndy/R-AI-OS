use raios_core::security::license::{scan_licenses, LicenseReport};
use std::path::Path;

pub fn cmd_license(project: Option<String>, dev_ops: &Path, json_out: bool) {
    let path = raios_surface_cli::cli::resolve_project_path(project, dev_ops);
    let report = scan_licenses(&path);

    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
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
            let tag = if dep.is_copyleft {
                "COPYLEFT"
            } else {
                "UNKNOWN "
            };
            println!(
                "    [{}] {} {} — {}",
                tag, dep.name, dep.version, dep.license
            );
        }
    }
    println!();
}

pub fn cmd_verify_chain(last: usize, json: bool) {
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {}", e);
            std::process::exit(1);
        }
    };

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

    match raios_core::security::verify_chain(&conn) {
        Ok(n) => {
            if json {
                println!("{{\"status\":\"ok\",\"entries_verified\":{}}}", n);
            } else {
                println!(
                    "✅ Audit chain OK — {} entries verified, no tampering detected.",
                    n
                );
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

pub fn cmd_rate_status(json: bool) {
    let config = raios_core::security::PolicyConfig::try_load_default();

    match config.and_then(|c| c.rate_limits) {
        None => {
            if json {
                println!("{{\"enabled\":false,\"source\":\"no raios-policy.toml found or no [rate_limits] section\"}}");
            } else {
                println!(
                    "Rate limiting: DISABLED (no raios-policy.toml or no [rate_limits] section)"
                );
                println!("Create raios-policy.toml with [rate_limits] to enable.");
            }
        }
        Some(cfg) => {
            if json {
                let rules: Vec<serde_json::Value> = cfg
                    .rules
                    .iter()
                    .map(|r| serde_json::json!({ "tool": r.tool, "max_calls": r.max_calls }))
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "enabled": cfg.enabled,
                        "window_secs": cfg.window_secs,
                        "default_max": cfg.default_max,
                        "rules": rules,
                    })
                );
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
                        let label = if r.tool == "*" {
                            "(all tools)"
                        } else {
                            &r.tool
                        };
                        println!(
                            "    {label:<30}  max {} calls/{}s",
                            r.max_calls, cfg.window_secs
                        );
                    }
                }
                println!();
                println!("Note: live call counts are tracked per MCP server process.");
            }
        }
    }
}

pub fn cmd_pin_reset(json: bool) {
    match raios_core::security::tool_pin::reset_pin() {
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

pub fn cmd_pin_status(json: bool) {
    match raios_core::security::tool_pin::current_pin() {
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
