use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
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

fn resolve_pending_diff_target(
    file_path: &std::path::Path,
    allowed_base: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let resolved = if file_path.exists() {
        file_path.canonicalize().ok()
    } else {
        let file_name = file_path.file_name()?;
        file_path
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|parent| parent.join(file_name))
    }?;

    resolved.starts_with(allowed_base).then_some(resolved)
}

/// Resolve the bind address for the HTTP server based on hub policy.
pub fn resolve_bind_addr(port: u16) -> SocketAddr {
    let hub = crate::security::PolicyConfig::try_load_default()
        .and_then(|p| p.server)
        .and_then(|s| s.hub);

    let mode = hub
        .as_ref()
        .map(|h| h.bind_mode.as_str())
        .unwrap_or("localhost");

    let ip: std::net::IpAddr = match mode {
        "all" => "0.0.0.0".parse().unwrap(),
        "tailscale" => detect_tailscale_ip().unwrap_or_else(|| {
            eprintln!("[Kernel] Tailscale IP not found, falling back to localhost");
            "127.0.0.1".parse().unwrap()
        }),
        _ => "127.0.0.1".parse().unwrap(),
    };

    SocketAddr::new(ip, port)
}

/// Call `tailscale ip --1` and parse the result.
pub fn detect_tailscale_ip() -> Option<std::net::IpAddr> {
    let out = std::process::Command::new("tailscale")
        .args(["ip", "--1"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .ok()
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
        .route("/api/inbox", get(handle_inbox))
        .route("/api/usage", get(handle_usage))
        .route("/api/plans", get(handle_plans))
        .route("/api/approve", post(handle_approve))
        .route("/api/git-status", get(handle_git_status))
        .route("/api/swarm", get(handle_swarm))
        .route("/api/stream", get(handle_websocket))
        .layer(axum::middleware::from_fn(auth_middleware))
        .with_state(app_state);

    let addr = resolve_bind_addr(port);
    println!("[Kernel] HTTP API Adapter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// HTTP middleware — dual-path auth:
///   localhost (127.0.0.1)  → ephemeral session token (existing behaviour)
///   Tailscale / remote     → persistent API key (SHA-256 hashed in policy.toml)
async fn auth_middleware(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    // 1. Determine if request is local from the actual TCP peer address
    let is_localhost = peer.ip().is_loopback();

    // 2. DNS rebinding guard — only for localhost connections
    if is_localhost {
        if let Some(host) = headers.get(header::HOST) {
            let host_str = host.to_str().unwrap_or("").to_lowercase();
            if !host_str.starts_with("localhost") && !host_str.starts_with("127.0.0.1") {
                eprintln!("[HTTP Auth] DNS rebinding attempt from {peer}: {host_str}");
                return Err(StatusCode::BAD_REQUEST);
            }
        } else {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // 3. Extract bearer token
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_owned);

    let Some(token) = bearer else {
        eprintln!("[HTTP Auth] Missing Bearer token");
        return Err(StatusCode::UNAUTHORIZED);
    };

    // 4. Route to the appropriate validator
    if is_localhost {
        let mgr = SessionTokenManager::new();
        if !mgr.validate_token(&token) {
            eprintln!("[HTTP Auth] Invalid session token (localhost)");
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else {
        if !validate_api_key(&token) {
            eprintln!("[HTTP Auth] Invalid API key (remote)");
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    Ok(next.run(req).await)
}

/// Validate a remote API key by comparing its SHA-256 hash against policy.toml.
fn validate_api_key(provided: &str) -> bool {
    use sha2::{Digest, Sha256};

    let stored_hash = crate::security::PolicyConfig::try_load_default()
        .and_then(|p| p.server)
        .and_then(|s| s.hub)
        .and_then(|h| h.api_key_hash);

    let Some(stored) = stored_hash else {
        eprintln!("[HTTP Auth] No api_key_hash configured in raios-policy.toml [server.hub]");
        return false;
    };

    let mut hasher = Sha256::new();
    hasher.update(provided.as_bytes());
    let computed = format!("{:x}", hasher.finalize());

    // Constant-time comparison
    computed.len() == stored.len()
        && computed
            .bytes()
            .zip(stored.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
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
    let config =
        Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));

    match crate::tasks::load_tasks(&config.dev_ops_path) {
        Ok(tasks) => Json(json!({ "status": "ok", "tasks": tasks })),
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

/// GET /api/inbox
/// Unified operational view: active tasks, pending approvals, in-progress runs.
/// All data sourced from canonical cp_* tables.
async fn handle_inbox() -> impl IntoResponse {
    match crate::db::open_db() {
        Ok(conn) => {
            let tasks = crate::db::cp_query_active_tasks(&conn).unwrap_or_default();
            let approvals = crate::db::cp_query_pending_approvals(&conn).unwrap_or_default();
            let runs = crate::db::cp_query_active_runs(&conn).unwrap_or_default();
            let blocked = crate::db::cp_query_blocked_tasks(&conn).unwrap_or_default();
            Json(json!({
                "status": "ok",
                "active_tasks": tasks,
                "pending_approvals": approvals,
                "active_runs": runs,
                "blocked_tasks": blocked,
            }))
        }
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

/// GET /api/usage
/// Returns local usage/quota signals for supported AI tools.
async fn handle_usage() -> impl IntoResponse {
    let report = crate::system_scan::scan_system();
    Json(json!({ "status": "ok", "usage": report.usage }))
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
                return Json(
                    json!({ "status": "ok", "message": format!("Swarm task {} approved and merged", payload.task_id) }),
                );
            }
            Err(e) => {
                return Json(json!({ "status": "error", "message": e.to_string() }));
            }
        }
    }

    // 2. Approve Diff if ID matches
    let mut s = state.daemon_state.write().await;
    if let Some(pos) = s.pending_diffs.iter().position(|d| d.id == payload.task_id) {
        let Some(diff) = s.pending_diffs.remove(pos) else {
            return Json(
                json!({ "status": "error", "message": "Pending diff disappeared before approval" }),
            );
        };
        drop(s);

        if let Ok(content) = decode_base64(&diff.proposed) {
            let file_path = std::path::Path::new(&diff.file_path);
            if let Some(config) = Config::load() {
                if let Ok(allowed_base) = config.dev_ops_path.canonicalize() {
                    if resolve_pending_diff_target(file_path, &allowed_base).is_some()
                        && std::fs::write(file_path, content).is_ok()
                    {
                        return Json(
                            json!({ "status": "ok", "message": format!("File diff {} approved and written", payload.task_id) }),
                        );
                    }
                }
            }
        }
        return Json(json!({ "status": "error", "message": "Failed to apply proposed changes" }));
    }

    Json(json!({ "status": "error", "message": "Task or diff ID not found" }))
}

/// GET /api/plans
/// Reads docs/superpowers/plans/*.md and returns title + checkbox status.
async fn handle_plans() -> impl IntoResponse {
    let plans_dir = locate_plans_dir();
    let plans = match plans_dir {
        Some(dir) => scan_plans(&dir),
        None => vec![],
    };
    Json(json!({ "plans": plans }))
}

fn locate_plans_dir() -> Option<std::path::PathBuf> {
    let suffix = std::path::Path::new("docs")
        .join("superpowers")
        .join("plans");

    // Try binary parent → project root (works for target/debug and target/release)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(target) = exe.parent().and_then(|p| p.parent()) {
            let candidate = target.parent().unwrap_or(target).join(&suffix);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }

    // Fallback: current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(&suffix);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::resolve_pending_diff_target;
    use tempfile::TempDir;

    #[test]
    fn resolve_pending_diff_target_accepts_existing_workspace_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();

        let resolved = resolve_pending_diff_target(&file, tmp.path()).unwrap();
        assert_eq!(resolved, file.canonicalize().unwrap());
    }

    #[test]
    fn resolve_pending_diff_target_rejects_path_without_filename() {
        let tmp = TempDir::new().unwrap();
        let resolved = resolve_pending_diff_target(std::path::Path::new(""), tmp.path());
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_pending_diff_target_rejects_outside_workspace() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let file = outside.path().join("other.rs");
        std::fs::write(&file, "").unwrap();

        let resolved = resolve_pending_diff_target(&file, tmp.path());
        assert!(resolved.is_none());
    }
}

fn scan_plans(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return vec![],
    };
    entries.sort_by_key(|e| e.file_name());

    entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                return None;
            }
            let slug = path.file_stem()?.to_string_lossy().to_string();
            let content = std::fs::read_to_string(&path).unwrap_or_default();

            let title = content
                .lines()
                .find(|l| l.starts_with("# "))
                .map(|l| l.trim_start_matches("# ").to_string())
                .unwrap_or_else(|| slug.clone());

            let checked = content.matches("- [x]").count() + content.matches("- [X]").count();
            let unchecked = content.matches("- [ ]").count();
            let total = checked + unchecked;
            let pct: u8 = checked
                .checked_mul(100)
                .and_then(|v| v.checked_div(total))
                .map(|v| v.min(100) as u8)
                .unwrap_or(0);

            let status = match (checked, unchecked) {
                (0, 0) => "no_tasks",
                (0, _) => "not_started",
                (_, 0) => "done",
                _ => "in_progress",
            };

            let date = slug.get(..10).unwrap_or("").to_string();

            Some(json!({
                "slug": slug,
                "title": title,
                "date": date,
                "status": status,
                "checked": checked,
                "total": total,
                "pct": pct,
            }))
        })
        .collect()
}

#[derive(Deserialize)]
struct PathQuery {
    path: Option<String>,
}

/// GET /api/git-status?path=<workspace_path>
async fn handle_git_status(Query(params): Query<PathQuery>) -> impl IntoResponse {
    let path = params
        .path
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".to_string());

    let out = std::process::Command::new("git")
        .args(["-C", &path, "status", "--porcelain=v1", "-b"])
        .output();

    match out {
        Err(_) => Json(json!({ "error": "git not available" })),
        Ok(output) if !output.status.success() && output.stdout.is_empty() => {
            Json(json!({ "error": "not a git repo" }))
        }
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut branch = "unknown".to_string();
            let mut staged: u32 = 0;
            let mut modified: u32 = 0;
            let mut untracked: u32 = 0;

            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("## ") {
                    branch = rest.split("...").next().unwrap_or(rest).to_string();
                } else if line.len() >= 2 {
                    let bytes = line.as_bytes();
                    let x = bytes[0] as char;
                    let y = bytes[1] as char;
                    if x == '?' && y == '?' {
                        untracked += 1;
                    } else {
                        if x != ' ' {
                            staged += 1;
                        }
                        if y != ' ' {
                            modified += 1;
                        }
                    }
                }
            }

            let dirty = staged + modified + untracked > 0;
            Json(json!({
                "branch": branch,
                "dirty": dirty,
                "staged": staged,
                "modified": modified,
                "untracked": untracked,
            }))
        }
    }
}

/// GET /api/swarm
/// Lists active (non-terminal) swarm tasks.
async fn handle_swarm() -> impl IntoResponse {
    let store =
        crate::swarm::store::SwarmStore::new(crate::swarm::store::SwarmStore::default_path());
    let tasks: Vec<_> = store
        .list_active()
        .iter()
        .map(|t| {
            let status = match &t.status {
                crate::swarm::SwarmStatus::Initializing => "initializing",
                crate::swarm::SwarmStatus::Running => "running",
                crate::swarm::SwarmStatus::AwaitingReview => "awaiting_review",
                crate::swarm::SwarmStatus::Merged => "merged",
                crate::swarm::SwarmStatus::Rejected => "rejected",
                crate::swarm::SwarmStatus::Failed(_) => "failed",
            };
            json!({
                "id": t.id.to_string(),
                "project": t.project_name,
                "description": t.task_description,
                "agent": t.agent,
                "status": status,
                "created_at": t.created_at,
            })
        })
        .collect();
    Json(json!({ "tasks": tasks }))
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
