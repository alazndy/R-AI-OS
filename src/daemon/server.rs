use super::state::DaemonState;
use crate::config::Config;
use crate::factory::{Factory, Job};
use crate::proxy_store::{CapabilityProxy, CapabilityStore};
use crate::session::SessionStore;
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
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
        // 1. Generate and save IPC token for security using SessionTokenManager
        let token_mgr = crate::security::SessionTokenManager::new();
        let token = token_mgr.generate_and_save()?;
        let config_dir = Config::config_file().parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let token_path = config_dir.join(".session_token");
        println!(
            "[Daemon] Security: Secure Session Token generated and saved to {:?}",
            token_path
        );

        // Also write legacy token for backwards compatibility with any existing tools
        let _ = std::fs::write(config_dir.join(".ipc_token"), &token);

        println!("Server is listening on 127.0.0.1:42069...");
        let listener = TcpListener::bind("127.0.0.1:42069").await?;

        let factory = Arc::new(Factory::new(tx.clone()));

        // ... (workers spawn logic unchanged)
        let health_state = self.state.clone();
        let health_tx = tx.clone();
        tokio::spawn(async move {
            super::health::start_health_worker(health_state, health_tx).await;
        });

        let git_tx = tx.clone();
        let git_state = self.state.clone();
        tokio::spawn(async move {
            super::git::start_git_worker(git_state, git_tx).await;
        });

        let cortex_tx_rx = tx.subscribe();
        let cortex_state = self.state.clone();
        tokio::spawn(async move {
            super::cortex::start_cortex_worker(cortex_state, cortex_tx_rx).await;
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
        tokio::spawn(async move {
            super::sentinel::start_sentinel_worker(sentinel_state, sentinel_tx).await;
        });

        let evolution_rx = tx.subscribe();
        tokio::spawn(async move {
            crate::evolution::start_evolution_worker(evolution_rx).await;
        });

        // Start file watcher
        let config = Config::load().unwrap_or_else(|| Config {
            dev_ops_path: PathBuf::from(""),
            master_md_path: PathBuf::from(""),
            skills_path: PathBuf::from(""),
            vault_projects_path: PathBuf::from(""),
        });

        let watcher_tx = tx.clone();
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
        let port_tx = tx.clone();
        tokio::spawn(async move {
            let common_ports = [3000, 5173, 8080, 4200];
            loop {
                let mut active = Vec::new();
                for &port in &common_ports {
                    let addr = format!("127.0.0.1:{}", port);
                    if tokio::time::timeout(
                        std::time::Duration::from_millis(100),
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
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });

        loop {
            let (mut socket, addr) = listener.accept().await?;
            println!("Client connected: {}", addr);

            let mut rx = tx.subscribe();
            let state_for_client = self.state.clone();
            let proxy_for_client = self.execution_proxy.clone();
            let _tx_sender = tx.clone();
            let server_token = token.clone();
            let sessions_for_client = self.sessions.clone();
            let factory_for_client = factory.clone();
            let proxy_for_client_cap = self.proxy.clone();
            let graph_store_for_client = Arc::new(
                crate::task_graph::GraphStore::new(crate::task_graph::GraphStore::default_path())
            );
            let swarm_store_for_client = Arc::new(
                crate::swarm::store::SwarmStore::new(crate::swarm::store::SwarmStore::default_path())
            );
            let evolution_store_for_client = Arc::new(
                crate::evolution::CandidateStore::new(crate::evolution::CandidateStore::default_path())
            );

            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let (reader, mut writer) = socket.split();
                let mut reader = BufReader::new(reader);
                let mut line = String::new();

                // 2. Authentication Challenge
                if let Ok(n) = reader.read_line(&mut line).await {
                    if n == 0
                        || !line.trim().starts_with("AUTH ")
                        || line.trim()[5..] != server_token
                    {
                        println!(
                            "[Daemon] Auth failed for client {}. Dropping connection.",
                            addr
                        );
                        let _ = writer
                            .write_all(
                                b"{\"event\":\"Error\",\"message\":\"Authentication failed\"}\n",
                            )
                            .await;
                        return;
                    }
                    println!("[Daemon] Client {} authenticated.", addr);
                } else {
                    return;
                }
                line.clear();

                // Auto-start session after successful auth
                let session_id = sessions_for_client.start("daemon-client", None);
                let session_msg = serde_json::json!({
                    "event": "SessionStarted",
                    "session_id": session_id
                });
                let _ = writer
                    .write_all(format!("{}\n", session_msg).as_bytes())
                    .await;

                loop {
                    tokio::select! {
                        // Read from socket
                        res = reader.read_line(&mut line) => {
                            if res.unwrap_or(0) == 0 { break; }
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                                if v["command"] == "AgentInfo" {
                                    if let Some(agent) = v["agent"].as_str() {
                                        let project = v["project"].as_str();
                                        sessions_for_client.record_event(
                                            &session_id,
                                            "agent_info",
                                            &format!("agent={} project={}", agent, project.unwrap_or("-")),
                                        );
                                        // Update sessions table with actual agent and project names
                                        if let Ok(conn) = rusqlite::Connection::open(
                                            crate::session::SessionStore::default_path()
                                        ) {
                                            if let Err(e) = conn.execute(
                                                "UPDATE sessions SET agent=?1, project=COALESCE(?2, project) WHERE id=?3",
                                                rusqlite::params![agent, project, session_id],
                                            ) {
                                                eprintln!("[Daemon] AgentInfo DB update failed: {e}");
                                            }
                                        }
                                        let _ = writer.write_all(b"{\"event\":\"AgentInfoAck\"}\n").await;
                                    }
                                } else if v["command"] == "Search" {
                                    if let Some(query) = v["query"].as_str() {
                                        let s = state_for_client.read().await;
                                        if let Some(ref idx) = s.index {
                                            let results = idx.search(query);
                                            let response = format!("{{\"event\":\"SearchResults\",\"results\":{}}}\n", serde_json::to_string(&results).unwrap());
                                            let _ = writer.write_all(response.as_bytes()).await;
                                        }
                                    }
                                } else if v["command"] == "VectorSearch" {
                                    if let Some(query) = v["query"].as_str() {
                                        let top_k = v["top_k"].as_u64().unwrap_or(10) as usize;

                                        // 1. Semantic hits
                                        let vector_hits = match crate::cortex::Cortex::init() {
                                            Ok(cortex) => cortex.search(query, top_k).unwrap_or_default(),
                                            Err(_) => vec![],
                                        };

                                        // 2. BM25 hits
                                        let bm25_hits = {
                                            let s = state_for_client.read().await;
                                            if let Some(ref idx) = s.index {
                                                idx.search(query)
                                            } else {
                                                vec![]
                                            }
                                        };

                                        // 3. Hybrid Fuse
                                        let fused = crate::hybrid_search::fuse(bm25_hits, vector_hits, top_k);

                                        let results: Vec<serde_json::Value> = fused.iter().map(|r| {
                                            serde_json::json!({
                                                "path": r.path.to_string_lossy(),
                                                "project": r.project,
                                                "snippet": r.snippet,
                                                "line": r.start_line,
                                                "score": r.rrf_score,
                                                "source": r.source.label()
                                            })
                                        }).collect();

                                        let response = format!("{{\"event\":\"VectorResults\",\"results\":{}}}\n", serde_json::to_string(&results).unwrap());
                                        let _ = writer.write_all(response.as_bytes()).await;
                                    }
                                } else if v["command"] == "Handover" {
                                    let target = v["target"].as_str().unwrap_or("unknown");
                                    let instruction = v["instruction"].as_str().unwrap_or("");
                                    let project_path = v["project_path"].as_str().unwrap_or("");

                                    let mut s = state_for_client.write().await;
                                    s.handover_count += 1;
                                    let limit = 5;

                                    if s.handover_count > limit {
                                        // Request approval
                                        println!("[Daemon] Handover limit exceeded. Requesting human approval.");
                                        let msg = format!("{{\"event\":\"HumanApprovalRequired\", \"target\":\"{}\", \"instruction\":\"{}\", \"reason\":\"Handover limit ({}) exceeded\"}}\n", target, instruction, limit);
                                        let _ = writer.write_all(msg.as_bytes()).await;
                                    } else {
                                        // Auto-approve and spawn
                                        println!("[Daemon] Auto-approving handover to {}. Count: {}", target, s.handover_count);
                                        let msg = format!("{{\"event\":\"HandoverApproved\", \"target\":\"{}\", \"instruction\":\"{}\", \"count\":{}}}\n", target, instruction, s.handover_count);
                                        let _ = writer.write_all(msg.as_bytes()).await;

                                        // Spawn in background
                                        let proxy = proxy_for_client.clone();
                                        let target_str = target.to_string();
                                        let path_str = project_path.to_string();
                                        tokio::spawn(async move {
                                            let _ = proxy.spawn_agent(&target_str, &path_str, 3600).await;
                                        });
                                    }
                                    sessions_for_client.record_event(&session_id, "handover", &format!("target={target}"));
                                } else if v["command"] == "HealthScan" {
                                    let s = state_for_client.read().await;
                                    let response = format!("{{\"event\":\"HealthReport\",\"report\":{}}}\n",
                                        serde_json::to_string(&s.health_reports).unwrap());
                                    let _ = writer.write_all(response.as_bytes()).await;
                                } else if v["command"] == "GetState" {
                                    let s = state_for_client.read().await;
                                    let response = serde_json::json!({
                                        "event": "StateSync",
                                        "projects": s.projects,
                                        "health_reports": s.health_reports,
                                        "active_agents": s.active_agents,
                                        "index_ready": s.index.is_some(),
                                        "handover_count": s.handover_count,
                                        "pending_file_changes": s.pending_file_changes
                                    });
                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                } else if v["command"] == "RequestFileChange" {
                                    let mut s = state_for_client.write().await;
                                    let approval = crate::daemon::state::FileChangeApproval {
                                        id: uuid::Uuid::new_v4(),
                                        path: v["path"].as_str().unwrap_or("").to_string(),
                                        original_content: v["original"].as_str().unwrap_or("").to_string(),
                                        new_content: v["new"].as_str().unwrap_or("").to_string(),
                                        agent_name: v["agent"].as_str().unwrap_or("unknown").to_string(),
                                    };
                                    sessions_for_client.record_event(&session_id, "file_change_request", &format!("path={}", approval.path));
                                    s.pending_file_changes.push(approval.clone());

                                    // Notify TUI to show diff
                                    let event = serde_json::json!({
                                        "event": "FileChangeRequested",
                                        "approval": approval
                                    });
                                    let _ = writer.write_all(format!("{}\n", event).as_bytes()).await;
                                } else if v["command"] == "ApproveFileChange" {
                                    if let Some(id_str) = v["id"].as_str() {
                                        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                                            let mut s = state_for_client.write().await;
                                            if let Some(pos) = s.pending_file_changes.iter().position(|a| a.id == id) {
                                                let approval = s.pending_file_changes.remove(pos);
                                                println!("[Daemon] Approved file change for: {}", approval.path);
                                                let _ = std::fs::write(&approval.path, &approval.new_content);
                                            }
                                        }
                                    }
                                } else if v["command"] == "HumanApproval" {
                                    if let Some(approved) = v["approved"].as_bool() {
                                        let mut s = state_for_client.write().await;
                                        if approved {
                                            println!("[Daemon] Human approved handover. Resetting counter.");
                                            s.handover_count = 0;
                                            let response = "{\"event\":\"HumanApprovalResult\",\"status\":\"approved\"}\n";
                                            let _ = writer.write_all(response.as_bytes()).await;
                                        } else {
                                            println!("[Daemon] Human rejected handover.");
                                            let response = "{\"event\":\"HumanApprovalResult\",\"status\":\"rejected\"}\n";
                                            let _ = writer.write_all(response.as_bytes()).await;
                                        }
                                    }
                                } else if v["command"] == "GetPendingDiffs" {
                                    let s = state_for_client.read().await;
                                    let diffs: Vec<&crate::daemon::state::PendingDiff> =
                                        s.pending_diffs.iter().collect();
                                    let response = serde_json::json!({
                                        "event": "PendingDiffs",
                                        "diffs": diffs
                                    });
                                    let _ = writer
                                        .write_all(format!("{}\n", response).as_bytes())
                                        .await;
                                } else if v["command"] == "ApproveDiff" {
                                    let diff_id = v["id"].as_str().unwrap_or("").to_string();
                                    let mut s = state_for_client.write().await;
                                    if let Some(pos) =
                                        s.pending_diffs.iter().position(|d| d.id == diff_id)
                                    {
                                        let diff = s.pending_diffs.remove(pos).unwrap();
                                        drop(s);
                                        match decode_base64(&diff.proposed) {
                                            Ok(content) => {
                                                // Path traversal guard
                                                let file_path = std::path::Path::new(&diff.file_path);
                                                let config = Config::load().unwrap_or_else(|| Config {
                                                    dev_ops_path: PathBuf::from(""),
                                                    master_md_path: PathBuf::from(""),
                                                    skills_path: PathBuf::from(""),
                                                    vault_projects_path: PathBuf::from(""),
                                                });
                                                let allowed_base = match config.dev_ops_path.canonicalize() {
                                                    Ok(p) => p,
                                                    Err(_) => {
                                                        let response = serde_json::json!({
                                                            "event": "DiffError",
                                                            "id": diff_id,
                                                            "error": "could not resolve allowed base path"
                                                        });
                                                        let _ = writer
                                                            .write_all(format!("{}\n", response).as_bytes())
                                                            .await;
                                                        return;
                                                    }
                                                };
                                                let canonical_target = match file_path.canonicalize() {
                                                    Ok(p) => p,
                                                    Err(_) => {
                                                        // File doesn't exist yet — use parent directory check
                                                        match file_path.parent().and_then(|p| p.canonicalize().ok()) {
                                                            Some(parent) if parent.starts_with(&allowed_base) => file_path.to_path_buf(),
                                                            _ => {
                                                                let response = serde_json::json!({
                                                                    "event": "DiffError",
                                                                    "id": diff_id,
                                                                    "error": "file path is outside workspace"
                                                                });
                                                                let _ = writer
                                                                    .write_all(format!("{}\n", response).as_bytes())
                                                                    .await;
                                                                return;
                                                            }
                                                        }
                                                    }
                                                };
                                                if !canonical_target.starts_with(&allowed_base) {
                                                    let response = serde_json::json!({
                                                        "event": "DiffError",
                                                        "id": diff_id,
                                                        "error": "file path is outside workspace"
                                                    });
                                                    let _ = writer
                                                        .write_all(format!("{}\n", response).as_bytes())
                                                        .await;
                                                    return;
                                                }
                                                // Safe to write
                                                match std::fs::write(file_path, &content) {
                                                    Ok(_) => {
                                                        let response = serde_json::json!({
                                                            "event": "DiffApproved",
                                                            "id": diff_id
                                                        });
                                                        let _ = writer
                                                            .write_all(
                                                                format!("{}\n", response)
                                                                    .as_bytes(),
                                                            )
                                                            .await;
                                                    }
                                                    Err(e) => {
                                                        let response = serde_json::json!({
                                                            "event": "DiffError",
                                                            "id": diff_id,
                                                            "error": format!("write failed: {e}")
                                                        });
                                                        let _ = writer
                                                            .write_all(
                                                                format!("{}\n", response)
                                                                    .as_bytes(),
                                                            )
                                                            .await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let response = serde_json::json!({
                                                    "event": "DiffError",
                                                    "id": diff_id,
                                                    "error": format!("base64 decode failed: {e}")
                                                });
                                                let _ = writer
                                                    .write_all(
                                                        format!("{}\n", response).as_bytes(),
                                                    )
                                                    .await;
                                            }
                                        }
                                    } else {
                                        drop(s);
                                        let response = serde_json::json!({
                                            "event": "DiffError",
                                            "id": diff_id,
                                            "error": format!("diff {} not found", diff_id)
                                        });
                                        let _ = writer
                                            .write_all(format!("{}\n", response).as_bytes())
                                            .await;
                                    }
                                } else if v["command"] == "RejectDiff" {
                                    let diff_id = v["id"].as_str().unwrap_or("").to_string();
                                    let mut s = state_for_client.write().await;
                                    s.pending_diffs.retain(|d| d.id != diff_id);
                                    drop(s);
                                    let response = serde_json::json!({
                                        "event": "DiffRejected",
                                        "id": diff_id
                                    });
                                    let _ = writer
                                        .write_all(format!("{}\n", response).as_bytes())
                                        .await;
                                } else if v["command"] == "SubmitJob" {
                                    let description = v["description"].as_str().unwrap_or("unnamed task").to_string();
                                    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();
                                    let project = v["project"].as_str().map(|s| s.to_string());
                                    let webhook_url = v["webhook_url"].as_str().map(|s| s.to_string());
                                    let shell_cmd = v["shell_cmd"].as_str().unwrap_or("").to_string();

                                    if shell_cmd.is_empty() {
                                        let err = serde_json::json!({
                                            "event": "JobError",
                                            "error": "shell_cmd is required"
                                        });
                                        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                    } else {
                                        let job = Job::new(
                                            &description,
                                            &agent_name,
                                            project.as_deref(),
                                            webhook_url.as_deref(),
                                        );
                                        let task = Box::pin(async move {
                                            let output = tokio::process::Command::new("cmd")
                                                .args(["/C", &shell_cmd])
                                                .output()
                                                .await?;
                                            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                                            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                                            if output.status.success() {
                                                Ok(stdout)
                                            } else {
                                                Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
                                            }
                                        });
                                        let job_id = factory_for_client.submit(job, task);
                                        let response = serde_json::json!({
                                            "event": "JobSubmitted",
                                            "job_id": job_id.to_string()
                                        });
                                        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                    }
                                } else if v["command"] == "GetJob" {
                                    if let Some(id_str) = v["job_id"].as_str() {
                                        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                                            match factory_for_client.get(&id) {
                                                Some(job) => {
                                                    let response = serde_json::json!({
                                                        "event": "JobInfo",
                                                        "job": job
                                                    });
                                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                                }
                                                None => {
                                                    let err = serde_json::json!({
                                                        "event": "JobError",
                                                        "error": format!("job {} not found", id_str)
                                                    });
                                                    let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                                }
                                            }
                                        }
                                    }
                                } else if v["command"] == "ListInbox" {
                                    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
                                    let jobs = factory_for_client.list_inbox(limit);
                                    let response = serde_json::json!({
                                        "event": "InboxList",
                                        "jobs": jobs
                                    });
                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                } else if v["command"] == "ListRunning" {
                                    let jobs = factory_for_client.list_running();
                                    let response = serde_json::json!({
                                        "event": "RunningList",
                                        "jobs": jobs
                                    });
                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                } else if v["command"] == "ExecuteCapability" {
                                    let capability = v["capability"].as_str().unwrap_or("").to_string();
                                    let input = v["input"].as_str().unwrap_or("").to_string();
                                    if capability.is_empty() {
                                        let err = serde_json::json!({
                                            "event": "CapabilityError",
                                            "error": "capability name is required"
                                        });
                                        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                    } else {
                                        match proxy_for_client_cap.execute(&capability, &input) {
                                            Ok(result) => {
                                                let response = serde_json::json!({
                                                    "event": "CapabilityResult",
                                                    "capability": capability,
                                                    "result": result
                                                });
                                                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                            }
                                            Err(e) => {
                                                let err = serde_json::json!({
                                                    "event": "CapabilityError",
                                                    "capability": capability,
                                                    "error": e.to_string()
                                                });
                                                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                            }
                                        }
                                    }
                                } else if v["command"] == "RouteCapability" {
                                    let query = v["query"].as_str().unwrap_or("").to_string();
                                    let input = v["input"].as_str().unwrap_or("").to_string();
                                    if query.is_empty() {
                                        let err = serde_json::json!({
                                            "event": "RouteError",
                                            "error": "query is required"
                                        });
                                        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                    } else {
                                        match proxy_for_client_cap.route(&query, &input) {
                                            Ok(result) => {
                                                let response = serde_json::json!({
                                                    "event": "RouteResult",
                                                    "query": query,
                                                    "result": result
                                                });
                                                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                            }
                                            Err(e) => {
                                                let err = serde_json::json!({
                                                    "event": "RouteError",
                                                    "query": query,
                                                    "error": e.to_string()
                                                });
                                                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                            }
                                        }
                                    }
                                } else if v["command"] == "ListCapabilities" {
                                    let caps: Vec<serde_json::Value> = proxy_for_client_cap
                                        .store()
                                        .list()
                                        .iter()
                                        .map(|c| serde_json::json!({
                                            "name": c.name,
                                            "description": c.description,
                                            "platforms": c.platforms
                                        }))
                                        .collect();
                                    let response = serde_json::json!({
                                        "event": "CapabilityList",
                                        "capabilities": caps
                                    });
                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                } else if v["command"] == "ListInstinctCandidates" {
                                    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
                                    let store = crate::evolution::CandidateStore::new(crate::evolution::CandidateStore::default_path());
                                    let candidates = store.list_pending(limit);
                                    let response = serde_json::json!({
                                        "event": "InstinctCandidatesList",
                                        "candidates": candidates
                                    });
                                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                } else if v["command"] == "PromoteInstinct" {
                                    if let Some(rule) = v["rule"].as_str() {
                                        let store = crate::evolution::CandidateStore::new(crate::evolution::CandidateStore::default_path());
                                        store.promote(rule);

                                        let mut engine = crate::instinct::InstinctEngine::init();
                                        engine.add_rule(rule.to_string());
                                        let _ = engine.save();

                                        let response = serde_json::json!({
                                            "event": "InstinctPromoted",
                                            "rule": rule
                                        });
                                        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                    } else {
                                        let err = serde_json::json!({
                                            "event": "PromoteError",
                                            "error": "rule is required"
                                        });
                                        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                    }
                                } else if v["command"] == "CreateTaskGraph" {
                                    let goal = v["goal"].as_str().unwrap_or("unnamed goal").to_string();
                                    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();
                                    let nodes_val = v["nodes"].as_array().cloned().unwrap_or_default();
                                    let nodes: Vec<crate::task_graph::NodeSpec> = nodes_val
                                        .iter()
                                        .filter_map(|n| {
                                            Some(crate::task_graph::NodeSpec {
                                                id: n["id"].as_str()?.to_string(),
                                                description: n["description"].as_str()?.to_string(),
                                                shell_cmd: n["shell_cmd"].as_str()?.to_string(),
                                                deps: n["deps"].as_array()
                                                    .map(|arr| arr.iter().filter_map(|d| d.as_str().map(ToOwned::to_owned)).collect())
                                                    .unwrap_or_default(),
                                            })
                                        })
                                        .collect();
                                    match graph_store_for_client.create(&goal, &agent_name, nodes) {
                                        Ok(graph_id) => {
                                            let response = serde_json::json!({
                                                "event": "TaskGraphCreated",
                                                "graph_id": graph_id
                                            });
                                            let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                        }
                                        Err(e) => {
                                            let err = serde_json::json!({
                                                "event": "TaskGraphError",
                                                "error": e.to_string()
                                            });
                                            let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                        }
                                    }
                                } else if v["command"] == "ExecuteTaskGraph" {
                                    if let Some(graph_id) = v["graph_id"].as_str().map(|s| s.to_string()) {
                                        let store = graph_store_for_client.clone();
                                        let factory = factory_for_client.clone();
                                        let gid = graph_id.clone();
                                        tokio::spawn(async move {
                                            execute_graph_async(store, factory, gid).await;
                                        });
                                        let response = serde_json::json!({
                                            "event": "TaskGraphExecuting",
                                            "graph_id": graph_id
                                        });
                                        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                    }
                                } else if v["command"] == "GetTaskGraph" {
                                    if let Some(graph_id) = v["graph_id"].as_str() {
                                        match graph_store_for_client.get(graph_id) {
                                            Some(graph) => {
                                                let response = serde_json::json!({
                                                    "event": "TaskGraphState",
                                                    "graph": graph
                                                });
                                                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                                            }
                                            None => {
                                                let err = serde_json::json!({
                                                    "event": "TaskGraphError",
                                                    "error": format!("graph {} not found", graph_id)
                                                });
                                                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                                            }
                                        }
                                    }
                                } else if v["command"] == "CreateSwarmTask" {
                                    let project_name = v["project_name"].as_str().unwrap_or("unknown").to_string();
                                    let project_path = v["project_path"].as_str().unwrap_or(".").to_string();
                                    let description = v["description"].as_str().unwrap_or("").to_string();
                                    let agent = v["agent"].as_str().unwrap_or("claude").to_string();
                                    match swarm_store_for_client.create(
                                        &project_name,
                                        std::path::Path::new(&project_path),
                                        &description,
                                        &agent,
                                    ) {
                                        Ok(task) => {
                                            let r = serde_json::json!({"event":"SwarmTaskCreated","task_id":task.id});
                                            let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                        }
                                        Err(e) => {
                                            let r = serde_json::json!({"event":"SwarmError","error":e.to_string()});
                                            let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                        }
                                    }
                                } else if v["command"] == "GetSwarmTask" {
                                    if let Some(id) = v["task_id"].as_str() {
                                        let r = match swarm_store_for_client.get(id) {
                                            Some(task) => serde_json::json!({"event":"SwarmTaskState","task":task}),
                                            None => serde_json::json!({"event":"SwarmError","error":format!("task {} not found", id)}),
                                        };
                                        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                    }
                                } else if v["command"] == "ListSwarmTasks" {
                                    let tasks = swarm_store_for_client.list_active();
                                    let r = serde_json::json!({"event":"SwarmTaskList","tasks":tasks});
                                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                } else if v["command"] == "ApproveSwarmTask" {
                                    if let Some(id) = v["task_id"].as_str() {
                                        if let Some(task) = swarm_store_for_client.get(id) {
                                            let msg = format!("swarm merge: {}", task.task_description);
                                            match crate::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg) {
                                                Ok(_) => {
                                                    let _ = crate::swarm::worktree::remove_worktree(&task.project_path, &task.worktree_path);
                                                    swarm_store_for_client.set_status(id, crate::swarm::SwarmStatus::Merged);
                                                    let r = serde_json::json!({"event":"SwarmTaskMerged","task_id":id});
                                                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                                }
                                                Err(e) => {
                                                    let r = serde_json::json!({"event":"SwarmError","error":e.to_string()});
                                                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                                }
                                            }
                                        }
                                    }
                                } else if v["command"] == "ListEvolutionCandidates" {
                                    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
                                    let candidates = evolution_store_for_client.list_pending(limit);
                                    let r = serde_json::json!({"event":"EvolutionCandidates","candidates":candidates});
                                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                } else if v["command"] == "PromoteEvolutionCandidate" {
                                    if let Some(rule) = v["rule"].as_str() {
                                        evolution_store_for_client.promote(rule);
                                        let r = serde_json::json!({"event":"EvolutionCandidatePromoted","rule":rule});
                                        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                    }
                                } else if v["command"] == "PruneExpiredCandidates" {
                                    let removed = evolution_store_for_client.sweep_expired();
                                    let r = serde_json::json!({"event":"EvolutionPruned","removed":removed});
                                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                } else if v["command"] == "RejectSwarmTask" {
                                    if let Some(id) = v["task_id"].as_str() {
                                        if let Some(task) = swarm_store_for_client.get(id) {
                                            let _ = crate::swarm::worktree::remove_worktree(&task.project_path, &task.worktree_path);
                                            let _ = crate::swarm::merge::delete_branch(&task.project_path, &task.branch_name);
                                            swarm_store_for_client.set_status(id, crate::swarm::SwarmStatus::Rejected);
                                            let r = serde_json::json!({"event":"SwarmTaskRejected","task_id":id});
                                            let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                        }
                                    }
                                }
                            }
                            line.clear();
                        }

                        // Read from broadcast
                        msg_res = rx.recv() => {
                            if let Ok(msg) = msg_res {
                                let mut payload = msg.clone();
                                payload.push('\n');
                                if writer.write_all(payload.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }

                sessions_for_client.end(&session_id, None);

                // Auto-append to memory.md if we know the project
                if let Some(sess) = sessions_for_client.get(&session_id) {
                    if let Some(proj) = &sess.project {
                        if let Some(config) = Config::load() {
                            let mem_path = config.dev_ops_path.join(proj).join("memory.md");
                            if mem_path.exists() {
                                sessions_for_client.append_to_memory(&session_id, &mem_path);
                            }
                        }
                    }
                }
                println!("[Daemon] Session {} ended for client {}", session_id, addr);
            });
        }
    }
}

fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(Into::into)
}

async fn execute_graph_async(
    store: std::sync::Arc<crate::task_graph::GraphStore>,
    factory: std::sync::Arc<crate::factory::Factory>,
    graph_id: String,
) {
    let timeout = std::time::Duration::from_secs(600);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            eprintln!("[TaskGraph] Execution timeout for graph {}", graph_id);
            break;
        }

        let ready = store.ready_nodes(&graph_id);
        if ready.is_empty() {
            if let Some(graph) = store.get(&graph_id) {
                if graph.status != "pending" && graph.status != "running" {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }

        for node in ready {
            let store2 = store.clone();
            let factory2 = factory.clone();
            let gid = graph_id.clone();
            let nid = node.id.clone();
            let cmd = node.shell_cmd.clone();
            let desc = node.description.clone();

            let job = crate::factory::Job::new(&desc, "graph-executor", None, None);
            let job_id = job.id;

            store2.mark_node_running(&gid, &nid, &job_id.to_string());

            factory2.submit(
                job,
                Box::pin(async move {
                    let output = tokio::process::Command::new("cmd")
                        .args(["/C", &cmd])
                        .output()
                        .await?;
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let result = if output.status.success() {
                        Ok(stdout)
                    } else {
                        Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
                    };
                    match &result {
                        Ok(r) => store2.mark_node_complete(&gid, &nid, r, &job_id.to_string()),
                        Err(e) => store2.mark_node_failed(&gid, &nid, &e.to_string()),
                    }
                    result
                }),
            );
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
