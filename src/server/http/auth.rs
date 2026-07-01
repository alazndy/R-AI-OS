use axum::{
    extract::{connect_info::ConnectInfo, Request},
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use std::net::SocketAddr;

use crate::security::SessionTokenManager;

pub(super) async fn auth_middleware(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    req: Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/health" || path == "/api/health" || path == "/.well-known/agent.json" {
        return Ok(next.run(req).await);
    }

    let is_localhost = peer.ip().is_loopback();

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

    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_owned);

    let Some(token) = bearer else {
        eprintln!("[HTTP Auth] Missing Bearer token");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if is_localhost {
        let mgr = SessionTokenManager::new();
        if !mgr.validate_token(&token) {
            eprintln!("[HTTP Auth] Invalid session token (localhost)");
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else if !validate_api_key(&token) {
        eprintln!("[HTTP Auth] Invalid API key (remote)");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

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

    computed.len() == stored.len()
        && computed
            .bytes()
            .zip(stored.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}
