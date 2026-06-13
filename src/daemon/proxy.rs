use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::timeout;
use uuid::Uuid;

use crate::daemon::state::DaemonState;

fn agent_shell_command(agent_name: &str) -> (String, Vec<String>) {
    #[cfg(target_family = "windows")]
    {
        (
            "powershell".to_string(),
            vec!["-Command".to_string(), agent_name.to_string()],
        )
    }

    #[cfg(not(target_family = "windows"))]
    {
        (
            "sh".to_string(),
            vec!["-lc".to_string(), agent_name.to_string()],
        )
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
}

impl ExecutionProxy {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        Self { state }
    }

    /// Spawns an agent in an isolated environment with a Death Timer.
    pub async fn spawn_agent(
        &self,
        agent_name: &str,
        project_path: &str,
        timeout_secs: u64,
    ) -> Result<String> {
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

        println!(
            "[Proxy] Spawning agent '{}' (ID: {}) with {}s death timer",
            agent_name, process_id, timeout_secs
        );

        let agent_name_cloned = agent_name.to_string();
        let path_cloned = project_path.to_string();
        let state_cloned = self.state.clone();

        // Spawn a background task to handle the process execution
        tokio::spawn(async move {
            use std::process::Stdio;
            use tokio::io::{AsyncBufReadExt, BufReader};

            let (program, args) = agent_shell_command(&agent_name_cloned);
            let mut cmd = Command::new(&program);
            cmd.args(&args);
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
    use super::agent_shell_command;

    #[cfg(target_family = "windows")]
    #[test]
    fn agent_shell_command_uses_powershell_on_windows() {
        let (program, args) = agent_shell_command("claude");
        assert_eq!(program, "powershell");
        assert_eq!(args, vec!["-Command", "claude"]);
    }

    #[cfg(not(target_family = "windows"))]
    #[test]
    fn agent_shell_command_uses_sh_on_unix() {
        let (program, args) = agent_shell_command("claude");
        assert_eq!(program, "sh");
        assert_eq!(args, vec!["-lc", "claude"]);
    }
}
