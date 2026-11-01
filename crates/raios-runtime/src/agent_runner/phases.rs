//! The steps of `run_agent_with_session_labels`, in the order a wrapper run goes through them.
//!
//! Each function is one phase of the run (pre-flight, handoff lookup and decision, building the
//! command, opening the session row, waiting for the agent, closing the session, post-session
//! memory work). The orchestration - and the `#[cfg(unix)]` wrapper-note socket, which is tied to
//! the child's lifetime - stays in `run_agent_with_session_labels`.

use super::*;
use raios_core::db::WrapperProcessIdentity;

/// A pending handoff plus what delivering it needs: the open connection, the receiving agent's
/// identity, the handoff itself and the context block shown to (and handed to) the agent.
pub(super) type PendingHandoff = (
    rusqlite::Connection,
    &'static str,
    raios_core::control_plane::HandoffContext,
    String,
);

/// Phase 1: shield warnings, and - when the policy enforces it - the pre-flight gate that can
/// refuse the run.
pub(super) fn preflight_gate(
    project_dir: Option<&str>,
    shield: &AgentShield,
) -> Result<(), String> {
    if let Some(dir) = project_dir {
        let warnings = shield.preflight_check(Path::new(dir));
        for warning in warnings {
            println!("{}", warning);
        }

        let skip_preflight = std::env::var("RAIOS_SKIP_PREFLIGHT").ok().as_deref() == Some("1");
        let enforce_before_run = raios_core::security::PolicyConfig::try_load_default()
            .and_then(|cfg| cfg.preflight.map(|p| p.enforce_before_run))
            .unwrap_or(false);
        if enforce_before_run && !skip_preflight {
            let checks = crate::cli::preflight::run_gate(
                Path::new(dir),
                crate::cli::preflight::PreflightMode::AgentRunGate,
            );
            let blockers: Vec<_> = checks.iter().filter(|c| !c.pass && c.blocking).collect();
            for c in &checks {
                let icon = if c.pass {
                    "✓"
                } else if c.blocking {
                    "✗"
                } else {
                    "⚠"
                };
                let detail = if c.detail.is_empty() {
                    String::new()
                } else {
                    format!("  {}", c.detail)
                };
                println!("  {}  {:<28}{}", icon, c.label, detail);
            }
            if !blockers.is_empty() {
                println!("  Preflight enforcement blocked this agent run. See `raios pre-flight` for the full commit gate.");
                return Err("Preflight blocked agent run; fix the findings above first.".into());
            }
        } else if skip_preflight {
            println!("⚠ Preflight enforcement bypassed via RAIOS_SKIP_PREFLIGHT=1");
        }
    }
    Ok(())
}

/// Effective project directory: the explicit flag, else the current directory, canonicalized when
/// possible.
pub(super) fn resolve_project_dir(project_dir: Option<String>) -> Option<String> {
    project_dir
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .map(|p| {
            Path::new(&p)
                .canonicalize()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(p)
        })
}

/// Phase 3: the control-plane handoff addressed to this agent in this project, with the context
/// block to hand to the agent. The connection stays open: delivery is recorded on it later.
pub(super) fn lookup_pending_handoff(
    agent: &str,
    extra_args: &[String],
    project_dir: Option<&str>,
) -> Option<PendingHandoff> {
    if is_informational_invocation(extra_args) {
        None
    } else {
        canonical_agent_identity(agent).and_then(|identity| {
            let conn = match raios_core::db::open_db() {
                Ok(conn) => conn,
                Err(error) => {
                    eprintln!("Warning: could not read pending handoffs: {error}");
                    return None;
                }
            };
            let ctx = match pending_handoff_for_project(&conn, project_dir?, identity) {
                Ok(ctx) => ctx?,
                Err(error) => {
                    eprintln!("Warning: could not select pending handoff: {error}");
                    return None;
                }
            };
            let mut block = format!(
                "[HANDOVER CONTEXT]\nFrom: {}  Status: {}\n{}",
                ctx.from_agent, ctx.status, ctx.context_summary
            );
            if let Some(diff_stat) = &ctx.diff_stat {
                block.push_str(&format!("\n\n[Changed files since handoff]\n{diff_stat}"));
            }
            if let Some(trace_block) = crate::trace_recall::relevant_trace_block(
                &conn,
                project_dir,
                &ctx.context_summary,
                3,
            ) {
                if !block.contains("[Relevant trace memory]") {
                    block.push_str("\n\n");
                    block.push_str(&trace_block);
                }
            }
            Some((conn, identity, ctx, block))
        })
    }
}

