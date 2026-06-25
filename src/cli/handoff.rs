use super::{HandoffStatus, HandoffTarget};
use std::path::Path;
use std::process::Command;

pub(super) fn cmd_handoff(
    to: HandoffTarget,
    status: HandoffStatus,
    msg: String,
    project_path: &Path,
    json: bool,
) {
    if let Some(label) = looks_like_secret(&msg) {
        eprintln!(
            "Handoff refused: message looks like it contains a {label}. \
             Remove it and resend — handoffs are stored in plain text (DB + process argv)."
        );
        std::process::exit(1);
    }

    let from_agent = std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());
    let to_agent = to.as_str();
    let status_str = status.as_str();
    let project_path_str = project_path.to_string_lossy();
    let diff_stat = git_diff_stat(project_path);

    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Handoff failed: could not open control plane DB: {e}");
            std::process::exit(1);
        }
    };

    let ids = match crate::db::create_handoff_workflow(
        &conn,
        &project_path_str,
        &from_agent,
        to_agent,
        status_str,
        &msg,
        diff_stat.as_deref(),
    ) {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("Handoff failed: {e}");
            std::process::exit(1);
        }
    };

    // MemPalace cross-wing tunneling is an MCP-only capability (mempalace_create_tunnel) —
    // the mempalace CLI has no equivalent subcommand, and raios is not an MCP client. The
    // issuing agent already holds that tool, so we hand back a hint instead of shelling out.
    let tunnel_hint = format!(
        "wing_{from_agent} diary -> wing_{to_agent} diary",
        from_agent = from_agent,
        to_agent = to_agent
    );

    if json {
        let out = serde_json::json!({
            "handoff": "ok",
            "from": from_agent,
            "to": to_agent,
            "status": status_str,
            "task_id": ids.task_id,
            "agent_run_id": ids.agent_run_id,
            "artifact_id": ids.artifact_id,
            "approval_id": ids.approval_id,
            "project_id": ids.project_id,
            "diff_stat": diff_stat,
            "tunnel": "agent_action_required",
            "tunnel_hint": tunnel_hint,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        println!("Handoff recorded: {from_agent} -> {to_agent} ({status_str})");
        println!("  task_id:     {}", ids.task_id);
        println!("  approval_id: {}", ids.approval_id);
        if diff_stat.is_some() {
            println!("  diff stat:   attached (git diff --stat vs HEAD)");
        }
        println!(
            "  Next: {to_agent} will receive this as [HANDOVER CONTEXT] on its next `raios run`/`raios task`."
        );
        println!(
            "  If this carries complex context, call mempalace_create_tunnel yourself: {tunnel_hint}"
        );
    }
}

/// Best-effort `git diff --stat HEAD` in the target project — shows the receiving agent
/// what changed without the sender having to type a file list by hand. `None` on any
/// failure (not a git repo, no HEAD yet, git not installed): never blocks the handoff.
fn git_diff_stat(project_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(["diff", "--stat", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stat = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stat.is_empty() {
        None
    } else {
        Some(stat)
    }
}

/// Heuristic scan for obvious secrets (API keys, tokens, private keys) in a handoff
/// message. Not exhaustive — a deterrent against accidental paste, not a DLP system.
fn looks_like_secret(text: &str) -> Option<&'static str> {
    let patterns: &[(&str, &str)] = &[
        (r"AKIA[0-9A-Z]{16}", "AWS access key"),
        (r"sk-ant-[A-Za-z0-9_-]{20,}", "Anthropic API key"),
        (r"sk-[A-Za-z0-9]{20,}", "OpenAI-style API key"),
        (r"gh[pousr]_[A-Za-z0-9]{36,}", "GitHub token"),
        (r"github_pat_[A-Za-z0-9_]{20,}", "GitHub fine-grained PAT"),
        (
            r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----",
            "private key block",
        ),
        (
            r"(?i)(api[_-]?key|secret|password|token)\s*[=:]\s*['\x22]?[A-Za-z0-9_\-/+]{12,}",
            "key/secret/password/token assignment",
        ),
    ];
    for (pattern, label) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if re.is_match(text) {
                return Some(label);
            }
        }
    }
    None
}
