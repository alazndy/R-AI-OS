use super::{HandoffStatus, HandoffTarget};
use std::path::Path;

pub(super) fn cmd_handoff(
    to: HandoffTarget,
    status: HandoffStatus,
    msg: String,
    project_path: &Path,
    json: bool,
) {
    if let Some(label) = raios_core::security::looks_like_secret(&msg) {
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
    let diff_stat = raios_runtime::git_utils::diff_stat(project_path);

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Handoff failed: could not open control plane DB: {e}");
            std::process::exit(1);
        }
    };
    let trace_block =
        raios_runtime::trace_recall::relevant_trace_block(&conn, Some(&project_path_str), &msg, 3);
    let handoff_msg = match &trace_block {
        Some(block) => format!("{msg}\n\n{block}"),
        None => msg.clone(),
    };
    if let Some(label) = raios_core::security::looks_like_secret(&handoff_msg) {
        eprintln!(
            "Handoff refused: enriched context looks like it contains a {label}. \
             Remove it and resend — handoffs are stored in plain text (DB + process argv)."
        );
        std::process::exit(1);
    }

    let ids = match raios_core::db::create_handoff_workflow(
        &conn,
        &project_path_str,
        &from_agent,
        to_agent,
        status_str,
        &handoff_msg,
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
            "trace_memory_attached": trace_block.is_some(),
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
        if trace_block.is_some() {
            println!("  trace memory: attached");
        }
        println!(
            "  Next: {to_agent} will receive this as [HANDOVER CONTEXT] on its next `raios run`/`raios task`."
        );
        println!(
            "  If this carries complex context, call mempalace_create_tunnel yourself: {tunnel_hint}"
        );
    }
}