/// Prints the handover context in a box before anything else is asked.
pub(super) fn print_handover_banner(block: &str) {
    let width = 62usize;
    let border = "═".repeat(width);
    println!("\n\x1b[1;33m╔{border}╗\x1b[0m");
    println!(
        "\x1b[1;33m║\x1b[0m  \x1b[1;33m✦ HANDOVER CONTEXT\x1b[0m{}\x1b[1;33m║\x1b[0m",
        " ".repeat(width - 20)
    );
    println!("\x1b[1;33m╠{border}╣\x1b[0m");
    for line in block.lines() {
        let truncated: String = line.chars().take(width - 2).collect();
        let pad = width.saturating_sub(truncated.chars().count() + 2);
        println!(
            "\x1b[33m║\x1b[0m  {}{} \x1b[33m║\x1b[0m",
            truncated,
            " ".repeat(pad)
        );
    }
    println!("\x1b[1;33m╚{border}╝\x1b[0m");
    println!();
}

/// Decides what to do with a pending handoff (accept, reject, mark completed, defer), asking on
/// an interactive terminal. Anything but accepting drops the handoff from this run.
pub(super) fn resolve_handoff_decision(
    pending_handoff: &mut Option<PendingHandoff>,
    handover_block: &mut Option<String>,
    handoff_action: HandoffAction,
) -> Result<(), String> {
    if pending_handoff.is_some() {
        // The owner may have already consented to this handoff in the control plane; asking
        // here again would be a second approval of the same decision. Delivery is still this
        // wrapper's own act, and an explicit action flag still wins over the default.
        let owner_approved = handoff_action == HandoffAction::Auto
            && pending_handoff
                .as_ref()
                .map(|(_, _, ctx, _)| ctx.approved_by_owner)
                .unwrap_or(false);
        let action = if owner_approved {
            println!("Handoff already approved by the control-plane owner; delivering.");
            HandoffAction::Accept
        } else {
            match handoff_action {
                HandoffAction::Auto if io::stdin().is_terminal() && io::stdout().is_terminal() => {
                    read_handoff_action(&mut io::stdin().lock(), &mut io::stdout().lock())
                        .map_err(|e| format!("Could not read handoff decision: {e}"))?
                }
                HandoffAction::Auto => HandoffAction::Accept,
                action => action,
            }
        };
        if matches!(action, HandoffAction::Reject | HandoffAction::Complete) {
            if let Some((conn, identity, ctx, _)) = pending_handoff.as_mut() {
                if action == HandoffAction::Reject {
                    let command = raios_contracts::Command::RejectHandoff {
                        approval_id: ctx.approval_id.clone(),
                        expected_revision: None,
                        reason: "Rejected by user in agent wrapper".into(),
                        idempotency_key: format!("wrapper-handoff-{}-reject", ctx.approval_id),
                    };
                    crate::control_plane::service::dispatch_control_command(
                        conn,
                        &crate::control_plane::service::ControlActor::local_session(),
                        &command,
                    )
                    .map_err(|e| format!("Could not resolve handoff: {}", e.message))?;
                } else {
                    // "Already completed": the addressed agent's own wrapper marks the
                    // delivery directly — never through the owner's approval. No context is
                    // injected, and the receipt reads delivered by the target.
                    if let Err(error) = raios_core::db::cp_consume_handoff(conn, ctx, identity) {
                        return Err(format!("Could not mark handoff completed: {error}"));
                    }
                }
            }
            println!(
                "Handoff {}. Starting without handoff context.",
                if action == HandoffAction::Reject {
                    "rejected"
                } else {
                    "marked completed"
                }
            );
        } else if action == HandoffAction::Defer {
            println!("Handoff left pending. Starting without handoff context.");
        }
        if action != HandoffAction::Accept {
            *pending_handoff = None;
            *handover_block = None;
        }
    }
    Ok(())
}

