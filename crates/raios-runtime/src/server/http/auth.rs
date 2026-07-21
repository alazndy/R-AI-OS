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
    mut req: Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/health" || path == "/api/health" || path == "/.well-known/agent.json" {
        return Ok(next.run(req).await);
    }

    let effective_ip = effective_peer_ip(peer, &headers, trusted_proxy_enabled());
    if effective_ip != peer.ip() {
        eprintln!(
            "[HTTP Auth] Trusted proxy at {peer}: treating request as coming from {effective_ip}"
        );
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

    let actor = if is_localhost {
        crate::control_plane::service::ControlActor::local_session()
    } else {
        // A remote API key proves authentication but is not an ownership grant.
        // Mutating control-plane operations therefore fail closed until an
        // explicit remote-principal provisioning feature exists.
        crate::control_plane::service::ControlActor::remote_session("remote_api_key")
    };
    req.extensions_mut().insert(actor);

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

/// End-to-end tests for `auth_middleware` itself — everything above only
/// covers the pure `effective_peer_ip` helper. `SessionTokenManager` and
/// `PolicyConfig::try_load_default()` both resolve fixed, non-injectable
/// paths (`$HOME/.config/raios/...`, and `./raios-policy.toml` in the
/// current directory before that) — there's no dependency-injection seam,
/// so these tests redirect HOME and the working directory to a tempdir for
/// their duration. That mutates process-global state, which is why every
/// test here takes the same lock: cargo runs tests in one process by
/// default, and two of these racing would read each other's fixtures.
#[cfg(all(test, unix))]
mod middleware_integration_tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware, routing::get, Router};
    use std::sync::Mutex;
    use tower::ServiceExt;

    fn peer(ip: &str) -> SocketAddr {
        format!("{ip}:12345").parse().unwrap()
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct IsolatedEnv {
        _lock: std::sync::MutexGuard<'static, ()>,
        original_cwd: std::path::PathBuf,
        original_home: Option<String>,
        original_xdg_config_home: Option<String>,
        tmp: tempfile::TempDir,
    }

    impl IsolatedEnv {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let tmp = tempfile::TempDir::new().unwrap();
            let original_cwd = std::env::current_dir().unwrap();
            let original_home = std::env::var("HOME").ok();
            let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
            std::env::set_current_dir(tmp.path()).unwrap();
            std::env::set_var("HOME", tmp.path());
            // dirs::config_dir() prefers XDG_CONFIG_HOME over HOME on Linux —
            // must be cleared too, or a real value here would leak the real
            // session token/policy files into these tests.
            std::env::remove_var("XDG_CONFIG_HOME");
            Self {
                _lock: lock,
                original_cwd,
                original_home,
                original_xdg_config_home,
                tmp,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.tmp.path()
        }

        /// Generates a valid session token under this env's isolated HOME
        /// and returns it.
        fn issue_session_token(&self) -> String {
            SessionTokenManager::new().generate_and_save().unwrap()
        }

        /// Writes `./raios-policy.toml` (checked before the config dir) with
        /// the given hub section. `filesystem`/`tools` are required fields
        /// on `PolicyConfig` (no serde default) — a file without them fails
        /// to parse, and `try_load_default()` silently swallows that error
        /// and returns `None`, which looks identical to "no policy file at
        /// all" from the caller's side. Worth remembering: this is the same
        /// class of silent-fallback gap as README's `default_action` bug.
        fn write_policy_with_api_key(&self, plaintext_key: &str) {
            use sha2::{Digest, Sha256};
            let hash = format!("{:x}", Sha256::digest(plaintext_key.as_bytes()));
            std::fs::write(
                self.path().join("raios-policy.toml"),
                format!(
                    "[filesystem]\nenforce_sandbox = false\nallowed_paths = []\nblocked_paths = []\n\n\
                     [tools]\ndefault_action = \"confirm\"\n\n\
                     [server]\n[server.hub]\napi_key_hash = \"{hash}\"\n"
                ),
            )
            .unwrap();
        }
    }

    impl Drop for IsolatedEnv {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original_cwd);
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

    async fn ok_handler() -> StatusCode {
        StatusCode::OK
    }

    fn test_router() -> Router {
        Router::new()
            .route("/api/protected", get(ok_handler))
            .layer(middleware::from_fn(auth_middleware))
    }

    /// Builds a request with `peer` attached as connection info, as axum's
    /// real server does via `into_make_service_with_connect_info`. Without
    /// this, `ConnectInfo<SocketAddr>` extraction fails before the
    /// middleware body even runs.
    fn request_from(peer: SocketAddr) -> Request<Body> {
        let mut req = Request::builder()
            .uri("/api/protected")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(peer));
        req
    }

    fn with_bearer(mut req: Request<Body>, token: &str) -> Request<Body> {
        req.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        req
    }

    fn with_host(mut req: Request<Body>, host: &str) -> Request<Body> {
        req.headers_mut()
            .insert(header::HOST, host.parse().unwrap());
        req
    }

    #[tokio::test]
    async fn missing_bearer_is_unauthorized() {
        let _env = IsolatedEnv::new();
        let req = with_host(request_from(peer("127.0.0.1")), "localhost");
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn loopback_with_wrong_session_token_is_unauthorized() {
        let env = IsolatedEnv::new();
        env.issue_session_token();
        let req = with_bearer(
            with_host(request_from(peer("127.0.0.1")), "localhost"),
            "not-the-real-token",
        );
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn loopback_with_correct_session_token_and_host_is_ok() {
        let env = IsolatedEnv::new();
        let token = env.issue_session_token();
        let req = with_bearer(
            with_host(request_from(peer("127.0.0.1")), "localhost"),
            &token,
        );
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn loopback_with_wrong_host_header_is_rejected_dns_rebinding() {
        let env = IsolatedEnv::new();
        let token = env.issue_session_token();
        let req = with_bearer(
            with_host(request_from(peer("127.0.0.1")), "evil.example.com"),
            &token,
        );
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn loopback_with_missing_host_header_is_rejected() {
        let env = IsolatedEnv::new();
        let token = env.issue_session_token();
        let req = with_bearer(request_from(peer("127.0.0.1")), &token);
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remote_peer_without_api_key_hash_configured_is_unauthorized() {
        let _env = IsolatedEnv::new();
        // No raios-policy.toml written at all — fail-closed, not fail-open.
        let req = with_bearer(request_from(peer("203.0.113.9")), "any-key-at-all");
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn remote_peer_with_correct_api_key_is_ok() {
        let env = IsolatedEnv::new();
        env.write_policy_with_api_key("correct-horse-battery-staple");
        let req = with_bearer(
            request_from(peer("203.0.113.9")),
            "correct-horse-battery-staple",
        );
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn remote_peer_with_wrong_api_key_is_unauthorized() {
        let env = IsolatedEnv::new();
        env.write_policy_with_api_key("correct-horse-battery-staple");
        let req = with_bearer(request_from(peer("203.0.113.9")), "wrong-key");
        let res = test_router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_endpoint_bypasses_auth_entirely() {
        let _env = IsolatedEnv::new();
        let router = Router::new()
            .route("/api/health", get(ok_handler))
            .layer(middleware::from_fn(auth_middleware));
        let mut req = Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(peer("203.0.113.9")));
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "no bearer, no host — should still pass"
        );
    }
}
