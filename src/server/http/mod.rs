use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::daemon::state::DaemonState;

mod a2a;
mod auth;
mod plans;
mod routes;
mod websocket;

use a2a::{handle_a2a, handle_agent_card};
use auth::auth_middleware;
use routes::{
    handle_approve, handle_git_status, handle_health, handle_inbox, handle_plans, handle_projects,
    handle_swarm, handle_tasks, handle_usage,
};
use websocket::handle_websocket;

#[derive(Clone)]
struct AppState {
    daemon_state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
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

    let app = Router::new()
        .route("/health", get(handle_health))
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
        .route("/.well-known/agent.json", get(handle_agent_card))
        .route("/a2a", post(handle_a2a))
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
