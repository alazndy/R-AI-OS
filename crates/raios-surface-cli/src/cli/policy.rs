use super::*;
use raios_core::security::{render_rules_toml, suggest_policy_rules, PolicyConfig};

pub(super) fn cmd_policy(action: PolicyCmd, json: bool) {
    match action {
        PolicyCmd::Suggest { min_count, apply } => cmd_suggest(min_count, apply, json),
        PolicyCmd::Show => cmd_show(json),
        PolicyCmd::Caps { tool } => cmd_caps(tool, json),
        PolicyCmd::Simulate { tool, args } => cmd_simulate(tool, args, json),
    }
}

fn cmd_simulate(tool: String, args: Option<String>, json: bool) {
    let umai = raios_core::security::Umai::from_default_policy();
    let decision = umai.check(&tool, args.as_deref());
    let rule_source = umai.rule_source(&tool);
    let caps = raios_core::security::capabilities::resolve(&tool, umai.tool_capabilities(&tool));
    let is_path_resolving = raios_core::security::capabilities::PATH_RESOLVING_TOOLS.contains(&tool.as_str());

    let (decision_label, reason): (&str, Option<String>) = match &decision {
        raios_core::security::UmaiDecision::Allow => ("allow", None),
        raios_core::security::UmaiDecision::Deny(r) => ("deny", Some(r.clone())),
        raios_core::security::UmaiDecision::Confirm(r) => ("confirm", Some(r.clone())),
    };

    if json {
        let out = serde_json::json!({
            "tool": tool,
            "decision": decision_label,
            "reason": reason,
            "rule_source": rule_source,
            "capabilities": {
                "fs_read": caps.fs_read,
                "fs_write": caps.fs_write,
                "network": caps.network,
                "exec": caps.exec,
            },
            "path_resolution_simulated": false,
            "note": if is_path_resolving {
                Some("this tool resolves a real project path at call time — capability enforcement against that path is not simulated here, only the declared capability is shown")
            } else {
                None
            },
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    let (icon, color) = match decision_label {
        "allow" => ("✓", "\x1b[32m"),
        "confirm" => ("?", "\x1b[33m"),
        _ => ("✗", "\x1b[31m"),
    };
    println!();
    println!("  {color}{icon} {}\x1b[0m  →  {}", tool, decision_label.to_uppercase());
    if let Some(r) = &reason {
        println!("    reason: {r}");
    }
    println!("    rule source: {rule_source}");
    println!(
        "    capabilities: fs_read={} fs_write={} network=[{}] exec={}",
        !caps.fs_read.is_empty(),
        !caps.fs_write.is_empty(),
        caps.network.join(","),
        caps.exec
    );
    if is_path_resolving {
        println!(
            "    \x1b[90mnote: this tool resolves a real project path at call time — that resolution isn't simulated here, only the declared capability is shown\x1b[0m"
        );
    }
    println!();
}

fn cmd_caps(tool: Option<String>, json: bool) {
    let umai = raios_core::security::Umai::from_default_policy();
    let tools: Vec<String> = match &tool {
        Some(t) => vec![t.clone()],
        None => raios_core::security::capabilities::ALL_TOOLS.iter().map(|s| s.to_string()).collect(),
    };

    if json {
        let out: Vec<_> = tools
            .iter()
            .map(|t| {
                let override_caps = umai.tool_capabilities(t);
                let caps = raios_core::security::capabilities::resolve(t, override_caps.clone());
                serde_json::json!({
                    "tool": t,
                    "fs_read": caps.fs_read,
                    "fs_write": caps.fs_write,
                    "network": caps.network,
                    "exec": caps.exec,
                    "source": if override_caps.is_some() { "toml_override" } else { "default" },
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    println!();
    for t in &tools {
        let override_caps = umai.tool_capabilities(t);
        let source = if override_caps.is_some() { "toml" } else { "default" };
        let caps = raios_core::security::capabilities::resolve(t, override_caps);
        println!(
            "  {:<32} fs_read={:<6} fs_write={:<6} network={:<20} exec={:<5} [{source}]",
            t,
            !caps.fs_read.is_empty(),
            !caps.fs_write.is_empty(),
            caps.network.join(","),
            caps.exec,
        );
    }
    println!();
}

fn cmd_suggest(min_count: usize, apply: bool, json: bool) {
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };

    let existing = PolicyConfig::try_load_default();
    let suggestions = match suggest_policy_rules(&conn, existing.as_ref(), min_count) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to analyze audit_log: {e}");
            return;
        }
    };

    if json {
        let out = serde_json::json!({
            "allow": suggestions.allow.iter().map(|s| serde_json::json!({
                "tool": s.tool, "count": s.count, "total": s.total,
            })).collect::<Vec<_>>(),
            "deny": suggestions.deny.iter().map(|s| serde_json::json!({
                "tool": s.tool, "count": s.count, "total": s.total,
            })).collect::<Vec<_>>(),
            "needs_review": suggestions.needs_review.iter().map(|n| serde_json::json!({
                "tool": n.tool, "confirm_count": n.confirm_count, "total": n.total,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        print_suggestions_human(&suggestions, min_count);
    }

    if apply {
        apply_suggestions(&suggestions, json);
    }
}

fn print_suggestions_human(s: &raios_core::security::PolicySuggestions, min_count: usize) {
    if s.allow.is_empty() && s.deny.is_empty() && s.needs_review.is_empty() {
        println!(
            "  No policy suggestions yet — not enough audit history (need >= {min_count} decisions per tool)."
        );
        return;
    }
    if !s.allow.is_empty() {
        println!("\n  \x1b[32mSuggested ALLOW rules\x1b[0m (pins current default-allow behavior):");
        for sug in &s.allow {
            println!(
                "    [[tools.rules]]  name = \"{}\"  action = \"allow\"   \x1b[90m({}/{} decisions)\x1b[0m",
                sug.tool, sug.count, sug.total
            );
        }
    }
    if !s.deny.is_empty() {
        println!("\n  \x1b[31mSuggested DENY rules\x1b[0m (pins current default-deny behavior):");
        for sug in &s.deny {
            println!(
                "    [[tools.rules]]  name = \"{}\"  action = \"deny\"   \x1b[90m({}/{} decisions)\x1b[0m",
                sug.tool, sug.count, sug.total
            );
        }
    }
    if !s.needs_review.is_empty() {
        println!("\n  \x1b[33mNeeds human review\x1b[0m (frequently hits confirm — not auto-suggested):");
        for n in &s.needs_review {
            println!(
                "    {}   \x1b[90m({}/{} confirm decisions)\x1b[0m",
                n.tool, n.confirm_count, n.total
            );
        }
    }
    println!();
}

fn apply_suggestions(s: &raios_core::security::PolicySuggestions, json: bool) {
    if s.allow.is_empty() && s.deny.is_empty() {
        if !json {
            println!("  Nothing to apply.");
        }
        return;
    }
    let path = match PolicyConfig::default_path() {
        Some(p) => p,
        None => {
            eprintln!(
                "  \x1b[31m✗\x1b[0m  No raios-policy.toml found (checked cwd and ~/.config/raios/) — create one before applying suggestions."
            );
            return;
        }
    };

    let mut block = render_rules_toml(&s.allow);
    block.push_str(&render_rules_toml(&s.deny));

    use std::io::Write;
    let result = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(block.as_bytes()));

    match result {
        Ok(()) => {
            let count = s.allow.len() + s.deny.len();
            if json {
                println!("{{\"applied\":true,\"rules\":{count},\"path\":\"{}\"}}", path.display());
            } else {
                println!("  \x1b[32m✓\x1b[0m  Applied {count} rule(s) to {}", path.display());
            }
        }
        Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  Failed to write {}: {e}", path.display()),
    }
}

fn cmd_show(json: bool) {
    match PolicyConfig::try_load_default() {
        Some(cfg) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
            } else {
                println!("\n  Default action: {}\n", cfg.tools.default_action.as_str());
                for r in &cfg.tools.rules {
                    println!("  {:<32} {}", r.name, r.action.as_str());
                }
                println!();
            }
        }
        None => {
            if json {
                println!("{{\"loaded\":false}}");
            } else {
                println!("  No raios-policy.toml found (checked cwd and ~/.config/raios/).");
            }
        }
    }
}
