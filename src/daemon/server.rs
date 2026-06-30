use super::state::DaemonState;
use crate::config::Config;
use crate::factory::{Factory, Job};
use crate::proxy_store::{CapabilityProxy, CapabilityStore};
use crate::session::SessionStore;
use notify::{RecursiveMode, Watcher};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};

fn validate_file_change_target(
    target: &std::path::Path,
    workspace: &std::path::Path,
    blocked_paths: &[String],
) -> Result<std::path::PathBuf, String> {
    crate::security::sandbox::SandboxGuard::new(workspace.to_path_buf())
        .with_blocked_paths(blocked_paths.to_vec())
        .check(target)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::validate_file_change_target;
    use tempfile::TempDir;

    #[test]
    fn validate_file_change_target_allows_workspace_files() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();

        let validated = validate_file_change_target(&file, tmp.path(), &[]).unwrap();
        assert_eq!(validated, file.canonicalize().unwrap());
    }

    #[test]
    fn validate_file_change_target_blocks_outside_workspace() {
        let tmp = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let file = outside_dir.path().join("outside.rs");
        std::fs::write(&file, "").unwrap();

        let err = validate_file_change_target(&file, tmp.path(), &[]).unwrap_err();
        assert!(err.contains("outside the allowed workspace"));
    }

    #[test]
    fn validate_file_change_target_blocks_explicit_blocked_paths() {
        let tmp = TempDir::new().unwrap();
        let blocked_dir = tmp.path().join("secrets");
        std::fs::create_dir_all(&blocked_dir).unwrap();
        let file = blocked_dir.join("token.txt");
        std::fs::write(&file, "").unwrap();

        let err = validate_file_change_target(
            &file,
            tmp.path(),
            &[blocked_dir.to_string_lossy().into_owned()],
        )
        .unwrap_err();
        assert!(err.contains("matches a blocked path"));
    }
}

fn policy_blocked_paths() -> Vec<String> {
    crate::security::PolicyConfig::try_load_default()
        .map(|p| p.filesystem.blocked_paths)
        .unwrap_or_default()
}

fn approval_workflow_ids(
    approval: &crate::daemon::state::FileChangeApproval,
) -> Option<crate::control_plane::FileChangeWorkflowIds> {
    Some(crate::control_plane::FileChangeWorkflowIds {
        task_id: approval.task_id.clone()?,
        agent_run_id: approval.agent_run_id.clone()?,
        artifact_id: approval.artifact_id.clone()?,
        approval_id: approval.id.clone(), // id IS the canonical approval id
        project_id: None,
    })
}

pub struct Server {
    state: Arc<RwLock<DaemonState>>,
    execution_proxy: super::proxy::ExecutionProxy,
    sessions: Arc<SessionStore>,
    proxy: Arc<CapabilityProxy>,
}

