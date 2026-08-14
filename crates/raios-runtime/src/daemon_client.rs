//! Blocking HTTP client for one-shot processes (the CLI binary, the MCP
//! server) that need to reach the running daemon's live `DaemonState` —
//! neither process holds it in-process. Used only for `raios steer` /
//! the `steer_agent` MCP tool today; not a general daemon-RPC framework.

use anyhow::{anyhow, Result};

/// Resolves the daemon's HTTP base URL from the same policy file the daemon
/// itself reads its bind port from — `raios-policy.toml`'s `[server]
/// http_port`, the same field `crate::kernel`'s own daemon startup reads via
/// `PolicyConfig::try_load_default().and_then(|p| p.server).and_then(|s|
/// s.http_port)` — falling back to the documented default.
pub(crate) fn resolve_base_url() -> String {
    let port = raios_core::security::PolicyConfig::try_load_default()
        .and_then(|p| p.server)
        .and_then(|s| s.http_port)
        .unwrap_or(42071);
    format!("http://127.0.0.1:{port}")
}

/// Reads the local session token the daemon's own HTTP `auth_middleware`
/// validates loopback requests against (`server/http/auth.rs` →
/// `SessionTokenManager::validate_token`). `/api/agents/steer` is **not** one
/// of the three auth-bypassed paths (`/health`, `/api/health`,
/// `/.well-known/agent.json`), so every steer request must carry this as
/// `Authorization: Bearer <token>` or the daemon answers 401 before the route
/// ever runs.
fn session_bearer_token() -> Result<String> {
    raios_core::security::SessionTokenManager::new()
        .get_valid_token()
        .map_err(|e| {
            anyhow!(
                "could not read the raios daemon session token ({e}) — \
                 is the daemon running? (`raios hub status`)"
            )
        })
}

/// Calls the daemon's `POST /api/agents/steer` route. Returns `Err` with the
/// daemon's own error message on any non-2xx response or transport failure —
/// callers (CLI, MCP) surface this string directly rather than wrapping it.
pub fn steer_agent_via_http(agent_id: &str, message: &str, sender: &str) -> Result<()> {
    let token = session_bearer_token()?;
    steer_agent_at(&resolve_base_url(), &token, agent_id, message, sender)
}

