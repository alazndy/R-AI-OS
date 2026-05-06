use std::time::Duration;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::process::Command;
use tokio::time::timeout;
use anyhow::{Result, Context};
use uuid::Uuid;

use crate::daemon::state::DaemonState;

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
    pub async fn spawn_agent(&self, agent_name: &str, project_path: &str, timeout_secs: u64) -> Result<String> {
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

        println!("[Proxy] Spawning agent '{}' (ID: {}) with {}s death timer", agent_name, process_id, timeout_secs);

        let agent_name_cloned = agent_name.to_string();
        let path_cloned = project_path.to_string();
        let state_cloned = self.state.clone();

        // Spawn a background task to handle the process execution
        tokio::spawn(async move {
            use tokio::io::{BufReader, AsyncBufReadExt};
            use std::process::Stdio;

            let mut cmd = Command::new("powershell");
            cmd.args(&["-Command", &agent_name_cloned]); // Use powershell as a bridge for commands
            cmd.current_dir(&path_cloned);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            
            // Wait for the child process to complete, with a timeout
            let result = match cmd.spawn() {
                Ok(mut child) => {
                    let stdout = child.stdout.take().unwrap();
                    let stderr = child.stderr.take().unwrap();
                    let state_for_logs = state_cloned.clone();
                    
                    // Stream output to logs
                    tokio::spawn(async move {
                        let mut reader = BufReader::new(stdout).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            let mut s = state_for_logs.write().await;
                            if let Some(agent) = s.active_agents.iter_mut().find(|a| a.id == process_id) {
                                agent.logs.push(format!("[stdout] {}", line));
                            }
                        }
                    });

                    let state_for_err = state_cloned.clone();
                    tokio::spawn(async move {
                        let mut reader = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            let mut s = state_for_err.write().await;
                            if let Some(agent) = s.active_agents.iter_mut().find(|a| a.id == process_id) {
                                agent.logs.push(format!("[stderr] {}", line));
                            }
                        }
                    });

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
            if let Some(agent) = state_lock.active_agents.iter_mut().find(|a| a.id == process_id) {
                agent.status = result.to_string();
            }
            
            println!("[Proxy] Agent '{}' (ID: {}) finished. Status: {}", agent_name_cloned, process_id, result);
        });

        Ok(process_id.to_string())
    }
}