/// Builds the agent command, wiring the handover into the agent's native prompt flag and adding
/// the instincts, context mode and handover environment. Returns the command and the native
/// conversation id to resume it by.
pub(super) fn build_agent_command(
    agent: &str,
    extra_args: &[String],
    handover_block: &Option<String>,
    instinct: &InstinctEngine,
    budget_active: bool,
    project_dir: &Option<String>,
) -> Result<(Command, Option<String>), String> {
    let mut recovery_args = extra_args.to_vec();
    let native_id = crate::session_recovery::prepare_native_checkpoint(agent, &mut recovery_args);
    let mut cmd = handoff_agent_command(agent, handover_block.as_deref(), &recovery_args)?;

    // Inject Instincts & Budget Info
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }
    if budget_active {
        cmd.env("RAIOS_CONTEXT_MODE", "compact");
    }
    // Best-effort env fallback for "cursor" and any future agent without a native flag yet.
    if let Some(block) = handover_block {
        cmd.env("RAIOS_HANDOVER_CONTEXT", block);
    }

    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }
    Ok((cmd, native_id))
}

/// Opens the session row so every wrapper-routed run is traceable. Refuses to launch if it cannot
/// be persisted.
pub(super) fn start_wrapper_session(
    agent: &str,
    project_dir: &Option<String>,
    name: &Option<String>,
    work: &Option<String>,
    wrapper_owner: &Option<WrapperProcessIdentity>,
) -> Result<Option<(String, String)>, String> {
    let session_ids = {
        let identity = canonical_agent_identity(agent).unwrap_or(agent);
        let conn_res = raios_core::db::open_db();
        match conn_res {
            Ok(conn) => {
                let project_id = project_dir
                    .as_deref()
                    .and_then(|dir| raios_core::db::project_id_for_file_path(&conn, dir));
                match raios_core::db::cp_session_start_named(
                    &conn,
                    identity,
                    project_id,
                    project_dir.as_deref().unwrap_or("/"),
                    wrapper_owner.as_ref(),
                    name.as_deref(),
                    work.as_deref(),
                ) {
                    Ok(ids) => Some((ids.0, ids.1)),
                    Err(error) => {
                        return Err(format!("Could not persist session before launch: {error}"));
                    }
                }
            }
            Err(error) => {
                return Err(format!("Could not persist session before launch: {error}"));
            }
        }
    };
    Ok(session_ids)
}

/// Records which native conversation this wrapper session resumes.
pub(super) fn bind_native_resume(
    session_ids: &Option<(String, String)>,
    native_id: &Option<String>,
) -> Result<(), String> {
    if let (Some((task_id, run_id)), Some(native_id)) = (session_ids, native_id) {
        let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
        if let Err(error) = raios_core::db::cp_bind_wrapper_resume(&conn, run_id, native_id) {
            let _ = raios_core::db::cp_session_end(&conn, task_id, run_id, false);
            return Err(format!(
                "Could not persist native recovery identity: {error}"
            ));
        }
    }
    Ok(())
}

/// For claude: puts the session block into ~/.claude/CLAUDE.md so it is visible inside Claude
/// Code's own UI. Returns whether it did (and so must be stripped on exit).
pub(super) fn inject_claude_session_block(
    agent: &str,
    session_ids: &Option<(String, String)>,
    now_str: &str,
) -> bool {
    let injected_claude_md = agent.to_lowercase() == "claude";
    if injected_claude_md {
        if let Some((_, run_id)) = session_ids {
            let identity = canonical_agent_identity(agent).unwrap_or(agent);
            inject_session_to_claude_md(run_id, identity, now_str);
        }
    }
    injected_claude_md
}

/// Wrapper-active indicator - always printed so the user can confirm routing is live.
pub(super) fn print_wrapper_banner(
    agent: &str,
    session_ids: &Option<(String, String)>,
    native_id: &Option<String>,
    name: &Option<String>,
    work: &Option<String>,
) {
    let session_label = session_ids
        .as_ref()
        .map(|(_, run_id)| format!("  session: {}", &run_id[..8]))
        .unwrap_or_default();
    println!(
        "\x1b[32m⟦ RAIOS WRAPPER ✓ ⟧\x1b[0m  agent: {}{}",
        canonical_agent_identity(agent).unwrap_or(agent),
        session_label
    );

    if let Some(native_id) = native_id {
        println!("  saved conversation: {native_id}");
    }
    if let Some(name) = name {
        println!("  name: {}", name.trim());
    }
    if let Some(work) = work {
        println!("  work: {}", work.trim());
    }
}

