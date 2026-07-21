use super::state::DaemonState;
use crate::factory::Factory;
use crate::proxy_store::{CapabilityProxy, CapabilityStore};
use crate::session::SessionStore;
use notify::{RecursiveMode, Watcher};
use raios_core::config::Config;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};

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
        let config_dir = Config::config_file()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let token_mgr =
            raios_core::security::SessionTokenManager::with_path(config_dir.join(".session_token"));
        let token = bootstrap_session_token(&token_mgr, &config_dir)?;

        // NOTE: this daemon no longer writes a `.ipc_token` copy. It used to,
        // for backwards compatibility with older clients, but that copy
        // carried the exact same secret as .session_token without matching
        // its owner-only (0600) permissions — defeating the hardening
        // entirely for any local reader. All in-tree clients (MCP server,
        // TUI, tray, VS Code extension) now read .session_token, with the
        // VS Code/tray clients falling back to a pre-existing .ipc_token
        // file on disk for one release cycle to avoid a hard break.

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
                                        let sender =
                                            v["log"]["sender"].as_str().unwrap_or("daemon");
                                        let content =
                                            v["log"]["content"].as_str().unwrap_or("").to_string();
                                        if content.is_empty() {
                                            None
                                        } else {
                                            Some((sender, content))
                                        }
                                    }
                                    "JobSubmitted" => {
                                        let job_id = v["job_id"].as_str().unwrap_or("?");
                                        let short = &job_id[..8.min(job_id.len())];
                                        Some(("RUN", format!("⏳ Job queued [{}]", short)))
                                    }
                                    _ => None,
                                };
                                if let Some((sender, content)) = entry {
                                    if let Ok(conn) = raios_core::db::open_db() {
                                        let _ =
                                            raios_core::db::cp_log_append(&conn, sender, &content);
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

        let eager_cortex_indexing = daemon_cfg.startup_cortex_indexing;
        let cortex_tx = super::cortex::spawn_cortex_worker(eager_cortex_indexing);
        {
            let mut s = self.state.write().await;
            s.cortex_tx = Some(cortex_tx.clone());
        }

        let mut cortex_tx_rx = tx.subscribe();
        let cortex_tx_for_watcher = cortex_tx.clone();
        tokio::spawn(async move {
            while let Ok(msg) = cortex_tx_rx.recv().await {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
                    if v["event"] == "FileChanged" {
                        if let Some(path) = v["path"].as_str() {
                            if super::cortex::is_indexable(path) {
                                let _ = cortex_tx_for_watcher
                                    .send(super::cortex::CortexRequest::IndexFile {
                                        path: std::path::PathBuf::from(path),
                                    })
                                    .await;
                            }
                        }
                    }
                }
            }
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
        let lifecycle_interval = std::time::Duration::from_secs(daemon_cfg.lifecycle_interval_secs);
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
            let sched_interval = std::time::Duration::from_secs(daemon_cfg.scheduler_interval_secs);
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
            let (socket, addr) = listener.accept().await?;
            println!("Client connected: {}", addr);

            let rx = tx.subscribe();
            let telem_rx = telem_tx.subscribe();
            let state_for_client = self.state.clone();
            let proxy_for_client = self.execution_proxy.clone().with_event_tx(tx.clone());
            let _tx_sender = tx.clone();
            let server_token = token.clone();
            let sessions_for_client = self.sessions.clone();
            let factory_for_client = factory.clone();
            let proxy_for_client_cap = self.proxy.clone();
            let graph_store_for_client = Arc::new(raios_core::task_graph::GraphStore::new(
                raios_core::task_graph::GraphStore::default_path(),
            ));
            let swarm_store_for_client = Arc::new(crate::swarm::store::SwarmStore::new(
                crate::swarm::store::SwarmStore::default_path(),
            ));
            let evolution_store_for_client = Arc::new(crate::evolution::CandidateStore::new(
                crate::evolution::CandidateStore::default_path(),
            ));

            let umai = raios_core::security::Umai::from_default_policy();

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
                },
            ));
        }
    }
}

/// Generates the daemon's session token, saves it (owner-only permissions
/// via `SessionTokenManager::generate_and_save`), and logs where it landed.
/// Pulled out of `run_inner` so it's testable without a TcpListener/accept
/// loop — this is the exact code path the `.ipc_token` permission bug lived
/// in (see git history), and it had zero test coverage even after the fix.
fn bootstrap_session_token(
    token_mgr: &raios_core::security::SessionTokenManager,
    config_dir: &std::path::Path,
) -> anyhow::Result<String> {
    let token = token_mgr.generate_and_save()?;
    let token_path = config_dir.join(".session_token");
    println!(
        "[Daemon] Security: Secure Session Token generated and saved to {:?}",
        token_path
    );
    Ok(token)
}

#[cfg(test)]
mod bootstrap_session_token_tests {
    use super::*;

    #[test]
    fn generates_and_saves_a_token_with_owner_only_permissions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let token_path = tmp.path().join(".session_token");
        let mgr = raios_core::security::SessionTokenManager::with_path(token_path.clone());

        let token = bootstrap_session_token(&mgr, tmp.path()).unwrap();

        assert_eq!(token.len(), 64, "expected a 64-char hex token");
        let on_disk = std::fs::read_to_string(&token_path).unwrap();
        assert_eq!(on_disk.trim(), token);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "session token must be owner-only");
        }
    }

    /// Regression guard for the vulnerability fixed this session: a `.ipc_token`
    /// copy used to be written right after this, carrying the same secret
    /// without matching permissions. Bootstrapping the token alone must never
    /// produce that file — if a future edit reintroduces it here, this fails.
    #[test]
    fn never_writes_a_legacy_ipc_token_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr =
            raios_core::security::SessionTokenManager::with_path(tmp.path().join(".session_token"));

        bootstrap_session_token(&mgr, tmp.path()).unwrap();

        assert!(
            !tmp.path().join(".ipc_token").exists(),
            "bootstrap_session_token must not write a .ipc_token copy"
        );
    }
}
// ─── END OF server.rs — all client handling is in handlers.rs ─────────────────
