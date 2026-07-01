//! Learns policy suggestions from the audit ledger (`raios policy suggest`).
//!
//! Reads `tool_allow` / `tool_deny` / `tool_confirm` decisions written by
//! `record_tool_decision` (see `verify_chain.rs`) and proposes new
//! `[[tools.rules]]` entries. This never suggests *escalating* a tool's
//! privilege — it only pins down whatever the effective `default_action`
//! already produced for that tool, so applying a suggestion cannot grant new
//! access the audit trail didn't already show happening. Tools that mostly
//! hit `confirm` are surfaced as a review note instead of an auto-suggestion,
//! since the ledger has no record of whether a human ever approved them.

use anyhow::Result;
use rusqlite::Connection;

use super::policy::{PolicyAction, PolicyConfig};

#[derive(Debug, Clone, PartialEq)]
pub struct PolicySuggestion {
    pub tool: String,
    pub action: PolicyAction,
    pub count: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewNote {
    pub tool: String,
    pub confirm_count: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolicySuggestions {
    pub allow: Vec<PolicySuggestion>,
    pub deny: Vec<PolicySuggestion>,
    pub needs_review: Vec<ReviewNote>,
}

#[derive(Default)]
struct ToolStats {
    allow: usize,
    deny: usize,
    confirm: usize,
}

/// Analyzes `audit_log` and proposes `[[tools.rules]]` entries.
///
/// Only decisions where `matched_rule == "default"` are considered — a tool
/// with an existing explicit rule already has a human-authored answer, so
/// there is nothing to suggest for it. `min_count` is the minimum number of
/// qualifying decisions (allow + deny + confirm) before a tool is surfaced at
/// all, to avoid suggesting rules from a handful of one-off calls.
pub fn suggest_policy_rules(
    conn: &Connection,
    existing: Option<&PolicyConfig>,
    min_count: usize,
) -> Result<PolicySuggestions> {
    let mut stmt = conn.prepare(
        "SELECT event_type, data FROM audit_log \
         WHERE event_type IN ('tool_allow', 'tool_deny', 'tool_confirm')",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);

    let mut stats: std::collections::BTreeMap<String, ToolStats> = std::collections::BTreeMap::new();
    for (event_type, data) in &rows {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) else { continue };
        let Some(tool) = parsed["tool"].as_str() else { continue };
        if parsed["matched_rule"].as_str() != Some("default") {
            continue; // an explicit rule already governs this tool
        }
        let entry = stats.entry(tool.to_string()).or_default();
        match event_type.as_str() {
            "tool_allow" => entry.allow += 1,
            "tool_deny" => entry.deny += 1,
            "tool_confirm" => entry.confirm += 1,
            _ => {}
        }
    }

    let mut out = PolicySuggestions::default();
    for (tool, s) in stats {
        let total = s.allow + s.deny + s.confirm;
        if total < min_count {
            continue;
        }
        if let Some(cfg) = existing {
            if cfg.tools.rules.iter().any(|r| r.name == tool) {
                continue; // already has an explicit rule
            }
        }
        if s.confirm >= s.allow && s.confirm >= s.deny {
            out.needs_review.push(ReviewNote { tool, confirm_count: s.confirm, total });
        } else if s.allow >= s.deny {
            out.allow.push(PolicySuggestion { tool, action: PolicyAction::Allow, count: s.allow, total });
        } else {
            out.deny.push(PolicySuggestion { tool, action: PolicyAction::Deny, count: s.deny, total });
        }
    }

    Ok(out)
}

/// Renders accepted suggestions as `[[tools.rules]]` TOML blocks, ready to be
/// appended to `raios-policy.toml`. Appending (rather than re-serializing the
/// whole `PolicyConfig`) preserves the hand-authored file's comments and
/// section ordering; `[[tools.rules]]` array entries are valid anywhere in a
/// TOML document, so appending after other sections (e.g. `[egress]`) still
/// merges correctly into the `tools.rules` array on next parse.
pub fn render_rules_toml(suggestions: &[PolicySuggestion]) -> String {
    let mut block = String::new();
    for s in suggestions {
        block.push_str(&format!(
            "\n[[tools.rules]]\nname = \"{}\"\naction = \"{}\"\n",
            s.tool,
            s.action.as_str()
        ));
    }
    block
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use raios_core::security::policy::{FilesystemPolicy, ToolRule, ToolsPolicy};
    use raios_core::security::verify_chain::record_tool_decision;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE audit_log (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp  TEXT NOT NULL,
                event_type TEXT NOT NULL,
                actor      TEXT NOT NULL DEFAULT 'raios',
                data       TEXT NOT NULL DEFAULT '',
                prev_hash  TEXT NOT NULL DEFAULT '',
                hash       TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn empty_policy() -> PolicyConfig {
        PolicyConfig {
            server: None,
            filesystem: FilesystemPolicy { enforce_sandbox: false, allowed_paths: vec![], blocked_paths: vec![] },
            tools: ToolsPolicy { default_action: PolicyAction::Confirm, rules: vec![] },
            preflight: None,
            egress: None,
            rate_limits: None,
            quarantine: None,
        }
    }

    #[test]
    fn suggests_allow_after_repeated_default_allows() {
        let conn = in_memory_db();
        for _ in 0..25 {
            record_tool_decision(&conn, "list_projects", "h", "default", "tool_allow", "claude_kaira").unwrap();
        }
        let out = suggest_policy_rules(&conn, None, 20).unwrap();
        assert_eq!(out.allow, vec![PolicySuggestion {
            tool: "list_projects".into(), action: PolicyAction::Allow, count: 25, total: 25,
        }]);
        assert!(out.deny.is_empty());
        assert!(out.needs_review.is_empty());
    }

    #[test]
    fn suggests_deny_after_repeated_default_denies() {
        let conn = in_memory_db();
        for _ in 0..20 {
            record_tool_decision(&conn, "dangerous_tool", "h", "default", "tool_deny", "claude_kaira").unwrap();
        }
        let out = suggest_policy_rules(&conn, None, 20).unwrap();
        assert_eq!(out.deny.len(), 1);
        assert_eq!(out.deny[0].tool, "dangerous_tool");
        assert_eq!(out.deny[0].action, PolicyAction::Deny);
    }

    #[test]
    fn confirm_heavy_tool_is_a_review_note_not_an_allow_suggestion() {
        let conn = in_memory_db();
        for _ in 0..30 {
            record_tool_decision(&conn, "run_build", "h", "default", "tool_confirm", "claude_kaira").unwrap();
        }
        let out = suggest_policy_rules(&conn, None, 20).unwrap();
        assert!(out.allow.is_empty(), "confirm-heavy tools must never be auto-suggested as allow");
        assert!(out.deny.is_empty());
        assert_eq!(out.needs_review.len(), 1);
        assert_eq!(out.needs_review[0].tool, "run_build");
        assert_eq!(out.needs_review[0].confirm_count, 30);
    }

    #[test]
    fn below_min_count_is_not_suggested() {
        let conn = in_memory_db();
        for _ in 0..5 {
            record_tool_decision(&conn, "rarely_used", "h", "default", "tool_allow", "claude_kaira").unwrap();
        }
        let out = suggest_policy_rules(&conn, None, 20).unwrap();
        assert!(out.allow.is_empty());
    }

    #[test]
    fn tool_with_existing_explicit_rule_is_skipped() {
        let conn = in_memory_db();
        for _ in 0..25 {
            record_tool_decision(&conn, "already_ruled", "h", "default", "tool_allow", "claude_kaira").unwrap();
        }
        let mut cfg = empty_policy();
        cfg.tools.rules.push(ToolRule { name: "already_ruled".into(), action: PolicyAction::Deny, capabilities: None });
        let out = suggest_policy_rules(&conn, Some(&cfg), 20).unwrap();
        assert!(out.allow.is_empty(), "a tool with an explicit rule already has a human-authored answer");
    }

    #[test]
    fn decisions_matched_by_an_explicit_rule_are_ignored_even_without_config() {
        // matched_rule != "default" means an explicit rule produced this decision
        // at record time — it must never feed the learning pipeline, regardless
        // of whether the caller happens to pass the config back in.
        let conn = in_memory_db();
        for _ in 0..25 {
            record_tool_decision(&conn, "explicit_tool", "h", "rule", "tool_allow", "claude_kaira").unwrap();
        }
        let out = suggest_policy_rules(&conn, None, 20).unwrap();
        assert!(out.allow.is_empty());
    }

    #[test]
    fn render_produces_idempotent_appendable_toml() {
        let suggestions = vec![
            PolicySuggestion { tool: "list_projects".into(), action: PolicyAction::Allow, count: 25, total: 25 },
            PolicySuggestion { tool: "dangerous_tool".into(), action: PolicyAction::Deny, count: 20, total: 20 },
        ];
        let rendered = render_rules_toml(&suggestions);

        // Simulate appending after an existing document that already ends in
        // a different table, to prove [[tools.rules]] still merges correctly.
        let base = "[filesystem]\nenforce_sandbox = false\nallowed_paths = []\nblocked_paths = []\n\n\
                     [tools]\ndefault_action = \"confirm\"\n\n\
                     [egress]\nenabled = true\ndeny_all = false\nallowed_domains = []\nblocked_domains = []\n";
        let full = format!("{base}{rendered}");
        let cfg: PolicyConfig = toml::from_str(&full).unwrap();
        assert_eq!(cfg.tool_action("list_projects"), &PolicyAction::Allow);
        assert_eq!(cfg.tool_action("dangerous_tool"), &PolicyAction::Deny);
    }
}