/// A launch that failed before the agent started: close the session row as failed.
pub(super) fn close_unstarted_session(session_ids: &Option<(String, String)>) {
    if let Some((task_id, run_id)) = session_ids {
        if let Ok(conn) = raios_core::db::open_db() {
            if let Err(error) =
                raios_core::db::cp_session_end_with_summary(&conn, task_id, run_id, false, None)
            {
                eprintln!("Warning: failed to close unstarted wrapper session: {error}");
            }
        }
    }
}

/// Lets the liveness tracker know which process is this session's child.
pub(super) fn attach_wrapper_child(
    child: &Child,
    session_ids: &Option<(String, String)>,
    wrapper_owner: &Option<WrapperProcessIdentity>,
) {
    if let (Some((_, run_id)), Some(owner)) = (session_ids, wrapper_owner) {
        let child_identity = crate::wrapper_liveness::child_identity(child.id());
        if let Ok(conn) = raios_core::db::open_db() {
            if let Err(error) = raios_core::db::cp_attach_wrapper_child(
                &conn,
                run_id,
                owner,
                child_identity.as_ref(),
            ) {
                eprintln!("Warning: failed to track wrapper child process: {error}");
            }
        }
    }
}

/// Once the agent has started: tells the user the handover was delivered and marks it consumed.
pub(super) fn report_handoff_delivery(pending_handoff: &Option<PendingHandoff>) {
    if let Some((conn, identity, ctx, _)) = pending_handoff {
        println!(
            "📨 Handover delivered from {} ({}).",
            ctx.from_agent, ctx.status
        );
        if let Err(e) = raios_core::db::cp_consume_handoff(conn, ctx, identity) {
            eprintln!("Warning: failed to mark handoff as consumed: {e}");
        }
    }
}

/// Periodic background memory sync (silent - the TUI may be live). Returns the stop flag and the
/// thread to join once the agent has exited.
pub(super) fn spawn_memory_sync_thread(
    agent: &str,
    project_dir: &Option<String>,
    session_start_time: SystemTime,
) -> (Arc<AtomicBool>, thread::JoinHandle<()>) {
    let stop_sync = Arc::new(AtomicBool::new(false));
    let sync_thread = {
        let stop = Arc::clone(&stop_sync);
        let agent_name = agent.to_string();
        let dir = project_dir.clone();
        let started = session_start_time;
        thread::spawn(move || {
            loop {
                // Check the stop flag once per second instead of blocking the
                // wrapper shutdown for the full memory-sync interval.
                let mut stopped = false;
                for _ in 0..MEMORY_SYNC_INTERVAL_SECS {
                    if stop.load(Ordering::Relaxed) {
                        stopped = true;
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                if stopped || stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(ref d) = dir {
                    crate::session_memory::auto_sync_agent_memory(&agent_name, d, started, false);
                }
            }
        })
    };
    (stop_sync, sync_thread)
}

/// Phase 5: waits for the agent, with the "death timer" when a timeout is given.
pub(super) fn wait_for_agent(
    child: &mut Child,
    agent: &str,
    timeout_secs: Option<u64>,
) -> Result<(), String> {
    if let Some(timeout) = timeout_secs {
        println!("⏱️ Death timer active: {} seconds.", timeout);
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("✅ Agent ({}) exited: {}", agent, status);
                    if status.success() {
                        break Ok(());
                    } else {
                        break Err(format!("Agent exited with {}", status));
                    }
                }
                Ok(None) => {
                    if start.elapsed().as_secs() > timeout {
                        println!("💀 TIMEOUT! Killing agent ({}) for budget safety...", agent);
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(format!("Agent {} timed out.", agent));
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                Err(e) => break Err(format!("Process error: {}", e)),
            }
        }
    } else {
        match child.wait() {
            Ok(status) => {
                println!("✅ Agent ({}) exited: {}", agent, status);
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("Agent exited with {}", status))
                }
            }
            Err(e) => Err(format!("Wait error: {}", e)),
        }
    }
}

