use crate::instinct::InstinctEngine;
use raios_core::shield::AgentShield;
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, LazyLock, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const MEMORY_SYNC_INTERVAL_SECS: u64 = 90;

const BUDGET_LIMIT_KB: u64 = 300;

const MAX_WRAPPER_LAUNCH_INPUT_CHARS: usize = 600;
mod claude_md;
mod cleanup;
mod detached;
mod phases;
use claude_md::*;
use cleanup::*;
use detached::*;
use phases::*;

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
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let payload = {
                        let mut payload = String::new();
                        let _ = BufReader::new(&mut stream).read_line(&mut payload);
                        payload
                    };
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

/// Pass handoff context through native CLI arguments without reading stdin.
fn handoff_agent_command(
    agent: &str,
    handover_block: Option<&str>,
    extra_args: &[String],
) -> Result<Command, String> {
    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("OPENAI_API_KEY");
            if let Some(block) = handover_block {
                c.arg("--append-system-prompt").arg(block);
                if extra_args.is_empty() {
                    c.arg(block);
                }
            }
            c
        }
        "opencode" => {
            let mut c = Command::new("opencode");
            if let Some(block) = handover_block {
                c.arg("--prompt").arg(block);
            }
            c
        }
        "cursor" => Command::new("cursor"),
        "antigravity" | "agy" => {
            let mut c = Command::new("agy");
            if let Some(block) = handover_block {
                c.arg("--prompt-interactive").arg(block);
            }
            c
        }
        "codex" => {
            let mut c = codex_command();
            if let Some(block) = handover_block {
                c.arg(block);
            }
            c
        }
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    cmd.args(extra_args);
    Ok(cmd)
}

/// Never select another project's handoff when this directory is unregistered.
fn pending_handoff_for_project(
    conn: &rusqlite::Connection,
    project_path: &str,
    identity: &str,
) -> rusqlite::Result<Option<raios_core::control_plane::HandoffContext>> {
    raios_core::db::cp_take_pending_handoff_for_path(conn, project_path, identity)
}

/// Explicit wrapper decision; Auto asks only when attached to an interactive TTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffAction {
    Auto,
    Accept,
    Reject,
    Complete,
    Defer,
}

fn read_handoff_action(
    input: &mut impl io::BufRead,
    output: &mut impl Write,
) -> io::Result<HandoffAction> {
    loop {
        write!(
            output,
            "Handoff: [a] Accept / [r] Reject / [c] Already completed / [l] Later (default): "
        )?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(HandoffAction::Defer);
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "a" | "accept" => return Ok(HandoffAction::Accept),
            "r" | "reject" => return Ok(HandoffAction::Reject),
            "c" | "complete" => return Ok(HandoffAction::Complete),
            "" | "l" | "later" | "defer" => return Ok(HandoffAction::Defer),
            _ => writeln!(output, "Choose a, r, c or l.")?,
        }
    }
}

pub fn run_agent(
    agent: &str,
    project_dir: Option<String>,
    timeout_secs: Option<u64>,
    extra_args: Vec<String>,
) -> Result<(), String> {
    run_agent_with_handoff(
        agent,
        project_dir,
        timeout_secs,
        extra_args,
        HandoffAction::Auto,
    )
}

pub fn run_agent_with_handoff(
    agent: &str,
    project_dir: Option<String>,
    timeout_secs: Option<u64>,
    extra_args: Vec<String>,
    handoff_action: HandoffAction,
) -> Result<(), String> {
    run_agent_with_session_labels(
        agent,
        project_dir,
        timeout_secs,
        extra_args,
        handoff_action,
        None,
        None,
    )
}

