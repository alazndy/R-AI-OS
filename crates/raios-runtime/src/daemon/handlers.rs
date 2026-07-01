use super::state::DaemonState;
use crate::proxy_store::CapabilityProxy;
use raios_core::security::{Umai, UmaiDecision};
use crate::session::SessionStore;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, RwLock};

/// Records one daemon WS command decision into the tamper-evident audit ledger.
/// Mirrors `McpServer::record_tool_audit` (src/mcp/tools.rs) so both dispatch
/// paths feed the same `raios policy suggest` learning pipeline.
fn record_ws_tool_audit(umai: &Umai, cmd_name: &str, raw_payload: Option<&str>, decision: &UmaiDecision) {
    let Ok(conn) = raios_core::db::open_db() else { return };
    let event_type = match decision {
        UmaiDecision::Allow => "tool_allow",
        UmaiDecision::Deny(_) => "tool_deny",
        UmaiDecision::Confirm(_) => "tool_confirm",
    };
    let args_hash = format!("{:x}", Sha256::digest(raw_payload.unwrap_or("").as_bytes()));
    let actor = std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());
    let _ = raios_core::security::record_tool_decision(
        &conn,
        cmd_name,
        &args_hash,
        umai.rule_source(cmd_name),
        event_type,
        &actor,
    );
}

/// All context needed to handle one TCP client connection.
pub struct ClientHandle {
    pub socket: tokio::net::TcpStream,
    pub addr: std::net::SocketAddr,
    pub rx: broadcast::Receiver<String>,
    pub telem_rx: broadcast::Receiver<String>,
    pub tx: broadcast::Sender<String>,
    pub state: Arc<RwLock<DaemonState>>,
    pub execution_proxy: super::proxy::ExecutionProxy,
    pub server_token: String,
    pub sessions: Arc<SessionStore>,
    pub factory: Arc<crate::factory::Factory>,
    pub proxy: Arc<CapabilityProxy>,
    pub graph_store: Arc<raios_core::task_graph::GraphStore>,
    pub swarm_store: Arc<crate::swarm::store::SwarmStore>,
    pub evolution_store: Arc<crate::evolution::CandidateStore>,
    pub umai: raios_core::security::Umai,
}