/// Does the actual request/response work for [`steer_agent_via_http`], with
/// the daemon base URL and the bearer token passed in explicitly rather than
/// resolved internally. Split out so tests can point it at a local mock
/// listener (with a known token to assert on) instead of a real running
/// daemon on the policy-resolved port, without mutating process-global `HOME`
/// to fake a session-token file. Public to allow cross-crate testing in
/// raios-surface-mcp.
pub fn steer_agent_at(
    base_url: &str,
    token: &str,
    agent_id: &str,
    message: &str,
    sender: &str,
) -> Result<()> {
    let url = format!("{base_url}/api/agents/steer");
    let body = serde_json::json!({
        "agent_id": agent_id,
        "message": message,
        "sender": sender,
    });

    let resp = ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(body)
        .map_err(|e| anyhow!("could not reach raios daemon at {url}: {e}"))?;

    let parsed: serde_json::Value = resp
        .into_json()
        .map_err(|e| anyhow!("daemon returned an unparseable response: {e}"))?;

    match parsed.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(()),
        _ => Err(anyhow!(
            "steer failed: {}",
            parsed
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown daemon error")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_base_url, steer_agent_at};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn resolve_base_url_defaults_to_42071() {
        // No raios-policy.toml in the test's cwd → falls back to the
        // documented default port, same default the policy file itself
        // ships (raios-policy.toml:7, `http_port = 42071`).
        assert_eq!(resolve_base_url(), "http://127.0.0.1:42071");
    }

    /// Binds an ephemeral local listener, accepts exactly one connection,
    /// **reads and returns the full request** (request line + headers +
    /// body), and writes back `body` as a `200 OK application/json`
    /// response. Returns the listener's `http://127.0.0.1:<port>` base URL
    /// and the accept-thread's join handle, whose value is the captured
    /// request text.
    ///
    /// Capturing the request rather than discarding it is deliberate: the
    /// earlier version of this helper read into a scratch buffer and threw it
    /// away, which made every assertion here blind to what was actually sent.
    /// That is precisely how the missing `Authorization` header (the daemon's
    /// `auth_middleware` 401s without it) shipped green — the mock answered
    /// `200 OK` unconditionally.
    fn spawn_mock_daemon(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("read bound addr");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock request");
            let request = read_http_request(&mut stream);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write mock response");
            stream.flush().expect("flush mock response");
            request
        });

        (format!("http://{addr}"), handle)
    }

    /// Reads one HTTP/1.1 request off `stream`: headers first, then exactly
    /// `Content-Length` more bytes so the client's write completes cleanly.
    /// Deliberately minimal — enough to assert on headers and body, not a
    /// general-purpose HTTP parser.
    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut raw = Vec::new();
        let mut byte = [0u8; 1];
        // Header section: read until the CRLFCRLF terminator.
        while !raw.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&raw).into_owned(),
                Ok(_) => raw.push(byte[0]),
            }
        }

        let headers = String::from_utf8_lossy(&raw).into_owned();
        let content_length = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);

        let mut body = vec![0u8; content_length];
        if content_length > 0 && stream.read_exact(&mut body).is_err() {
            body.clear();
        }
        format!("{headers}{}", String::from_utf8_lossy(&body))
    }

    /// True when `request` carries exactly `Authorization: Bearer <token>`.
    /// Header names are matched case-insensitively (HTTP/1.1 §3.2), the value
    /// exactly.
    fn has_bearer_token(request: &str, token: &str) -> bool {
        request.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case("authorization")
                    && value.trim() == format!("Bearer {token}")
            })
        })
    }

    #[test]
    fn steer_agent_at_returns_ok_when_daemon_reports_status_ok() {
        let (base_url, handle) = spawn_mock_daemon(r#"{"status":"ok"}"#);

        let result = steer_agent_at(
            &base_url,
            "test-session-token",
            "agent-1",
            "hello",
            "claude_kaira",
        );

        let request = handle.join().expect("mock server thread panicked");
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert!(
            has_bearer_token(&request, "test-session-token"),
            "steer request must carry the session bearer token the daemon's \
             auth_middleware validates, got request:\n{request}"
        );
        assert!(
            request.contains(r#""agent_id":"agent-1""#),
            "expected the agent id in the JSON body, got:\n{request}"
        );
    }

    #[test]
    fn steer_agent_at_returns_err_with_daemon_message_when_status_error() {
        let (base_url, handle) =
            spawn_mock_daemon(r#"{"status":"error","message":"steer target not found"}"#);

        let result = steer_agent_at(
            &base_url,
            "test-session-token",
            "agent-1",
            "hello",
            "claude_kaira",
        );

        handle.join().expect("mock server thread panicked");
        let err = result.expect_err("expected Err on status:error response");
        assert!(
            err.to_string().contains("steer target not found"),
            "expected error to surface the daemon's message, got: {err}"
        );
    }

    /// Regression guard for the 401 bug: the whole point of the header is
    /// that a *wrong* token is distinguishable from the right one at the
    /// mock, so a future change that drops or mangles the header fails here
    /// instead of silently passing.
    #[test]
    fn steer_agent_at_sends_the_exact_token_it_was_given() {
        let (base_url, handle) = spawn_mock_daemon(r#"{"status":"ok"}"#);

        let result = steer_agent_at(&base_url, "token-A", "agent-1", "hello", "claude_kaira");

        let request = handle.join().expect("mock server thread panicked");
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert!(has_bearer_token(&request, "token-A"));
        assert!(!has_bearer_token(&request, "token-B"));
    }
}
