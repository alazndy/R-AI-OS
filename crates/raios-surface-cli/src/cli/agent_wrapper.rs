use raios_runtime::agent_wrapper as aw;

pub enum AgentWrapperAction {
    Install { agents: Vec<String> },
    Remove { agents: Vec<String> },
    Status,
}

pub fn cmd_agent_wrapper(action: AgentWrapperAction, json: bool) {
    match action {
        AgentWrapperAction::Install { agents } => {
            let targets: Vec<&str> = if agents.is_empty() {
                aw::ALL_AGENTS.to_vec()
            } else {
                agents.iter().map(|s| s.as_str()).collect()
            };
            let results = aw::install(&targets);
            if json {
                let items: Vec<_> = results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "desc": r.desc,
                            "ok": r.ok,
                            "skipped": r.skipped
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                );
            } else {
                for r in &results {
                    let icon = if r.skipped {
                        "○"
                    } else if r.ok {
                        "✓"
                    } else {
                        "✗"
                    };
                    println!("  {} {}", icon, r.desc);
                }
            }
        }
        AgentWrapperAction::Remove { agents } => {
            let filter: Option<Vec<&str>> = if agents.is_empty() {
                None
            } else {
                Some(agents.iter().map(|s| s.as_str()).collect())
            };
            let results = aw::remove(filter.as_deref());
            if json {
                let items: Vec<_> = results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "desc": r.desc,
                            "ok": r.ok,
                            "skipped": r.skipped
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                );
            } else {
                for r in &results {
                    let icon = if r.skipped {
                        "○"
                    } else if r.ok {
                        "✓"
                    } else {
                        "✗"
                    };
                    println!("  {} {}", icon, r.desc);
                }
            }
        }
        AgentWrapperAction::Status => {
            let statuses = aw::status();
            if json {
                let items: Vec<_> = statuses
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "agent": s.agent,
                            "installed": s.installed,
                            "real_found": s.real_found,
                            "rc_file": s.rc_file
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                );
                return;
            }

            let rc_files = aw::detect_rc_paths();
            let rc_label = rc_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");

            println!("\n  AGENT WRAPPER STATUS");
            println!("  RC file: {}\n", rc_label);
            println!("  {:<12} {:<12} {:<12}", "AGENT", "WRAPPED", "BINARY FOUND");
            println!("  {}", "─".repeat(38));
            for s in &statuses {
                let wrapped = if s.installed { "✓ yes" } else { "✗ no" };
                let found = if s.real_found {
                    "✓ yes"
                } else {
                    "— not found"
                };
                println!("  {:<12} {:<12} {}", s.agent, wrapped, found);
            }
            println!();

            let any_installed = statuses.iter().any(|s| s.installed);
            if any_installed {
                println!("  Remove all: raios agent-wrapper remove");
            } else {
                println!("  Install all: raios agent-wrapper install");
            }
            println!();
        }
    }
}
