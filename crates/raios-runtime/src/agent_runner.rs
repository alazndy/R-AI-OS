use crate::instinct::InstinctEngine;
use raios_core::shield::AgentShield;
use std::io::{self, BufRead};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const MEMORY_SYNC_INTERVAL_SECS: u64 = 90;

const BUDGET_LIMIT_KB: u64 = 300;

const MAX_WRAPPER_LAUNCH_INPUT_CHARS: usize = 600;

/// Codex's own `workspace-write` sandbox (its default for agentic shell
/// commands) blocks all outbound sockets, including loopback TCP, unless
/// this is set — live-verified against a real `codex exec` child: identical
/// `Operation not permitted` failures with no flag and with a bare
/// `--sandbox workspace-write`, success only once this exact key was passed.
/// Namespaced under `sandbox_workspace_write`, so it is inert whenever
/// Codex's effective sandbox is `read-only` or `danger-full-access` instead.
/// This does widen network egress for every shell command Codex's own
/// agentic loop runs while wrapped by `raios run codex` — an explicit,
/// deliberate trade-off to make `wrapper-note` reachable out of the box,
/// not an incidental side effect.
const CODEX_NETWORK_ACCESS_CONFIG: &str = "sandbox_workspace_write.network_access=true";

/// The one place a `codex` child is constructed, so the network-access
/// override above can never be added at one call site and forgotten at
/// the other.
fn codex_command() -> Command {
    let mut c = Command::new("codex");
    c.arg("-c").arg(CODEX_NETWORK_ACCESS_CONFIG);
    c
}

/// A short-lived, wrapper-owned transport for sandboxed agent children. The
/// child never opens workspace.db itself; the wrapper validates and persists
/// the note on its behalf. Loopback TCP (not a Unix domain socket) is used
/// deliberately: a real live test against `codex exec --sandbox
/// workspace-write` showed Landlock denies `connect()` on a Unix socket even
/// when its directory is explicitly allow-listed, while loopback TCP is
/// reachable from inside that sandbox once `CODEX_NETWORK_ACCESS_CONFIG` is
/// also passed to the child. The random opaque `run_id` UUID is the actual
/// access-control boundary, not the transport's reachability.
#[cfg(unix)]
struct WrapperNoteIpc {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

#[cfg(unix)]
fn start_wrapper_note_ipc(run_id: String, project_path: String) -> std::io::Result<WrapperNoteIpc> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker = thread::spawn(move || {
        while !worker_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut payload = String::new();
                    let _ = Read::take(&mut stream, 2048).read_to_string(&mut payload);
                    let response = match serde_json::from_str::<serde_json::Value>(&payload) {
                        Ok(value) if value["run_id"].as_str() == Some(run_id.as_str()) => {
                            match value["note"].as_str() {
                                Some(note) => match raios_core::db::open_db().and_then(|conn| {
                                    raios_core::db::cp_record_wrapper_memory_note(&conn, &run_id, &project_path, note)
                                }) {
                                    Ok(event) => {
                                        crate::session_memory::sync_wrapper_session_note(
                                            &event.agent_name, &project_path, &run_id, note,
                                        );
                                        serde_json::json!({"recorded": true, "event_id": event.event_id}).to_string()
                                    }
                                    Err(error) => serde_json::json!({"recorded": false, "error": error.to_string()}).to_string(),
                                },
                                None => serde_json::json!({"recorded": false, "error": "missing note"}).to_string(),
                            }
                        }
                        _ => serde_json::json!({"recorded": false, "error": "invalid wrapper note capability"}).to_string(),
                    };
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    Ok(WrapperNoteIpc { addr, stop, worker })
}

/// Informational invocations must not consume a pending handoff intended for
/// an interactive work session.
fn is_informational_invocation(extra_args: &[String]) -> bool {
    extra_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "--version" | "-V"))
}

/// A wrapper-local memory fallback is deliberately narrow: only a single,
/// explicit launch prompt for agents whose upstream history is globally scoped.
fn wrapper_launch_input<'a>(agent: &str, extra_args: &'a [String]) -> Option<&'a str> {
    if !matches!(agent.to_ascii_lowercase().as_str(), "codex" | "opencode") {
        return None;
    }
    let [input] = extra_args else {
        return None;
    };
    let input = input.trim();
    if input.is_empty()
        || input.starts_with('-')
        || input.chars().count() > MAX_WRAPPER_LAUNCH_INPUT_CHARS
        || raios_core::security::looks_like_secret(input).is_some()
    {
        return None;
    }
    Some(input)
}