pub fn run_agent_with_session_labels(
    agent: &str,
    project_dir: Option<String>,
    timeout_secs: Option<u64>,
    extra_args: Vec<String>,
    handoff_action: HandoffAction,
    name: Option<String>,
    work: Option<String>,
) -> Result<(), String> {
    raios_core::db::validate_session_labels(name.as_deref(), work.as_deref())
        .map_err(|e| e.to_string())?;
    let shield = AgentShield::init();
    let mut instinct = InstinctEngine::init();
    let run_started = Instant::now();
    let session_started_system = SystemTime::now();
    let review_window_start = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // 1. Pre-flight Security Check
    preflight_gate(project_dir.as_deref(), &shield)?;

    // 2. Token Budgeting (Sigmap Integration)
    let mut budget_active = false;
    if let Some(ref dir) = project_dir {
        budget_active = prepare_compact_context(Path::new(dir));
    }

    let project_dir = resolve_project_dir(project_dir);

    // 3. Look up any pending control-plane handoff addressed to this agent identity
    // (see AGENT_CONSTITUTION.md Section 10 — no STATE.json involved). Resolved here,
    // before the Command is built, so each agent's native prompt-injection flag can be
    // used instead of an env var no CLI actually reads.
    let mut pending_handoff = lookup_pending_handoff(agent, &extra_args, project_dir.as_deref());
    let mut handover_block = pending_handoff
        .as_ref()
        .map(|(_, _, _, block)| block.clone());

    // 4. Build the Command, wiring the handover into each agent's native prompt flag.
    // Interactive callers decide before any prompt reaches the child.
    if let Some(block) = &handover_block {
        print_handover_banner(block);
    }

    resolve_handoff_decision(&mut pending_handoff, &mut handover_block, handoff_action)?;

    let (mut cmd, native_id) = build_agent_command(
        agent,
        &extra_args,
        &handover_block,
        &instinct,
        budget_active,
        &project_dir,
    )?;

    // Open a session row in the DB so wrapper-routed sessions are always traceable.
    let session_start_time = SystemTime::now();
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let wrapper_owner = crate::wrapper_liveness::current_wrapper_identity();
    let session_ids = start_wrapper_session(agent, &project_dir, &name, &work, &wrapper_owner)?;
    bind_native_resume(&session_ids, &native_id)?;

    // For claude: inject session block into ~/.claude/CLAUDE.md so it's visible
    // inside Claude Code's own UI (system context, /status). Stripped on exit.
    let injected_claude_md = inject_claude_session_block(agent, &session_ids, &now_str);

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
    // From here on the CLAUDE.md block and the note socket are released by `cleanup`, whichever
    // way this function is left; SIGTERM/SIGHUP become a flag `wait_for_agent` polls.
    let mut cleanup = RunCleanup::new(
        injected_claude_md,
        #[cfg(unix)]
        wrapper_note_ipc,
    );
    let _signals = SignalGuard::install();
    print_wrapper_banner(agent, &session_ids, &native_id, &name, &work);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            close_unstarted_session(&session_ids);
            return Err(format!("Failed to spawn agent: {e}"));
        }
    };
    attach_wrapper_child(&child, &session_ids, &wrapper_owner);
    report_handoff_delivery(&pending_handoff);

    let (stop_sync, sync_thread) =
        spawn_memory_sync_thread(agent, &project_dir, session_start_time);

    // 5. Execution & Timeout Loop
    let result = wait_for_agent(&mut child, agent, timeout_secs);

    // Stop periodic sync thread before doing the final verbose sync below.
    stop_sync.store(true, Ordering::Relaxed);
    let _ = sync_thread.join();
    // 6. Release the note socket and the CLAUDE.md block, then close the session row and print
    // the post-session summary.
    cleanup.release();
    let success = result.is_ok();
    finish_session(
        agent,
        &session_ids,
        &project_dir,
        session_started_system,
        &review_window_start,
        run_started,
        success,
    );

    // 7. Post-session Instinct Learning
    learn_from_session(&mut instinct, success);

    // 8. Auto-sync memory for all agents (raios-native, no LLM).
    sync_memory_after_session(
        agent,
        &project_dir,
        session_start_time,
        &session_ids,
        &extra_args,
        success,
    );

    result
}

/// Keeps `memory.md` from growing without bound: once it passes the trigger
/// size, the oldest log entries move to `memory.archive.md`. Nothing is
/// deleted; failures only warn.
fn archive_oversized_memory(dir: &Path) {
    use crate::memory_archive::{archive_if_oversized, ArchivePolicy};
    match archive_if_oversized(dir, &ArchivePolicy::default()) {
        Ok(Some(report)) if report.applied => println!(
            "  \x1b[90mmemory.md: {} older entries moved to memory.archive.md ({} KB → {} KB)\x1b[0m",
            report.moved(),
            report.bytes_before / 1024,
            report.bytes_after / 1024
        ),
        Ok(Some(report)) => {
            if let Some(reason) = report.skipped_reason {
                println!("  \x1b[90mmemory.md archiving skipped: {reason}\x1b[0m");
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("  memory.md archiving failed: {e:#}"),
    }
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
    let child_pid = child.id();
    submit_detached_child(child)?;

    Ok(child_pid)
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
    let child_pid = child.id();
    submit_detached_child(child)?;
    Ok(child_pid)
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

/// Decides whether the project is too large to hand to the agent raw and, if
/// so, makes sure a signature map (`SIGMAP.md`) is available.
///
/// Returns true only when compact context is genuinely available — either
/// `SIGMAP.md` is already newer than every source file, or `sigmap` just
/// (re)generated it. A missing or failing `sigmap` is reported and the agent
/// starts without `RAIOS_CONTEXT_MODE=compact`, rather than being told about a
/// map that does not exist.
fn prepare_compact_context(dir: &Path) -> bool {
    prepare_compact_context_with(dir, BUDGET_LIMIT_KB * 1024, "sigmap")
}

fn prepare_compact_context_with(
    dir: &Path,
    budget_bytes: u64,
    sigmap: impl AsRef<std::ffi::OsStr>,
) -> bool {
    if !crate::context_budget::measure(dir, budget_bytes).exceeded {
        return false;
    }
    println!(
        "📉 Project source exceeds the {} KB context budget.",
        budget_bytes / 1024
    );

    if crate::context_budget::sigmap_is_fresh(dir) {
        println!("✓ SIGMAP.md is up to date.");
        return true;
    }

    println!("🔍 Compacting context via Sigmap...");
    match Command::new(sigmap).current_dir(dir).status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            println!("⚠ sigmap exited with {status}; continuing without compact context.");
            false
        }
        Err(e) => {
            println!(
                "⚠ Could not run sigmap ({e}); continuing without compact context. \
                 Install it with `raios bootstrap` or `npm i -g sigmap`."
            );
            false
        }
    }
}

#[cfg(test)]
mod tests;
