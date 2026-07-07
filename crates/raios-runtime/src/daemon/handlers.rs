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

/// Collects every string in an incoming WS command that AgentShield (UMAI
/// Layer 1, see `security/umai.rs`) needs to regex-scan before dispatch.
///
/// Most commands carry their payload in a top-level `input`/`query`/
/// `shell_cmd` field. `CreateTaskGraph` is the exception: its shell commands
/// live in a nested `nodes[].shell_cmd` array, one per graph node. Without
/// pulling those in explicitly, the scan below silently never saw them —
/// AgentShield's dangerous-pattern check (`rm -rf /`, `curl … | sh`, etc.)
/// had a blind spot for the exact field that `daemon/cmd/task_graph.rs`
/// later hands to a shell.
fn collect_scan_payload(v: &serde_json::Value) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(s) = v.get("input").and_then(|p| p.as_str()) {
        parts.push(s);
    }
    if let Some(s) = v.get("query").and_then(|p| p.as_str()) {
        parts.push(s);
    }
    if let Some(s) = v.get("shell_cmd").and_then(|p| p.as_str()) {
        parts.push(s);
    }
    if let Some(nodes) = v.get("nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            if let Some(s) = node.get("shell_cmd").and_then(|p| p.as_str()) {
                parts.push(s);
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
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

/// Checks a raw first line from a new connection against the expected
/// `AUTH <token>` challenge, using a constant-time comparison so the
/// response timing doesn't leak how many leading bytes of the token matched.
/// Pulled out as a pure function so the auth decision itself — the part a
/// careless refactor is most likely to quietly break — can be unit tested
/// without standing up a full ClientHandle (socket, DB-backed stores, etc).
fn check_auth_line(line: &str, server_token: &str) -> bool {
    match line.trim().strip_prefix("AUTH ") {
        Some(provided) => {
            raios_core::security::constant_time_compare(provided.as_bytes(), server_token.as_bytes())
        }
        None => false,
    }
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
        if n == 0 || !check_auth_line(&line, &server_token) {
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
                    let combined_payload = collect_scan_payload(&v);
                    let raw_payload = combined_payload.as_deref();
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

#[cfg(test)]
mod payload_scan_tests {
    use super::collect_scan_payload;
    use serde_json::json;

    #[test]
    fn collects_top_level_input_field() {
        let v = json!({ "command": "ExecuteCapability", "input": "danger" });
        assert_eq!(collect_scan_payload(&v).as_deref(), Some("danger"));
    }

    #[test]
    fn collects_nested_node_shell_cmds() {
        let v = json!({
            "command": "CreateTaskGraph",
            "goal": "build",
            "nodes": [
                { "id": "a", "description": "d", "shell_cmd": "rm -rf /" },
                { "id": "b", "description": "d", "shell_cmd": "echo ok" },
            ]
        });
        let payload = collect_scan_payload(&v).unwrap();
        assert!(payload.contains("rm -rf /"), "payload was: {payload}");
        assert!(payload.contains("echo ok"), "payload was: {payload}");
    }

    #[test]
    fn returns_none_when_nothing_scannable() {
        let v = json!({ "command": "ListProjects" });
        assert_eq!(collect_scan_payload(&v), None);
    }

    /// End-to-end regression test: before this fix, AgentShield's dangerous-
    /// pattern scan (`raios_core::security::Umai`, Layer 1) never received a
    /// `CreateTaskGraph` node's `shell_cmd` at all, so a node like
    /// `{"shell_cmd": "rm -rf /"}` sailed through UMAI untouched (its fate
    /// then depended entirely on `default_action` in raios-policy.toml).
    /// With `collect_scan_payload` wired into the dispatch loop, the same
    /// payload is now denied by Layer 1 before Layer 2 (the policy rule) is
    /// even consulted.
    #[test]
    fn nested_shell_cmd_dangerous_pattern_is_denied_by_umai() {
        use raios_core::security::Umai;

        let v = json!({
            "command": "CreateTaskGraph",
            "goal": "build",
            "nodes": [
                { "id": "a", "description": "d", "shell_cmd": "rm -rf /" }
            ]
        });
        let payload = collect_scan_payload(&v);
        let umai = Umai::new(None);
        let decision = umai.check("CreateTaskGraph", payload.as_deref());
        assert!(!decision.is_allowed(), "expected Deny, got {decision:?}");
    }
}

#[cfg(test)]
mod auth_handshake_tests {
    use super::*;
    use crate::proxy_store::CapabilityStore;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn correct_token_authenticates() {
        assert!(check_auth_line("AUTH secret123\n", "secret123"));
    }

    #[test]
    fn wrong_token_is_rejected() {
        assert!(!check_auth_line("AUTH wrong\n", "secret123"));
    }

    #[test]
    fn missing_auth_prefix_is_rejected() {
        assert!(!check_auth_line("secret123\n", "secret123"));
        assert!(!check_auth_line("{\"command\":\"ListProjects\"}\n", "secret123"));
    }

    #[test]
    fn empty_line_is_rejected() {
        assert!(!check_auth_line("", "secret123"));
        assert!(!check_auth_line("\n", "secret123"));
    }

    #[test]
    fn trailing_whitespace_is_trimmed() {
        assert!(check_auth_line("AUTH secret123\r\n", "secret123"));
    }

    /// Builds a real ClientHandle backed by tempdir-local SQLite files —
    /// never the shared `~/.config/raios/workspace.db` — so this test can't
    /// leak state into (or get confused by) a developer's real daemon data.
    /// `pub(super)` so the sibling `dispatch_loop_tests` module can reuse it.
    pub(super) fn test_client_handle(
        socket: TcpStream,
        addr: std::net::SocketAddr,
        server_token: String,
        tmp: &tempfile::TempDir,
    ) -> ClientHandle {
        let (tx, rx) = broadcast::channel::<String>(16);
        let telem_rx = tx.subscribe();
        let state = DaemonState::new();
        ClientHandle {
            socket,
            addr,
            rx,
            telem_rx,
            tx: tx.clone(),
            execution_proxy: super::super::proxy::ExecutionProxy::new(state.clone()),
            state,
            server_token,
            sessions: Arc::new(SessionStore::new(tmp.path().join("sessions.db"))),
            factory: Arc::new(crate::factory::Factory::new(tx)),
            proxy: Arc::new(CapabilityProxy::new(CapabilityStore::new())),
            graph_store: Arc::new(raios_core::task_graph::GraphStore::new(
                tmp.path().join("graph.db"),
            )),
            swarm_store: Arc::new(crate::swarm::store::SwarmStore::new(
                tmp.path().join("swarm.db"),
            )),
            evolution_store: Arc::new(crate::evolution::CandidateStore::new(
                tmp.path().join("evolution.db"),
            )),
            umai: Umai::new(None),
        }
    }

    /// The one integration-level path that's safe to exercise automatically:
    /// a rejected AUTH returns before `handle_client_connection` ever calls
    /// `raios_core::db::open_db()` (the shared, non-injectable global DB), so
    /// this can run in any environment without touching real daemon state.
    /// The success path is covered indirectly by `correct_token_authenticates`
    /// above; a full success-path integration test is deliberately not
    /// attempted here since it would hit that shared database.
    #[tokio::test]
    async fn wrong_token_over_real_socket_is_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut client = TcpStream::connect(addr).await.unwrap();
        let (socket, peer_addr) = listener.accept().await.unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let handle = test_client_handle(socket, peer_addr, "correct-token".into(), &tmp);
        let server_task = tokio::spawn(handle_client_connection(handle));

        client.write_all(b"AUTH definitely-wrong-token\n").await.unwrap();

        let mut buf = [0u8; 512];
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("\"event\":\"Error\""), "response was: {resp}");
        assert!(resp.contains("Authentication failed"), "response was: {resp}");

        // Server must close right after rejecting — a further read returns 0 (EOF).
        let n2 = client.read(&mut buf).await.unwrap();
        assert_eq!(n2, 0, "server should have closed the connection after auth failure");

        server_task.await.unwrap();
    }
}

/// Covers the post-auth command dispatch loop — specifically, that a UMAI
/// deny decision (Layer 1 AgentShield) is actually reflected in what comes
/// back over the wire, not just in the in-process `Umai::check` return value
/// already covered by `payload_scan_tests`. The dispatch loop also calls
/// `raios_core::db::open_db()` (audit ledger write) on every command, which
/// resolves a fixed `$HOME/.config/raios/workspace.db` with no injection
/// seam — these tests redirect HOME/XDG_CONFIG_HOME to a tempdir for their
/// duration, serialized since that's process-global state.
#[cfg(all(test, unix))]
mod dispatch_loop_tests {
    use super::auth_handshake_tests::test_client_handle;
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct IsolatedHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        original_home: Option<String>,
        original_xdg_config_home: Option<String>,
        _tmp: tempfile::TempDir,
    }

    impl IsolatedHome {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let tmp = tempfile::TempDir::new().unwrap();
            let original_home = std::env::var("HOME").ok();
            let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
            std::env::set_var("HOME", tmp.path());
            std::env::remove_var("XDG_CONFIG_HOME");
            Self {
                _lock: lock,
                original_home,
                original_xdg_config_home,
                _tmp: tmp,
            }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
            match &self.original_xdg_config_home {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn dangerous_shell_cmd_in_create_task_graph_is_denied_over_the_wire() {
        let _home = IsolatedHome::new();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).await.unwrap();
        let (socket, peer_addr) = listener.accept().await.unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let handle = test_client_handle(socket, peer_addr, "test-token".into(), &tmp);
        let server_task = tokio::spawn(handle_client_connection(handle));

        client.write_all(b"AUTH test-token\n").await.unwrap();

        // Drain the AUTH ack + SessionStarted line before sending a command.
        let mut buf = [0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..n]).contains("SessionStarted"),
            "expected SessionStarted after successful auth"
        );

        let cmd = serde_json::json!({
            "command": "CreateTaskGraph",
            "goal": "test",
            "nodes": [
                { "id": "a", "description": "d", "shell_cmd": "rm -rf /" }
            ]
        });
        client
            .write_all(format!("{cmd}\n").as_bytes())
            .await
            .unwrap();

        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("\"event\":\"UmaiBlocked\""),
            "expected the dangerous shell_cmd to be denied over the wire, got: {resp}"
        );

        drop(client);
        let _ = server_task.await;
    }
}
