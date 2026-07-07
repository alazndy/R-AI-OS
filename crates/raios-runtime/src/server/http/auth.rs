use axum::{
    extract::{connect_info::ConnectInfo, Request},
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use std::net::{IpAddr, SocketAddr};

use raios_core::security::SessionTokenManager;

/// Resolves the IP the auth decision should trust.
///
/// Normally that's the raw TCP peer. If `[server.hub] trusted_proxy = true`
/// in policy, and the raw TCP peer is itself loopback (i.e. a same-host
/// reverse proxy just forwarded this request), the leftmost address in
/// `X-Forwarded-For` is trusted instead. A remote attacker can't spoof this:
/// forging the header only matters if their own connection first passes the
/// loopback check, which requires already running on the same host.
fn effective_peer_ip(peer: SocketAddr, headers: &HeaderMap, trusted_proxy: bool) -> IpAddr {
    if trusted_proxy && peer.ip().is_loopback() {
        if let Some(forwarded_ip) = headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|first| first.trim().parse::<IpAddr>().ok())
        {
            return forwarded_ip;
        }
    }
    peer.ip()
}

fn trusted_proxy_enabled() -> bool {
    raios_core::security::PolicyConfig::try_load_default()
        .and_then(|p| p.server)
        .and_then(|s| s.hub)
        .map(|h| h.trusted_proxy)
        .unwrap_or(false)
}

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

    let effective_ip = effective_peer_ip(peer, &headers, trusted_proxy_enabled());
    if effective_ip != peer.ip() {
        eprintln!("[HTTP Auth] Trusted proxy at {peer}: treating request as coming from {effective_ip}");
    }
    let is_localhost = effective_ip.is_loopback();

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

    let stored_hash = raios_core::security::PolicyConfig::try_load_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(ip: &str) -> SocketAddr {
        format!("{ip}:12345").parse().unwrap()
    }

    fn headers_with_xff(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", value.parse().unwrap());
        h
    }

    #[test]
    fn ignores_xff_when_trusted_proxy_disabled() {
        let ip = effective_peer_ip(peer("127.0.0.1"), &headers_with_xff("8.8.8.8"), false);
        assert!(ip.is_loopback());
    }

    #[test]
    fn ignores_xff_when_direct_peer_is_not_loopback() {
        // A remote attacker can't spoof trust by forging X-Forwarded-For —
        // it's only honored when the direct TCP connection is loopback.
        let ip = effective_peer_ip(peer("203.0.113.9"), &headers_with_xff("127.0.0.1"), true);
        assert_eq!(ip.to_string(), "203.0.113.9");
    }

    #[test]
    fn trusts_leftmost_xff_ip_when_proxy_is_loopback_and_enabled() {
        let ip = effective_peer_ip(
            peer("127.0.0.1"),
            &headers_with_xff("203.0.113.9, 10.0.0.1"),
            true,
        );
        assert_eq!(ip.to_string(), "203.0.113.9");
    }

    #[test]
    fn falls_back_to_peer_when_xff_missing_even_if_trusted() {
        let ip = effective_peer_ip(peer("127.0.0.1"), &HeaderMap::new(), true);
        assert!(ip.is_loopback());
    }

    #[test]
    fn falls_back_to_peer_when_xff_is_unparseable() {
        let ip = effective_peer_ip(peer("127.0.0.1"), &headers_with_xff("not-an-ip"), true);
        assert!(ip.is_loopback());
    }
}