pub async fn handle_client_connection(h: ClientHandle) {
    let mut socket = h.socket;
    let addr = h.addr;
    let mut rx = h.rx;
    let mut telem_rx = h.telem_rx;
    let _tx_sender = h.tx;
    let state_for_client = h.state;
    let proxy_for_client = h.execution_proxy;
    let server_token = h.server_token;
    let sessions_for_client = h.sessions;
    let factory_for_client = h.factory;
    let proxy_for_client_cap = h.proxy;
    let graph_store_for_client = h.graph_store;
    let swarm_store_for_client = h.swarm_store;
    let evolution_store_for_client = h.evolution_store;
    let umai = h.umai;

    use tokio::io::{AsyncBufReadExt, BufReader};
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Authentication challenge
    if let Ok(n) = reader.read_line(&mut line).await {
        if n == 0 || !line.trim().starts_with("AUTH ") || line.trim()[5..] != server_token {
            println!("[Daemon] Auth failed for client {}. Dropping connection.", addr);
            let _ = writer
                .write_all(b"{\"event\":\"Error\",\"message\":\"Authentication failed\"}\n")
                .await;
            return;
        }
        println!("[Daemon] Client {} authenticated.", addr);
    } else {
        return;
    }
    line.clear();

    // Auto-start session
    let session_id = sessions_for_client.start("daemon-client", None);
    let session_msg = serde_json::json!({ "event": "SessionStarted", "session_id": session_id });
    let _ = writer.write_all(format!("{}\n", session_msg).as_bytes()).await;

    // Replay last 200 log entries so TUI has history on connect
    if let Ok(conn) = raios_core::db::open_db() {
        if let Ok(entries) = raios_core::db::cp_logs_replay(&conn, 200) {
            for (ts, sender, content) in entries {
                let replay = serde_json::json!({
                    "event": "NewLog",
                    "log": {
                        "timestamp": &ts[11..19].replace('T', "").chars().take(8).collect::<String>(),
                        "sender": sender,
                        "content": content
                    }
                });
                if writer.write_all(format!("{}\n", replay).as_bytes()).await.is_err() {
                    return;
                }
            }
        }
    }

    loop {
        tokio::select! {
            res = reader.read_line(&mut line) => {
                if res.unwrap_or(0) == 0 { break; }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    let cmd_name = v["command"].as_str().unwrap_or("");
                    let raw_payload = v.get("input")
                        .or_else(|| v.get("query"))
                        .or_else(|| v.get("shell_cmd"))
                        .and_then(|p| p.as_str());
                    let umai_result = umai.check(cmd_name, raw_payload);
                    record_ws_tool_audit(&umai, cmd_name, raw_payload, &umai_result);
                    if !umai_result.is_allowed() {
                        let _ = writer
                            .write_all(format!("{}\n", umai_result.into_error_json()).as_bytes())
                            .await;
                        line.clear();
                        continue;
                    }

                    match v["command"].as_str().unwrap_or("") {
                        "AgentInfo" => {
                            if let Some(agent) = v["agent"].as_str() {
                                let project = v["project"].as_str();
                                sessions_for_client.record_event(
                                    &session_id,
                                    "agent_info",
                                    &format!("agent={} project={}", agent, project.unwrap_or("-")),
                                );
                                if let Ok(conn) = rusqlite::Connection::open(
                                    crate::session::SessionStore::default_path(),
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
                        }
                        "Search" => {
                            if let Some(query) = v["query"].as_str() {
                                let s = state_for_client.read().await;
                                if let Some(ref idx) = s.index {
                                    let results = idx.search(query);
                                    let response = format!(
                                        "{{\"event\":\"SearchResults\",\"results\":{}}}\n",
                                        serde_json::to_string(&results).unwrap()
                                    );
                                    let _ = writer.write_all(response.as_bytes()).await;
                                }
                            }
                        }
                        "VectorSearch" => {
                            if let Some(query) = v["query"].as_str() {
                                let top_k = v["top_k"].as_u64().unwrap_or(10) as usize;
                                let vector_hits = match crate::cortex::Cortex::init() {
                                    Ok(cortex) => cortex.search(query, top_k).unwrap_or_default(),
                                    Err(_) => vec![],
                                };
                                let bm25_hits = {
                                    let s = state_for_client.read().await;
                                    if let Some(ref idx) = s.index { idx.search(query) } else { vec![] }
                                };
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
                                let response = format!(
                                    "{{\"event\":\"VectorResults\",\"results\":{}}}\n",
                                    serde_json::to_string(&results).unwrap()
                                );
                                let _ = writer.write_all(response.as_bytes()).await;
                            }
                        }
                        "Handover" => {
                            let target = v["target"].as_str().unwrap_or("unknown");
                            let instruction = v["instruction"].as_str().unwrap_or("");
                            let project_path = v["project_path"].as_str().unwrap_or("");
                            let mut s = state_for_client.write().await;
                            s.handover_count += 1;
                            let limit = 5;
                            if s.handover_count > limit {
                                println!("[Daemon] Handover limit exceeded. Requesting human approval.");
                                let msg = format!(
                                    "{{\"event\":\"HumanApprovalRequired\",\"target\":\"{}\",\"instruction\":\"{}\",\"reason\":\"Handover limit ({}) exceeded\"}}\n",
                                    target, instruction, limit
                                );
                                let _ = writer.write_all(msg.as_bytes()).await;
                            } else {
                                println!("[Daemon] Auto-approving handover to {}. Count: {}", target, s.handover_count);
                                let msg = format!(
                                    "{{\"event\":\"HandoverApproved\",\"target\":\"{}\",\"instruction\":\"{}\",\"count\":{}}}\n",
                                    target, instruction, s.handover_count
                                );
                                let _ = writer.write_all(msg.as_bytes()).await;
                                let proxy = proxy_for_client.clone();
                                let target_str = target.to_string();
                                let path_str = project_path.to_string();
                                tokio::spawn(async move {
                                    let _ = proxy.spawn_agent(&target_str, &path_str, 3600).await;
                                });
                            }
                            sessions_for_client.record_event(
                                &session_id, "handover", &format!("target={target}"),
                            );
                        }
                        "HealthScan" => {
                            let s = state_for_client.read().await;
                            let report_json = serde_json::to_string(&s.health_reports).unwrap();
                            let response = format!(
                                "{{\"event\":\"HealthReport\",\"report\":{}}}\n", report_json
                            );
                            let _ = writer.write_all(response.as_bytes()).await;
                            let delta = format!("{{\"event\":\"HealthDelta\",\"report\":{}}}", report_json);
                            let _ = _tx_sender.send(delta);
                        }
                        "GetState" => {
                            let s = state_for_client.read().await;
                            let _ = writer.write_all(format!("{}\n", s.sync_payload()).as_bytes()).await;
                        }
                        "GetLogs" => {
                            let limit = v["limit"].as_u64().unwrap_or(200) as usize;
                            if let Ok(conn) = raios_core::db::open_db() {
                                if let Ok(entries) = raios_core::db::cp_logs_replay(&conn, limit) {
                                    for (ts, sender, content) in entries {
                                        let msg = serde_json::json!({
                                            "event": "NewLog",
                                            "log": {
                                                "timestamp": &ts[11..].chars().take(8).collect::<String>(),
                                                "sender": sender,
                                                "content": content
                                            }
                                        });
                                        if writer.write_all(format!("{}\n", msg).as_bytes()).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        // ── file change ──────────────────────────────────────────────────
                        "RequestFileChange" => {
                            super::cmd::file::handle_request_file_change(
                                &v, &state_for_client, &sessions_for_client, &session_id, &mut writer,
                            ).await;
                        }
                        "ApproveFileChange" => {
                            if matches!(
                                super::cmd::file::handle_approve_file_change(&v, &state_for_client).await,
                                super::cmd::CmdResult::Continue
                            ) {
                                continue;
                            }
                        }
                        "HumanApproval" => {
                            super::cmd::file::handle_human_approval(
                                &v, &state_for_client, &mut writer,
                            ).await;
                        }
                        "GetPendingDiffs" => {
                            super::cmd::file::handle_get_pending_diffs(
                                &state_for_client, &mut writer,
                            ).await;
                        }
                        "ApproveDiff" => {
                            if matches!(
                                super::cmd::file::handle_approve_diff(
                                    &v, &state_for_client, &mut writer,
                                ).await,
                                super::cmd::CmdResult::Disconnect
                            ) {
                                return;
                            }
                        }
                        "RejectDiff" => {
                            super::cmd::file::handle_reject_diff(
                                &v, &state_for_client, &mut writer,
                            ).await;
                        }
                        // ── job factory ──────────────────────────────────────────────────
                        "SubmitJob" => {
                            super::cmd::jobs::handle_submit_job(
                                &v, &factory_for_client, &mut writer,
                            ).await;
                        }
                        "GetJob" => {
                            super::cmd::jobs::handle_get_job(
                                &v, &factory_for_client, &mut writer,
                            ).await;
                        }
                        "ListInbox" => {
                            super::cmd::jobs::handle_list_inbox(
                                &v, &factory_for_client, &mut writer,
                            ).await;
                        }
                        "ListRunning" => {
                            super::cmd::jobs::handle_list_running(
                                &factory_for_client, &mut writer,
                            ).await;
                        }
                        // ── capabilities ─────────────────────────────────────────────────
                        "ExecuteCapability" => {
                            super::cmd::capability::handle_execute_capability(
                                &v, &proxy_for_client_cap, &mut writer,
                            ).await;
                        }
                        "RouteCapability" => {
                            super::cmd::capability::handle_route_capability(
                                &v, &proxy_for_client_cap, &mut writer,
                            ).await;
                        }
                        "ListCapabilities" => {
                            super::cmd::capability::handle_list_capabilities(
                                &proxy_for_client_cap, &mut writer,
                            ).await;
                        }
                        // ── instinct / evolution ──────────────────────────────────────────
                        "ListInstinctCandidates" => {
                            super::cmd::evolution::handle_list_instinct_candidates(
                                &v, &mut writer,
                            ).await;
                        }
                        "PromoteInstinct" => {
                            super::cmd::evolution::handle_promote_instinct(
                                &v, &mut writer,
                            ).await;
                        }
                        "ListEvolutionCandidates" => {
                            super::cmd::evolution::handle_list_evolution_candidates(
                                &v, &evolution_store_for_client, &mut writer,
                            ).await;
                        }
                        "PromoteEvolutionCandidate" => {
                            super::cmd::evolution::handle_promote_evolution_candidate(
                                &v, &evolution_store_for_client, &mut writer,
                            ).await;
                        }
                        "PruneExpiredCandidates" => {
                            super::cmd::evolution::handle_prune_expired_candidates(
                                &evolution_store_for_client, &mut writer,
                            ).await;
                        }
                        // ── task graph ────────────────────────────────────────────────────
                        "CreateTaskGraph" => {
                            super::cmd::task_graph::handle_create_task_graph(
                                &v, &graph_store_for_client, &mut writer,
                            ).await;
                        }
                        "ExecuteTaskGraph" => {
                            super::cmd::task_graph::handle_execute_task_graph(
                                &v, &graph_store_for_client, &factory_for_client, &mut writer,
                            ).await;
                        }
                        "GetTaskGraph" => {
                            super::cmd::task_graph::handle_get_task_graph(
                                &v, &graph_store_for_client, &mut writer,
                            ).await;
                        }
                        // ── swarm ─────────────────────────────────────────────────────────
                        "CreateSwarmTask" => {
                            super::cmd::swarm::handle_create_swarm_task(
                                &v, &swarm_store_for_client, &mut writer,
                            ).await;
                        }
                        "GetSwarmTask" => {
                            super::cmd::swarm::handle_get_swarm_task(
                                &v, &swarm_store_for_client, &mut writer,
                            ).await;
                        }
                        "ListSwarmTasks" => {
                            super::cmd::swarm::handle_list_swarm_tasks(
                                &swarm_store_for_client, &mut writer,
                            ).await;
                        }
                        "ApproveSwarmTask" => {
                            super::cmd::swarm::handle_approve_swarm_task(
                                &v, &swarm_store_for_client, &mut writer,
                            ).await;
                        }
                        "RejectSwarmTask" => {
                            super::cmd::swarm::handle_reject_swarm_task(
                                &v, &swarm_store_for_client, &mut writer,
                            ).await;
                        }
                        _ => {}
                    }
                }
                line.clear();
            }

            // Control channel — guaranteed delivery, high priority
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

        // Telemetry channel — lossy, drain without blocking
        loop {
            match telem_rx.try_recv() {
                Ok(msg) => {
                    let mut payload = msg;
                    payload.push('\n');
                    if writer.write_all(payload.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
    }

    sessions_for_client.end(&session_id, None);

    // Auto-append to memory.md if we know the project
    if let Some(sess) = sessions_for_client.get(&session_id) {
        if let Some(proj) = &sess.project {
            if let Some(config) = raios_core::config::Config::load() {
                let mem_path = config.dev_ops_path.join(proj).join("memory.md");
                if mem_path.exists() {
                    sessions_for_client.append_to_memory(&session_id, &mem_path);
                }
            }
        }
    }
    println!("[Daemon] Session {} ended for client {}", session_id, addr);
}