/// Phase 6: builds the post-session review, closes the session row, records the trace and
/// prints the summary.
pub(super) fn finish_session(
    agent: &str,
    session_ids: &Option<(String, String)>,
    project_dir: &Option<String>,
    session_started_system: SystemTime,
    review_window_start: &str,
    run_started: Instant,
    success: bool,
) {
    let review_window_end = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let review = match (session_ids.as_ref(), project_dir.as_deref()) {
        (Some(_), Some(dir)) => raios_core::db::open_db().ok().map(|conn| {
            crate::session_review::build_review(
                &conn,
                agent,
                Path::new(dir),
                session_started_system,
                review_window_start,
                &review_window_end,
            )
        }),
        _ => None,
    };
    let review_summary = review.as_ref().map(|r| r.to_json());
    if let Some((task_id, run_id)) = &session_ids {
        match raios_core::db::open_db() {
            Ok(conn) => {
                if let Err(e) = raios_core::db::cp_session_end_with_summary(
                    &conn,
                    task_id,
                    run_id,
                    success,
                    review_summary.as_deref(),
                ) {
                    eprintln!("Warning: failed to close wrapper session {run_id}: {e}");
                }
                if let (Some(review), Some(dir)) = (review.as_ref(), project_dir.as_deref()) {
                    if let Err(e) = crate::trace_recall::record_post_run_review_trace(
                        &conn,
                        agent,
                        dir,
                        success,
                        review,
                        Some(run_id),
                    ) {
                        eprintln!("Warning: failed to record trace memory: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to open database to close wrapper session {run_id}: {e}")
            }
        }
    }
    let session_short = session_ids
        .as_ref()
        .map(|(_, run_id)| format!("  \x1b[90mrun: {}\x1b[0m", &run_id[..8]))
        .unwrap_or_default();
    let identity = canonical_agent_identity(agent).unwrap_or(agent);
    if success {
        println!(
            "\x1b[32m✓ Session ended\x1b[0m{}\n  \x1b[90mDuration:\x1b[0m {}s",
            session_short,
            run_started.elapsed().as_secs(),
        );
    } else {
        println!(
            "\x1b[31m✗ Session ended (non-zero)\x1b[0m{}\n  \x1b[90mDuration:\x1b[0m {}s",
            session_short,
            run_started.elapsed().as_secs(),
        );
    }
    if let Some(review) = &review {
        if let Some(changed) = &review.changed {
            println!("  \x1b[90mChanged:\x1b[0m {}", changed.replace('\n', " | "));
        }
        println!(
            "  \x1b[90mTests in session:\x1b[0m {}",
            if review.tests_run_during_session {
                "yes"
            } else {
                "no"
            }
        );
        if !review.risks.is_empty() {
            println!("  \x1b[90mRisks:\x1b[0m {}", review.risks.join(" | "));
        }
        if !review.learned.is_empty() {
            println!("  \x1b[90mLearned:\x1b[0m {}", review.learned.join(" | "));
        }
    }
    println!(
        "  \x1b[90mHandoff:\x1b[0m raios handoff --to {}-kaira --status {} --msg \"...\"",
        if success { agent } else { identity },
        if success { "success" } else { "failed" }
    );
    let _ = identity;
}

/// Phase 7: a clean session counts towards the instincts.
pub(super) fn learn_from_session(instinct: &mut InstinctEngine, success: bool) {
    if success {
        instinct.data.session_count += 1;
        let _ = instinct.save();
    }
}

/// Phase 8: raios-native memory sync for every agent (no LLM); the interactive memory.md prompt
/// is claude-only. Then keeps memory.md from growing without bound.
pub(super) fn sync_memory_after_session(
    agent: &str,
    project_dir: &Option<String>,
    session_start_time: SystemTime,
    session_ids: &Option<(String, String)>,
    extra_args: &[String],
    success: bool,
) {
    if success {
        if let Some(dir) = project_dir {
            crate::session_memory::auto_sync_agent_memory(agent, dir, session_start_time, true);
            if let (Some((_, run_id)), Some(input)) = (
                session_ids.as_ref(),
                wrapper_launch_input(agent, extra_args),
            ) {
                crate::session_memory::sync_wrapper_launch_input(agent, dir, run_id, input, true);
            }
            if agent.to_lowercase() == "claude" {
                crate::session_memory::post_session_memory_prompt(dir, session_start_time);
            }
            archive_oversized_memory(Path::new(dir));
        }
    }
}
