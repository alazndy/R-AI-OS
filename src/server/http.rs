use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::config::Config;
use crate::daemon::state::DaemonState;
use crate::security::SessionTokenManager;

/// State shared across HTTP route handlers.
#[derive(Clone)]
struct AppState {
    daemon_state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
}

/// Start the Axum HTTP & WebSocket API Server.
pub async fn start_http_server(
    port: u16,
    daemon_state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let app_state = AppState { daemon_state, tx };

    // Define router with auth middleware
    let app = Router::new()
        .route("/api/health", get(handle_health))
        .route("/api/projects", get(handle_projects))
        .route("/api/tasks", get(handle_tasks))
        .route("/api/approve", post(handle_approve))
        .route("/api/stream", get(handle_websocket))
        .layer(axum::middleware::from_fn(auth_middleware))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("[Kernel] HTTP API Adapter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// HTTP middleware to enforce the secure bootstrap token auth.
/// Host header is also validated to block DNS rebinding attacks.
async fn auth_middleware(
    headers: HeaderMap,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    // 1. Verify Host header is strictly localhost or 127.0.0.1
    if let Some(host) = headers.get(header::HOST) {
        let host_str = host.to_str().unwrap_or("").to_lowercase();
        if !host_str.starts_with("localhost") && !host_str.starts_with("127.0.0.1") {
            eprintln!("[HTTP Auth] Blocked request due to foreign Host header: {}", host_str);
            return Err(StatusCode::BAD_REQUEST);
        }
    } else {
        return Err(StatusCode::BAD_REQUEST);
    }

    // 2. Validate session token via Authorization header
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            eprintln!("[HTTP Auth] Blocked request due to missing Bearer token");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let token_manager = SessionTokenManager::new();
    if !token_manager.validate_token(token) {
        eprintln!("[HTTP Auth] Blocked request: Invalid or expired token");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Proceed to next handler
    Ok(next.run(req).await)
}

/// GET /api/health
async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.daemon_state.read().await;
    let payload = json!({
        "status": "ok",
        "handover_count": s.handover_count,
        "needs_human_approval": s.needs_human_approval,
        "active_agents": s.active_agents,
    });
    Json(payload)
}

/// GET /api/projects
async fn handle_projects(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.daemon_state.read().await;
    Json(s.projects.clone())
}

/// GET /api/tasks
/// Retrieves all tasks from SQLite.
async fn handle_tasks() -> impl IntoResponse {
    let config = Config::load().unwrap_or_else(|| {
        let detected = Config::auto_detect();
        Config {
            dev_ops_path: detected.dev_ops.unwrap_or_else(|| std::path::PathBuf::from(".")),
            master_md_path: detected.master_md.unwrap_or_else(|| std::path::PathBuf::from("MASTER.md")),
            skills_path: detected.skills.unwrap_or_else(|| std::path::PathBuf::from(".agents/skills")),
            vault_projects_path: detected.vault_projects.unwrap_or_default(),
        }
    });

    match crate::tasks::load_tasks(&config.dev_ops_path) {
        Ok(tasks) => Json(json!({ "status": "ok", "tasks": tasks })),
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct ApprovePayload {
    task_id: String,
}

/// POST /api/approve
/// Handles human-in-the-loop approvals for swarm tasks or files changes.
async fn handle_approve(
    State(state): State<AppState>,
    Json(payload): Json<ApprovePayload>,
) -> impl IntoResponse {
    // 1. Approve Swarm Task if ID matches
    let swarm_store = Arc::new(crate::swarm::store::SwarmStore::new(
        crate::swarm::store::SwarmStore::default_path(),
    ));

    if let Some(task) = swarm_store.get(&payload.task_id) {
        let msg = format!("swarm merge: {}", task.task_description);
        match crate::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg) {
            Ok(_) => {
                let _ = crate::swarm::worktree::remove_worktree(
                    &task.project_path,
                    &task.worktree_path,
                );
                swarm_store.set_status(&payload.task_id, crate::swarm::SwarmStatus::Merged);
                return Json(json!({ "status": "ok", "message": format!("Swarm task {} approved and merged", payload.task_id) }));
            }
            Err(e) => {
                return Json(json!({ "status": "error", "message": e.to_string() }));
            }
        }
    }

    // 2. Approve Diff if ID matches
    let mut s = state.daemon_state.write().await;
    if let Some(pos) = s.pending_diffs.iter().position(|d| d.id == payload.task_id) {
        let diff = s.pending_diffs.remove(pos).unwrap();
        drop(s);

        if let Ok(content) = decode_base64(&diff.proposed) {
            let file_path = std::path::Path::new(&diff.file_path);
            if let Some(config) = Config::load() {
                if let Ok(allowed_base) = config.dev_ops_path.canonicalize() {
                    // Quick workspace boundary check
                    let safe_path = if file_path.exists() {
                        file_path.canonicalize().ok()
                    } else {
                        file_path
                            .parent()
                            .and_then(|p| p.canonicalize().ok())
                            .map(|parent| parent.join(file_path.file_name().unwrap()))
                    };

                    if let Some(ref path) = safe_path {
                        if path.starts_with(&allowed_base) {
                            if std::fs::write(&file_path, content).is_ok() {
                                return Json(json!({ "status": "ok", "message": format!("File diff {} approved and written", payload.task_id) }));
                            }
                        }
                    }
                }
            }
        }
        return Json(json!({ "status": "error", "message": "Failed to apply proposed changes" }));
    }

    Json(json!({ "status": "error", "message": "Task or diff ID not found" }))
}

fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(Into::into)
}

/// WebSocket route handler.
async fn handle_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket_stream(socket, state))
}

/// WebSocket handler loop. Subscribes to the broadcast channel and forwards updates.
async fn websocket_stream(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            // Read from broadcast channel
            broadcast_msg = rx.recv() => {
                match broadcast_msg {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break; // connection dropped
                        }
                    }
                    Err(_) => break,
                }
            }
            // Optional ping/pong handling from socket
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