impl Server {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        let execution_proxy = super::proxy::ExecutionProxy::new(state.clone());
        let sessions = Arc::new(SessionStore::new(SessionStore::default_path()));
        let proxy = Arc::new(CapabilityProxy::new(CapabilityStore::new()));
        Self {
            state,
            execution_proxy,
            sessions,
            proxy,
        }
    }

    /// Run with an externally-provided broadcast channel (used by the Kernel
    /// so all protocols share the same event bus).
    pub async fn run_with_tx(&self, tx: broadcast::Sender<String>) -> anyhow::Result<()> {
        self.run_inner(tx).await
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let (tx, _) = broadcast::channel::<String>(256);
        self.run_inner(tx).await
    }

    async fn run_inner(&self, tx: broadcast::Sender<String>) -> anyhow::Result<()> {
        // Telemetry channel: lossy, low-priority (FileChanged, port events).
        // Capacity 64 — lagged receivers are silently dropped rather than stalling control flow.
        let (telem_tx, _) = broadcast::channel::<String>(64);

        // Load any pending file-change approvals that survived a restart
        self.state.write().await.refresh_pending_from_db();

        // 1. Generate and save IPC token for security using SessionTokenManager
        let token_mgr = crate::security::SessionTokenManager::new();
        let token = token_mgr.generate_and_save()?;
        let config_dir = Config::config_file()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let token_path = config_dir.join(".session_token");
        println!(
            "[Daemon] Security: Secure Session Token generated and saved to {:?}",
            token_path
        );

        // Also write legacy token for backwards compatibility with any existing tools
        let _ = std::fs::write(config_dir.join(".ipc_token"), &token);

        let bind_ip = crate::server::http::resolve_bind_addr(42069).ip();
        let daemon_addr = format!("{bind_ip}:42069");
        println!("Server is listening on {daemon_addr}...");
        let listener = TcpListener::bind(&daemon_addr).await?;

        let factory = Arc::new(Factory::new(tx.clone()));

        // ── Log ring-buffer writer ────────────────────────────────────────────
        // Subscribes to the broadcast channel and persists NewLog + job events
        // to cp_logs so late-connecting TUI clients can replay history.
        {
            let mut log_rx = tx.subscribe();
            tokio::spawn(async move {
                loop {
                    match log_rx.recv().await {
                        Ok(msg) => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
                                let event = v["event"].as_str().unwrap_or("");
                                let entry: Option<(&str, String)> = match event {
                                    "NewLog" => {
                                        let sender = v["log"]["sender"].as_str().unwrap_or("daemon");
                                        let content = v["log"]["content"].as_str().unwrap_or("").to_string();
                                        if content.is_empty() { None } else { Some((sender, content)) }
                                    }
                                    "JobSubmitted" => {
                                        let job_id = v["job_id"].as_str().unwrap_or("?");
                                        let short = &job_id[..8.min(job_id.len())];
                                        Some(("RUN", format!("⏳ Job queued [{}]", short)))
                                    }
                                    _ => None,
                                };
                                if let Some((sender, content)) = entry {
                                    if let Ok(conn) = crate::db::open_db() {
                                        let _ = crate::db::cp_log_append(&conn, sender, &content);
                                    }
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }

        let config =
            Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));
        let daemon_cfg = config.daemon.clone();

        let health_state = self.state.clone();
        let health_tx = tx.clone();
        if daemon_cfg.enable_health_worker {
            let health_interval = std::time::Duration::from_secs(daemon_cfg.health_interval_secs);
            tokio::spawn(async move {
                super::health::start_health_worker(health_state, health_tx, health_interval).await;
            });
        }

        let git_tx = tx.clone();
        let git_state = self.state.clone();
        let git_interval = std::time::Duration::from_secs(daemon_cfg.git_interval_secs);
        tokio::spawn(async move {
            super::git::start_git_worker(git_state, git_tx, git_interval).await;
        });

        let cortex_tx_rx = tx.subscribe();
        let cortex_state = self.state.clone();
        let eager_cortex_indexing = daemon_cfg.startup_cortex_indexing;
        tokio::spawn(async move {
            super::cortex::start_cortex_worker(cortex_state, eager_cortex_indexing, cortex_tx_rx)
                .await;
        });

        let validation_tx_rx = tx.subscribe();
        let validation_tx = tx.clone();
        let validation_state = self.state.clone();
        tokio::spawn(async move {
            super::validation::start_validation_worker(
                validation_state,
                validation_tx_rx,
                validation_tx,
            )
            .await;
        });

        let sentinel_tx = tx.clone();
        let sentinel_state = self.state.clone();
        if daemon_cfg.enable_sentinel_worker {
            let sentinel_interval =
                std::time::Duration::from_secs(daemon_cfg.sentinel_interval_secs);
            tokio::spawn(async move {
                super::sentinel::start_sentinel_worker(
                    sentinel_state,
                    sentinel_tx,
                    sentinel_interval,
                )
                .await;
            });
        }

        let lifecycle_tx = tx.clone();
        let lifecycle_state = self.state.clone();
        let lifecycle_interval =
            std::time::Duration::from_secs(daemon_cfg.lifecycle_interval_secs);
        let standby_days = daemon_cfg.lifecycle_standby_days;
        let archive_days = daemon_cfg.lifecycle_archive_days;
        tokio::spawn(async move {
            super::lifecycle::start_lifecycle_worker(
                lifecycle_state,
                lifecycle_tx,
                lifecycle_interval,
                standby_days,
                archive_days,
            )
            .await;
        });

        let scheduler_tx = tx.clone();
        let scheduler_state = self.state.clone();
        if daemon_cfg.enable_scheduler_worker {
            let sched_interval =
                std::time::Duration::from_secs(daemon_cfg.scheduler_interval_secs);
            tokio::spawn(async move {
                super::scheduler::start_scheduler_worker(
                    scheduler_state,
                    scheduler_tx,
                    sched_interval,
                )
                .await;
            });
        }

        let evolution_rx = tx.subscribe();
        tokio::spawn(async move {
            crate::evolution::start_evolution_worker(evolution_rx).await;
        });

        let watcher_tx = telem_tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let msg = format!(
                        "{{\"event\":\"FileChanged\",\"path\":\"{}\"}}",
                        path.display().to_string().replace("\\", "\\\\")
                    );
                    let _ = watcher_tx.send(msg);
                }
            }
        })
        .ok();

        if let Some(ref mut w) = watcher {
            w.watch(&config.dev_ops_path, RecursiveMode::Recursive).ok();
        }

        // Port Monitor Task
        if daemon_cfg.enable_port_monitor {
            let port_tx = telem_tx.clone();
            let port_monitor_interval =
                std::time::Duration::from_secs(daemon_cfg.port_monitor_interval_secs);
            let port_probe_timeout =
                std::time::Duration::from_millis(daemon_cfg.port_probe_timeout_ms);
            tokio::spawn(async move {
                let common_ports = [3000, 5173, 8080, 4200];
                loop {
                    let mut active = Vec::new();
                    for &port in &common_ports {
                        let addr = format!("127.0.0.1:{}", port);
                        if tokio::time::timeout(
                            port_probe_timeout,
                            tokio::net::TcpStream::connect(&addr),
                        )
                        .await
                        .is_ok()
                        {
                            active.push(port);
                        }
                    }
                    let msg = format!(
                        "{{\"event\":\"ActivePorts\",\"ports\":{}}}",
                        serde_json::to_string(&active).unwrap()
                    );
                    let _ = port_tx.send(msg);
                    tokio::time::sleep(port_monitor_interval).await;
                }
            });
        }

        loop {
            let (mut socket, addr) = listener.accept().await?;
            println!("Client connected: {}", addr);

            let mut rx = tx.subscribe();
            let mut telem_rx = telem_tx.subscribe();
            let state_for_client = self.state.clone();
            let proxy_for_client = self.execution_proxy.clone()
                .with_event_tx(tx.clone());
            let _tx_sender = tx.clone();
            let server_token = token.clone();
            let sessions_for_client = self.sessions.clone();
            let factory_for_client = factory.clone();
            let proxy_for_client_cap = self.proxy.clone();
            let graph_store_for_client = Arc::new(crate::task_graph::GraphStore::new(
                crate::task_graph::GraphStore::default_path(),
            ));
            let swarm_store_for_client = Arc::new(crate::swarm::store::SwarmStore::new(
                crate::swarm::store::SwarmStore::default_path(),
            ));
            let evolution_store_for_client = Arc::new(crate::evolution::CandidateStore::new(
                crate::evolution::CandidateStore::default_path(),
            ));

            let umai = crate::security::Umai::from_default_policy();

            tokio::spawn(super::handlers::handle_client_connection(
                super::handlers::ClientHandle {
                    socket,
                    addr,
                    rx,
                    telem_rx,
                    tx: tx.clone(),
                    state: state_for_client,
                    execution_proxy: proxy_for_client,
                    server_token,
                    sessions: sessions_for_client,
                    factory: factory_for_client,
                    proxy: proxy_for_client_cap,
                    graph_store: graph_store_for_client,
                    swarm_store: swarm_store_for_client,
                    evolution_store: evolution_store_for_client,
                    umai,
                }
            ));
        }
    }
}
// ─── END OF server.rs — all client handling is in handlers.rs ─────────────────
