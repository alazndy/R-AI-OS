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

/// Calls the daemon's `POST /api/agents/steer` route. Returns `Err` with the
/// daemon's own error message on any non-2xx response or transport failure —
/// callers (CLI, MCP) surface this string directly rather than wrapping it.
pub fn steer_agent_via_http(agent_id: &str, message: &str, sender: &str) -> Result<()> {
    steer_agent_at(&resolve_base_url(), agent_id, message, sender)
}

/// Does the actual request/response work for [`steer_agent_via_http`], with
/// the daemon base URL passed in explicitly rather than resolved internally.
/// Split out so tests can point it at a local mock listener instead of a
/// real running daemon on the policy-resolved port.
fn steer_agent_at(base_url: &str, agent_id: &str, message: &str, sender: &str) -> Result<()> {
    let url = format!("{base_url}/api/agents/steer");
    let body = serde_json::json!({
        "agent_id": agent_id,
        "message": message,
        "sender": sender,
    });

    let resp = ureq::post(&url)
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
    /// drains the request, and writes back `body` as a
    /// `200 OK application/json` response. Returns the listener's
    /// `http://127.0.0.1:<port>` base URL and the accept-thread's join
    /// handle so the test can wait for the exchange to finish before
    /// asserting.
    fn spawn_mock_daemon(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("read bound addr");

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock request");

            // Drain the request so the client's write completes cleanly;
            // the mock doesn't need to parse it, only respond.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write mock response");
            stream.flush().expect("flush mock response");
        });

        (format!("http://{addr}"), handle)
    }

    #[test]
    fn steer_agent_at_returns_ok_when_daemon_reports_status_ok() {
        let (base_url, handle) = spawn_mock_daemon(r#"{"status":"ok"}"#);

        let result = steer_agent_at(&base_url, "agent-1", "hello", "claude_kaira");

        handle.join().expect("mock server thread panicked");
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn steer_agent_at_returns_err_with_daemon_message_when_status_error() {
        let (base_url, handle) =
            spawn_mock_daemon(r#"{"status":"error","message":"steer target not found"}"#);

        let result = steer_agent_at(&base_url, "agent-1", "hello", "claude_kaira");

        handle.join().expect("mock server thread panicked");
        let err = result.expect_err("expected Err on status:error response");
        assert!(
            err.to_string().contains("steer target not found"),
            "expected error to surface the daemon's message, got: {err}"
        );
    }
}
