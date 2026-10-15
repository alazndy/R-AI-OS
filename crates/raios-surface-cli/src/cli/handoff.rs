use super::{HandoffStatus, HandoffTarget};
use raios_core::db::HandoffReport;
use std::path::{Path, PathBuf};

pub(super) fn cmd_handoffs(command: super::HandoffsCmd, json: bool) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    let conn = raios_core::db::open_db()?;
    match command {
        super::HandoffsCmd::List { all, project, top } => {
            let project = project.map(|p| {
                Path::new(&p)
                    .canonicalize()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(p)
            });
            let mut rows = raios_core::db::cp_list_handoffs(&conn, all, project.as_deref(), top)?;
            for row in &mut rows {
                row.message = raios_core::security::redact_secrets(&row.message)
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .collect();
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
                return Ok(());
            }
            let color = std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && std::env::var("TERM").as_deref() != Ok("dumb");
            let (bold, dim, reset) = if color {
                ("\x1b[1m", "\x1b[90m", "\x1b[0m")
            } else {
                ("", "", "")
            };
            println!("\n{bold}HANDOFFS{reset} — {} shown\n", rows.len());
            for row in rows {
                let status =
                    row.user_status
                        .as_deref()
                        .unwrap_or(match row.delivery_status.as_str() {
                            "pending" => "queued",
                            "approved" => "delivered (work not confirmed)",
                            "rejected" => "rejected",
                            other => other,
                        });
                let status_color = if !color {
                    ""
                } else if status == "completed" {
                    "\x1b[32m"
                } else {
                    "\x1b[33m"
                };
                println!(
                    "{bold}{} → {}{reset}  {status_color}{status}{reset}",
                    row.from_agent, row.to_agent
                );
                println!(
                    "  Project: {}",
                    row.project.as_deref().unwrap_or("unregistered")
                );
                println!("  Directory: {}", row.directory);
                println!("  Created: {}", row.created_at);
                for line in row.message.lines().take(3) {
                    let preview: String = line.chars().take(160).collect();
                    println!(
                        "  {preview}{}",
                        if line.chars().count() > 160 {
                            "…"
                        } else {
                            ""
                        }
                    );
                }
                if row.message.lines().count() > 3
                    || row.message.lines().any(|line| line.chars().count() > 160)
                {
                    println!("  {dim}Use --json to read the full message.{reset}");
                }
                println!("  {dim}ID: {}{reset}\n", row.id);
            }
        }
        super::HandoffsCmd::Done { id } => {
            raios_core::db::cp_manage_handoff(&conn, &id, false)?;
            if json {
                println!("{}", serde_json::json!({"id":id,"user_status":"completed"}));
            } else {
                println!("Work confirmed completed: {id}");
            }
        }
        super::HandoffsCmd::Remove { id } => {
            raios_core::db::cp_manage_handoff(&conn, &id, true)?;
            if json {
                println!("{}", serde_json::json!({"id":id,"user_status":"removed"}));
            } else {
                println!("Handoff removed from queue and default list: {id}. History retained with --all.");
            }
        }
    }
    Ok(())
}

/// Scans every free-text field of a `HandoffReport` for secrets, not just
/// `findings` — evidence/edge-cases/open-questions/not-checked are all
/// agent-authored free text and can just as easily carry a pasted token.
fn report_secret_label(report: &HandoffReport) -> Option<&'static str> {
    let mut joined = report.findings.clone();
    for field in report
        .evidence
        .iter()
        .chain(report.edge_cases_considered.iter())
        .chain(report.open_questions.iter())
        .chain(report.what_i_did_not_check.iter())
    {
        joined.push('\n');
        joined.push_str(field);
    }
    raios_core::security::looks_like_secret(&joined)
}

/// Resolves the mutually-exclusive `--msg` / `--report` pair into a single
/// `HandoffReport`. Back-compat: a bare `--msg` becomes
/// `HandoffReport { findings: msg, ..Default::default() }`. Exits the process
/// with a clear message on misuse (neither given, both given, unreadable or
/// malformed `--report` file) — the same "fail loud, fail early" pattern the
/// rest of this command already uses for secret-detection refusals.
fn resolve_report(msg: Option<String>, report_path: Option<PathBuf>) -> HandoffReport {
    match (msg, report_path) {
        (Some(_), Some(_)) => {
            eprintln!("Handoff failed: pass either --msg or --report, not both.");
            std::process::exit(1);
        }
        (None, None) => {
            eprintln!(
                "Handoff failed: either --msg <text> or --report <path-to-json> is required."
            );
            std::process::exit(1);
        }
        (Some(msg), None) => HandoffReport {
            findings: msg,
            ..Default::default()
        },
        (None, Some(path)) => {
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "Handoff failed: could not read --report file {}: {e}",
                        path.display()
                    );
                    std::process::exit(1);
                }
            };
            match serde_json::from_str::<HandoffReport>(&content) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!(
                        "Handoff failed: --report file {} is not a valid HandoffReport JSON: {e}",
                        path.display()
                    );
                    std::process::exit(1);
                }
            }
        }
    }
}

pub(super) fn cmd_handoff(
    to: HandoffTarget,
    status: HandoffStatus,
    msg: Option<String>,
    report_path: Option<PathBuf>,
    project_path: &Path,
    json: bool,
) {
    let mut report = resolve_report(msg, report_path);

    if let Some(label) = report_secret_label(&report) {
        eprintln!(
            "Handoff refused: message looks like it contains a {label}. \
             Remove it and resend — handoffs are stored in plain text (DB + process argv)."
        );
        std::process::exit(1);
    }

    let from_agent =
        std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());
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
    let trace_block = raios_runtime::trace_recall::relevant_trace_block(
        &conn,
        Some(&project_path_str),
        &report.findings,
        3,
    );
    if let Some(block) = &trace_block {
        report.evidence.push(block.clone());
    }
    if let Some(label) = report_secret_label(&report) {
        eprintln!(
            "Handoff refused: enriched context looks like it contains a {label}. \
             Remove it and resend — handoffs are stored in plain text (DB + process argv)."
        );
        std::process::exit(1);
    }

    let ids = match raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: &project_path_str,
            from_agent: &from_agent,
            to_agent,
            status: status_str,
            msg: &report.findings,
            diff_stat: diff_stat.as_deref(),
            report: Some(&report),
        },
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
            "report": serde_json::to_value(&report).ok(),
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
        println!("  confidence:  {:.0}%", report.confidence * 100.0);
        println!(
            "  Queued: the receiving agent has not started. Use `raios run` in this project, or add --run to handoff to start it immediately."
        );
        println!(
            "  If this carries complex context, call mempalace_create_tunnel yourself: {tunnel_hint}"
        );
    }
}