pub fn run_agent(
    agent: &str,
    project_dir: Option<String>,
    timeout_secs: Option<u64>,
    extra_args: Vec<String>,
) -> Result<(), String> {
    let shield = AgentShield::init();
    let mut instinct = InstinctEngine::init();
    let run_started = Instant::now();
    let session_started_system = SystemTime::now();
    let review_window_start = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 1. Pre-flight Security Check
    if let Some(ref dir) = project_dir {
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

    // 2. Token Budgeting (Sigmap Integration)
    let mut budget_active = false;
    if let Some(ref dir) = project_dir {
        let size = get_dir_size(Path::new(dir)).unwrap_or(0);
        if size > BUDGET_LIMIT_KB * 1024 {
            println!(
                "📉 Project size ({} KB) exceeds budget ({} KB).",
                size / 1024,
                BUDGET_LIMIT_KB
            );
            println!("🔍 Compacting context via Sigmap...");
            let _ = Command::new("sigmap").current_dir(dir).status();
            budget_active = true;
        }
    }

    // 3. Look up any pending control-plane handoff addressed to this agent identity
    // (see AGENT_CONSTITUTION.md Section 10 — no STATE.json involved). Resolved here,
    // before the Command is built, so each agent's native prompt-injection flag can be
    // used instead of an env var no CLI actually reads.
    let pending_handoff = if is_informational_invocation(&extra_args) {
        None
    } else {
        canonical_agent_identity(agent).and_then(|identity| {
            let conn = raios_core::db::open_db().ok()?;
            let project_id = project_dir
                .as_deref()
                .and_then(|dir| raios_core::db::project_id_for_file_path(&conn, dir));
            let ctx =
                raios_core::db::cp_take_pending_handoff(&conn, project_id, identity).ok()??;
            let mut block = format!(
                "[HANDOVER CONTEXT]\nFrom: {}  Status: {}\n{}",
                ctx.from_agent, ctx.status, ctx.context_summary
            );
            if let Some(diff_stat) = &ctx.diff_stat {
                block.push_str(&format!("\n\n[Changed files since handoff]\n{diff_stat}"));
            }
            if let Some(trace_block) = crate::trace_recall::relevant_trace_block(
                &conn,
                project_dir.as_deref(),
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
    };
    let handover_block = pending_handoff
        .as_ref()
        .map(|(_, _, _, block)| block.clone());

    // 4. Build the Command, wiring the handover into each agent's native prompt flag.
    // NOTE: --append-system-prompt on claude only works with --print (non-interactive).
    // For interactive `raios run claude`, we print the context as a terminal banner
    // and pause for Enter — the user reads the handoff, then the agent starts.
    // This ensures the context is visible in the terminal before the agent TUI takes over.
    if let Some(block) = &handover_block {
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
        println!("\n  \x1b[90mHandoff alındı — devam etmek için \x1b[37m[Enter]\x1b[0m\x1b[90m'a bas...\x1b[0m");
        let stdin = io::stdin();
        let _ = stdin.lock().lines().next();
        println!();
    }

    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("OPENAI_API_KEY");
            // No --append-system-prompt here: that flag implies --print mode and breaks
            // interactive sessions. Context is already printed as a banner above.
            c
        }
        "opencode" => {
            let mut c = Command::new("opencode");
            if let Some(block) = &handover_block {
                c.arg("--prompt").arg(block);
            }
            c
        }
        "cursor" => Command::new("cursor"),
        "antigravity" | "agy" => {
            let mut c = Command::new("agy");
            if let Some(block) = &handover_block {
                c.arg("--prompt-interactive").arg(block);
            }
            c
        }
        "codex" => {
            let mut c = codex_command();
            if let Some(block) = &handover_block {
                c.arg(block);
            }
            c
        }
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    // 5. Forward extra args verbatim to the agent binary
    if !extra_args.is_empty() {
        cmd.args(&extra_args);
    }

    // Inject Instincts & Budget Info
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }
    if budget_active {
        cmd.env("RAIOS_CONTEXT_MODE", "compact");
    }
    // Best-effort env fallback for "cursor" and any future agent without a native flag yet.
    if let Some(block) = &handover_block {
        cmd.env("RAIOS_HANDOVER_CONTEXT", block);
    }

    // Resolve effective project dir: explicit flag > CWD
    let project_dir = project_dir.or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    });

    if let Some(dir) = &project_dir {
        cmd.current_dir(dir);
    }

    // Open a session row in the DB so wrapper-routed sessions are always traceable.
    let session_start_time = SystemTime::now();
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let session_ids = {
        let identity = canonical_agent_identity(agent).unwrap_or(agent);
        let conn_res = raios_core::db::open_db();
        match conn_res {
            Ok(conn) => {
                let project_id = project_dir
                    .as_deref()
                    .and_then(|dir| raios_core::db::project_id_for_file_path(&conn, dir));
                match raios_core::db::cp_session_start(&conn, identity, project_id) {
                    Ok(ids) => Some((ids.0, ids.1)),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    };

    // For claude: inject session block into ~/.claude/CLAUDE.md so it's visible
    // inside Claude Code's own UI (system context, /status). Stripped on exit.
    let injected_claude_md = agent.to_lowercase() == "claude";
    if injected_claude_md {
        if let Some((_, run_id)) = &session_ids {
            let identity = canonical_agent_identity(agent).unwrap_or(agent);
            inject_session_to_claude_md(run_id, identity, &now_str);
        }
    }

    // Wrapper-active indicator — always printed so the user can confirm routing is live.
    let session_label = session_ids
        .as_ref()
        .map(|(_, run_id)| format!("  session: {}", &run_id[..8]))
        .unwrap_or_default();
    #[cfg(unix)]
    let wrapper_note_ipc = match (session_ids.as_ref(), project_dir.as_ref()) {
        (Some((_, run_id)), Some(dir)) => start_wrapper_note_ipc(run_id.clone(), dir.clone()).ok(),
        _ => None,
    };
    if let Some((_, run_id)) = &session_ids {
        // This opaque ID is scoped by cp_record_wrapper_memory_note to the
        // still-running child session and its registered project.
        cmd.env("RAIOS_WRAPPER_RUN_ID", run_id);
        if let Some(dir) = &project_dir {
            cmd.env("RAIOS_WRAPPER_PROJECT_PATH", dir);
        }
        #[cfg(unix)]
        if let Some(ipc) = &wrapper_note_ipc {
            cmd.env("RAIOS_WRAPPER_NOTE_SOCKET", ipc.addr.to_string());
        }
    }
    println!(
        "\x1b[32m⟦ RAIOS WRAPPER ✓ ⟧\x1b[0m  agent: {}{}",
        canonical_agent_identity(agent).unwrap_or(agent),
        session_label
    );

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn agent: {}", e)),
    };
    if let Some((conn, identity, ctx, _)) = &pending_handoff {
        println!(
            "📨 Handover delivered from {} ({}).",
            ctx.from_agent, ctx.status
        );
        if let Err(e) = raios_core::db::cp_consume_handoff(conn, ctx, identity) {
            eprintln!("Warning: failed to mark handoff as consumed: {e}");
        }
    }

    // Periodic background memory sync (silent — TUI may be live).
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

    // 5. Execution & Timeout Loop
    let result = if let Some(timeout) = timeout_secs {
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
    };

    // Stop periodic sync thread before doing the final verbose sync below.
    stop_sync.store(true, Ordering::Relaxed);
    let _ = sync_thread.join();
    #[cfg(unix)]
    if let Some(ipc) = wrapper_note_ipc {
        ipc.stop.store(true, Ordering::Relaxed);
        let _ = ipc.worker.join();
    }

    // 6. Close session row in DB and print post-session summary.
    if injected_claude_md {
        strip_session_from_claude_md();
    }
    let success = result.is_ok();
    let review_window_end = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let review = match (session_ids.as_ref(), project_dir.as_deref()) {
        (Some(_), Some(dir)) => raios_core::db::open_db().ok().map(|conn| {
            crate::session_review::build_review(
                &conn,
                agent,
                Path::new(dir),
                session_started_system,
                &review_window_start,
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

    // 7. Post-session Instinct Learning
    if success {
        instinct.data.session_count += 1;
        let _ = instinct.save();
    }

    // 8. Auto-sync memory for all agents (raios-native, no LLM).
    //    project memory.md interactive prompt is claude-only (uses claude --print).
    if success {
        if let Some(ref dir) = project_dir {
            crate::session_memory::auto_sync_agent_memory(agent, dir, session_start_time, true);
            if let (Some((_, run_id)), Some(input)) = (
                session_ids.as_ref(),
                wrapper_launch_input(agent, &extra_args),
            ) {
                crate::session_memory::sync_wrapper_launch_input(agent, dir, run_id, input, true);
            }
            if agent.to_lowercase() == "claude" {
                crate::session_memory::post_session_memory_prompt(dir, session_start_time);
            }
        }
    }

    result
}

/// Spawn an agent without waiting for it to exit.
/// Returns the child PID on success.
/// Does NOT do handoff lookup — caller provides the full prompt.
pub fn spawn_agent_detached(
    agent: &str,
    task_prompt: &str,
    project_dir: Option<&str>,
) -> Result<u32, String> {
    let shield = AgentShield::init();
    let instinct = InstinctEngine::init();

    // 1. Pre-flight Security Check
    if let Some(dir) = project_dir {
        let warnings = shield.preflight_check(Path::new(dir));
        for warning in warnings {
            println!("{}", warning);
        }
    }

    // 2. Per-agent command build
    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("OPENAI_API_KEY");
            c.arg("--append-system-prompt").arg(task_prompt);
            c
        }
        "opencode" => {
            let mut c = Command::new("opencode");
            c.arg("--prompt").arg(task_prompt);
            c
        }
        "cursor" => Command::new("cursor"),
        "antigravity" | "agy" => {
            let mut c = Command::new("agy");
            c.arg("--prompt-interactive").arg(task_prompt);
            c
        }
        "codex" => {
            let mut c = codex_command();
            c.arg(task_prompt);
            c
        }
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    // 3. Inject Instincts env
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }

    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }

    println!(
        "🚀 Raios Kernel: Spawning detached agent '{}' under Shield protection...",
        agent
    );

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn agent: {}", e)),
    };

    Ok(child.id())
}

/// Inject a RAIOS session block into ~/.claude/CLAUDE.md so the session ID
/// and tracking info appear inside Claude Code's own UI (visible via /status
/// and referenced in the system context). Strips any stale block first.
fn inject_session_to_claude_md(run_id: &str, agent_identity: &str, started_at: &str) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let stripped = strip_session_block(&content);
    let block = format!(
        "\n<!-- raios-session-begin -->\n\
# RAIOS WRAPPER SESSION\n\
- Session ID: `{}`\n\
- Agent: {}\n\
- Started: {}\n\
- Tracking: `raios sessions` | `raios sessions --agent claude`\n\
<!-- raios-session-end -->\n",
        &run_id[..8],
        agent_identity,
        started_at,
    );
    let _ = std::fs::write(&path, format!("{}{}", stripped.trim_end(), block));
}

/// Remove the RAIOS session block from ~/.claude/CLAUDE.md.
fn strip_session_from_claude_md() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let stripped = strip_session_block(&content);
    if stripped != content {
        let _ = std::fs::write(&path, stripped.trim_end().to_string() + "\n");
    }
}

