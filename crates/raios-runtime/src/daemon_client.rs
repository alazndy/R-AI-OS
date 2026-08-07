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
    let url = format!("{}/api/agents/steer", resolve_base_url());
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
    use super::resolve_base_url;

    #[test]
    fn resolve_base_url_defaults_to_42071() {
        // No raios-policy.toml in the test's cwd → falls back to the
        // documented default port, same default the policy file itself
        // ships (raios-policy.toml:7, `http_port = 42071`).
        assert_eq!(resolve_base_url(), "http://127.0.0.1:42071");
    }
}