fn strip_session_block(content: &str) -> String {
    const BEGIN: &str = "<!-- raios-session-begin -->";
    const END: &str = "<!-- raios-session-end -->";
    if let (Some(start), Some(end_off)) = (content.find(BEGIN), content.rfind(END)) {
        let before = content[..start].trim_end_matches('\n');
        let after = &content[end_off + END.len()..];
        format!("{}\n{}", before, after.trim_start_matches('\n'))
    } else {
        content.to_string()
    }
}

/// Extension schedules (registered via `raios ext install` — see
/// `crates/raios-surface-cli/src/cli/ext/schedule.rs`) write a
/// machine-parseable `task_description` of the form "raios ext <name>
/// <command>" and set `agent` to the extension's own name (e.g. "atc")
/// rather than a supported AI-agent CLI. Recognize that shape so callers
/// can dispatch through the extension runner instead of treating it as an
/// unrecognized AI agent.
pub fn ext_command_from_task_description(task_description: &str) -> Option<(&str, &str)> {
    let rest = task_description.strip_prefix("raios ext ")?;
    let (ext_name, command) = rest.split_once(' ')?;
    if ext_name.is_empty() || command.is_empty() {
        return None;
    }
    Some((ext_name, command))
}

/// Spawn `raios ext <name> <command>` detached. Used by the cron scheduler
/// for schedules an extension registered for itself (see
/// `crates/raios-surface-cli/src/cli/ext/schedule.rs`) — these run a
/// project's own script through the extension runner, not an interactive
/// AI-agent CLI, so they must bypass the fixed agent whitelist in
/// `spawn_agent_detached`.
pub fn spawn_ext_command_detached(ext_name: &str, command: &str) -> Result<u32, String> {
    let child = Command::new("raios")
        .arg("ext")
        .arg(ext_name)
        .arg(command)
        .spawn()
        .map_err(|e| format!("Failed to spawn 'raios ext {ext_name} {command}': {e}"))?;
    Ok(child.id())
}

/// Maps a spawnable agent name to its Kaira identity for handoff lookups.
/// Agents outside the 4-agent matrix (e.g. "cursor") return `None` — they
/// still spawn normally, just without handoff delivery.
fn canonical_agent_identity(agent: &str) -> Option<&'static str> {
    match agent.to_lowercase().as_str() {
        "claude" => Some("claude_kaira"),
        "codex" => Some("codex_kaira"),
        "opencode" => Some("opencode_kaira"),
        "antigravity" | "agy" => Some("antigravity_kaira"),
        _ => None,
    }
}

fn get_dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total_size = 0;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total_size += get_dir_size(&path)?;
            } else {
                total_size += entry.metadata()?.len();
            }
        }
    }
    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_command_always_enables_workspace_write_network_access() {
        let cmd = codex_command();
        assert_eq!(cmd.get_program(), "codex");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, vec!["-c", CODEX_NETWORK_ACCESS_CONFIG]);
    }

    #[test]
    fn wrapper_local_capture_accepts_only_one_bounded_prompt_for_unscoped_agents() {
        assert_eq!(
            wrapper_launch_input("codex", &["we decided to use SQLite".to_string()]),
            Some("we decided to use SQLite")
        );
        assert_eq!(
            wrapper_launch_input("opencode", &["use pnpm".to_string()]),
            Some("use pnpm")
        );
        assert_eq!(
            wrapper_launch_input("claude", &["use pnpm".to_string()]),
            None
        );
        assert_eq!(
            wrapper_launch_input("codex", &["--version".to_string()]),
            None
        );
        assert_eq!(
            wrapper_launch_input("codex", &["--model".to_string(), "gpt-5".to_string()]),
            None
        );
        assert_eq!(
            wrapper_launch_input("codex", &["x".repeat(MAX_WRAPPER_LAUNCH_INPUT_CHARS + 1)]),
            None
        );
        assert_eq!(
            wrapper_launch_input("codex", &["password = 'superSecretValue123'".to_string()]),
            None
        );
    }

    #[test]
    fn informational_invocations_do_not_consume_handoffs() {
        assert!(is_informational_invocation(&["--help".to_string()]));
        assert!(is_informational_invocation(&["-V".to_string()]));
        assert!(is_informational_invocation(&[
            "exec".to_string(),
            "--version".to_string(),
        ]));
        assert!(!is_informational_invocation(&[
            "implement the feature".to_string()
        ]));
    }

    // ─── canonical_agent_identity ────────────────────────────────────────

    #[test]
    fn maps_known_agents_to_kaira_identities() {
        assert_eq!(canonical_agent_identity("claude"), Some("claude_kaira"));
        assert_eq!(canonical_agent_identity("codex"), Some("codex_kaira"));
        assert_eq!(canonical_agent_identity("opencode"), Some("opencode_kaira"));
        assert_eq!(
            canonical_agent_identity("antigravity"),
            Some("antigravity_kaira")
        );
        assert_eq!(canonical_agent_identity("agy"), Some("antigravity_kaira"));
    }

    #[test]
    fn agent_identity_lookup_is_case_insensitive() {
        assert_eq!(canonical_agent_identity("Claude"), Some("claude_kaira"));
        assert_eq!(canonical_agent_identity("CODEX"), Some("codex_kaira"));
        assert_eq!(canonical_agent_identity("AGY"), Some("antigravity_kaira"));
    }

    #[test]
    fn unknown_agent_returns_none_without_blocking_spawn() {
        // Agents outside the 4-agent matrix (e.g. "cursor") still spawn
        // normally — this must fail open (None), never panic or error.
        assert_eq!(canonical_agent_identity("cursor"), None);
        assert_eq!(canonical_agent_identity(""), None);
        assert_eq!(canonical_agent_identity("gpt-5"), None);
    }

    // ─── ext_command_from_task_description ──────────────────────────────

    #[test]
    fn parses_a_well_formed_ext_task_description() {
        assert_eq!(
            ext_command_from_task_description("raios ext atc process"),
            Some(("atc", "process"))
        );
    }

    #[test]
    fn preserves_multi_word_commands_after_the_extension_name() {
        // splitn(2, ' ') must not truncate a command that itself has spaces.
        assert_eq!(
            ext_command_from_task_description("raios ext atc per project"),
            Some(("atc", "per project"))
        );
    }

    #[test]
    fn plain_ai_agent_task_descriptions_do_not_match() {
        assert_eq!(ext_command_from_task_description("backup database"), None);
        assert_eq!(ext_command_from_task_description(""), None);
    }

    #[test]
    fn rejects_prefix_without_a_trailing_command() {
        assert_eq!(ext_command_from_task_description("raios ext atc"), None);
        assert_eq!(ext_command_from_task_description("raios ext "), None);
    }

    // ─── strip_session_block ─────────────────────────────────────────────

    #[test]
    fn strip_session_block_removes_an_existing_block() {
        let content = "# My Notes\n\nSome content.\n\n\
            <!-- raios-session-begin -->\n\
            # RAIOS WRAPPER SESSION\n- Session ID: `abc12345`\n\
            <!-- raios-session-end -->\n\n\
            More content after.\n";
        let stripped = strip_session_block(content);
        assert!(!stripped.contains("raios-session-begin"));
        assert!(!stripped.contains("Session ID"));
        assert!(stripped.contains("# My Notes"));
        assert!(stripped.contains("More content after."));
    }

    #[test]
    fn strip_session_block_is_a_noop_without_a_block() {
        let content = "# My Notes\n\nJust some plain content, no session block.\n";
        assert_eq!(strip_session_block(content), content);
    }

    #[test]
    fn strip_session_block_replaces_stale_block_on_reinjection() {
        // inject_session_to_claude_md always strips first, then appends —
        // this is what guarantees re-running an agent never accumulates
        // multiple stacked session blocks in CLAUDE.md.
        let content = "# Notes\n\n\
            <!-- raios-session-begin -->\nold session\n<!-- raios-session-end -->\n";
        let stripped = strip_session_block(content);
        let reinjected = format!(
            "{}\n<!-- raios-session-begin -->\nnew session\n<!-- raios-session-end -->\n",
            stripped.trim_end()
        );
        assert_eq!(reinjected.matches("raios-session-begin").count(), 1);
        assert!(reinjected.contains("new session"));
        assert!(!reinjected.contains("old session"));
    }

    #[test]
    fn strip_session_block_handles_multiple_begin_markers_by_taking_outermost_span() {
        // find() picks the first BEGIN, rfind() picks the last END — this
        // documents the actual (not obviously correct) behavior on
        // malformed/doubled markers rather than leaving it unverified.
        let content = "pre\n<!-- raios-session-begin -->\nA\n<!-- raios-session-begin -->\nB\n<!-- raios-session-end -->\npost";
        let stripped = strip_session_block(content);
        assert_eq!(stripped, "pre\npost");
    }

    // ─── get_dir_size ─────────────────────────────────────────────────────

    #[test]
    fn get_dir_size_sums_files_recursively() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"12345").unwrap(); // 5 bytes
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.txt"), b"1234567890").unwrap(); // 10 bytes

        let size = get_dir_size(tmp.path()).unwrap();
        assert_eq!(size, 15);
    }

    #[test]
    fn get_dir_size_of_empty_dir_is_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(get_dir_size(tmp.path()).unwrap(), 0);
    }

    #[test]
    fn get_dir_size_of_a_file_path_is_zero() {
        // Matches the real call site's behavior: `path.is_dir()` gates the
        // whole body, so pointing this at a plain file (not a directory)
        // silently returns 0 rather than that file's own size.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("solo.txt");
        std::fs::write(&file, b"some bytes").unwrap();
        assert_eq!(get_dir_size(&file).unwrap(), 0);
    }

    // ─── wrapper-note IPC — real loopback TCP transport ────────────────────

    #[cfg(unix)]
    mod wrapper_note_ipc {
        use super::*;
        use rusqlite::params;
        use std::net::TcpStream;
        use std::sync::Mutex;

        // `RAIOS_DB_PATH` is process-global; serialize any test in this
        // binary that reads or writes it so parallel `cargo test` threads
        // never race on the same env var.
        static DB_ENV_LOCK: Mutex<()> = Mutex::new(());

        fn send_note(addr: SocketAddr, run_id: &str, note: &str) -> serde_json::Value {
            let mut stream = TcpStream::connect(addr).unwrap();
            let payload = serde_json::json!({"run_id": run_id, "note": note}).to_string();
            stream.write_all(payload.as_bytes()).unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            serde_json::from_str(&response).unwrap()
        }

        fn stop(ipc: WrapperNoteIpc) {
            ipc.stop.store(true, Ordering::Relaxed);
            let _ = ipc.worker.join();
        }

        #[test]
        fn wrapper_note_ipc_two_projects_parallel_no_cross_leak() {
            let _lock = DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let original_db_path = std::env::var("RAIOS_DB_PATH").ok();
            let tmp_db = tempfile::NamedTempFile::new().unwrap();
            std::env::set_var("RAIOS_DB_PATH", tmp_db.path());

            let conn = raios_core::db::open_db().unwrap();
            let project_a = raios_core::db::upsert_project(
                &conn,
                "Project A",
                "core",
                "/tmp/raios-wrapper-note-test-a",
                None,
                "active",
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let project_b = raios_core::db::upsert_project(
                &conn,
                "Project B",
                "core",
                "/tmp/raios-wrapper-note-test-b",
                None,
                "active",
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let (_, run_a) =
                raios_core::db::cp_session_start(&conn, "codex_kaira", Some(project_a)).unwrap();
            let (_, run_b) =
                raios_core::db::cp_session_start(&conn, "codex_kaira", Some(project_b)).unwrap();
            drop(conn);

            let ipc_a = start_wrapper_note_ipc(
                run_a.clone(),
                "/tmp/raios-wrapper-note-test-a".to_string(),
            )
            .unwrap();
            let ipc_b = start_wrapper_note_ipc(
                run_b.clone(),
                "/tmp/raios-wrapper-note-test-b".to_string(),
            )
            .unwrap();

            // Legitimate note into each project's own socket succeeds.
            let resp_a = send_note(ipc_a.addr, &run_a, "note for A");
            assert_eq!(resp_a["recorded"], true, "resp_a = {resp_a}");
            let resp_b = send_note(ipc_b.addr, &run_b, "note for B");
            assert_eq!(resp_b["recorded"], true, "resp_b = {resp_b}");

            // Cross-wiring: each socket only recognizes the run ID it was
            // started with, so presenting the *other* project's run ID must
            // be rejected by the socket itself, never reach the DB as a
            // write into the wrong project.
            let cross_into_a = send_note(ipc_a.addr, &run_b, "cross leak into A");
            assert_eq!(cross_into_a["recorded"], false, "cross_into_a = {cross_into_a}");
            let cross_into_b = send_note(ipc_b.addr, &run_a, "cross leak into B");
            assert_eq!(cross_into_b["recorded"], false, "cross_into_b = {cross_into_b}");

            stop(ipc_a);
            stop(ipc_b);

            let conn = raios_core::db::open_db().unwrap();
            let events_a: Vec<String> = conn
                .prepare("SELECT content FROM cp_wrapper_events WHERE project_id=?1")
                .unwrap()
                .query_map(params![project_a], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(events_a, vec!["note for A".to_string()]);

            let events_b: Vec<String> = conn
                .prepare("SELECT content FROM cp_wrapper_events WHERE project_id=?1")
                .unwrap()
                .query_map(params![project_b], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(events_b, vec!["note for B".to_string()]);
            drop(conn);

            match original_db_path {
                Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
                None => std::env::remove_var("RAIOS_DB_PATH"),
            }
        }
    }
}
