//! MCP (Model Context Protocol) server — stdio transport, JSON-RPC 2.0
//!
//! Launch: `raios mcp-server`
//! Claude Code integration (settings.json):
//!   "mcpServers": { "raios": { "command": "raios", "args": ["mcp-server"] } }

mod resources;
mod tools;
mod tools_dev;
mod tools_git;
mod tools_swarm;
mod tools_workspace;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use raios_core::config::Config;
use raios_core::security::quarantine::{self, QuarantineStore};
use raios_core::security::rate_limiter::RateLimiter;
use raios_core::security::tool_pin;
use raios_core::security::{EgressFilter, Umai};

// ─── JSON-RPC types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    pub(super) id: Option<Value>,
    pub(super) method: String,
    #[serde(default)]
    pub(super) params: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub(super) struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    pub(super) fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    pub(super) fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ─── Server context ───────────────────────────────────────────────────────────

pub(super) struct McpServer {
    pub(super) config: Config,
    rate_limiter: RateLimiter,
    quarantine: QuarantineStore,
    pin_broken: bool,
    umai: Umai,
    /// Domain allowlist/blocklist for outbound network calls made by tools,
    /// enforced in `handle_tools_call` against each tool's declared network
    /// capability (see `security::capabilities`).
    egress: EgressFilter,
    /// Explicitly blocked filesystem path prefixes (e.g. `~/.ssh`) from
    /// `[filesystem] blocked_paths`, enforced against every path-resolving
    /// tool's declared filesystem capability.
    blocked_paths: Vec<String>,
}

impl McpServer {
    fn new() -> Self {
        let config =
            Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));
        let policy = raios_core::security::PolicyConfig::try_load_default();
        let umai = Umai::new(policy.clone());
        let rate_limiter = policy
            .as_ref()
            .map(|p| RateLimiter::from_policy(p.rate_limits.clone()))
            .unwrap_or_else(RateLimiter::disabled);
        let egress = policy
            .as_ref()
            .map(EgressFilter::from_policy)
            .unwrap_or_else(EgressFilter::disabled);
        let blocked_paths = policy
            .as_ref()
            .map(|p| p.filesystem.blocked_paths.clone())
            .unwrap_or_default();
        let quarantine = QuarantineStore::from_policy(policy.and_then(|p| p.quarantine));

        if quarantine.is_enabled() {
            if let Ok(conn) = raios_core::db::open_db() {
                let _ = quarantine::ensure_table(&conn);
            }
        }

        let manifest_json =
            serde_json::to_string(&Self::static_tools_manifest()).unwrap_or_default();
        let pin_broken = match tool_pin::verify_or_pin(&manifest_json) {
            Ok(fresh) => {
                if fresh {
                    eprintln!("[raios] Tool manifest pinned (first run).");
                }
                false
            }
            Err(e) => {
                eprintln!("[raios] SECURITY WARNING: {e}");
                true
            }
        };

        Self {
            config,
            rate_limiter,
            quarantine,
            pin_broken,
            umai,
            egress,
            blocked_paths,
        }
    }

    fn static_tools_manifest() -> serde_json::Value {
        json!({
            "tools": [
                "update_state","handover","add_task","get_health","get_inbox","list_projects",
                "get_stats","semantic_search","project_info","portfolio_status",
                "disk_usage","list_ports","usage_status","version_info","version_bump","env_status",
                "deps_status","run_build","run_tests","git_status","git_log","git_diff",
                "git_commit","ask_architect","get_validation_errors","session_note",
                "create_swarm_task","list_swarm_tasks","approve_swarm_task",
                "route_capability","list_evolution_candidates","promote_evolution_candidate",
                "get_agent_stats"
            ]
        })
    }

    fn handle(&mut self, req: RpcRequest) -> Option<RpcResponse> {
        let id = req.id.clone().unwrap_or(Value::Null);
        let is_notification = req.id.is_none();

        let result = match req.method.as_str() {
            "initialize" => self.handle_initialize(&req.params),
            "initialized" => return None,
            "ping" => Ok(json!({})),
            "resources/list" => self.handle_resources_list(),
            "resources/read" => self.handle_resources_read(&req.params),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&req.params),
            other => Err(format!("method_not_found:{}", other)),
        };

        if is_notification {
            return None;
        }
        Some(match result {
            Ok(v) => RpcResponse::ok(id, v),
            Err(msg) if msg.starts_with("capability:") => RpcResponse::err(id, -32024, msg),
            Err(msg) if msg.starts_with("umai:") => RpcResponse::err(id, -32026, msg),
            Err(msg) if msg.starts_with("umai_confirm:") => RpcResponse::err(id, -32025, msg),
            Err(msg) if msg.starts_with("rate_limit:") => RpcResponse::err(id, -32029, msg),
            Err(msg) if msg.starts_with("tool_pin:") => RpcResponse::err(id, -32028, msg),
            Err(msg) if msg.starts_with("quarantine:") => RpcResponse::err(id, -32027, msg),
            Err(msg) if msg.starts_with("method_not_found:") => RpcResponse::err(id, -32601, msg),
            Err(msg) => RpcResponse::err(id, -32603, msg),
        })
    }

    pub(super) fn handle_initialize(&self, _params: &Value) -> Result<Value, String> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "resources": { "subscribe": false, "listChanged": false },
                "tools": {}
            },
            "serverInfo": { "name": "raios", "version": env!("CARGO_PKG_VERSION") }
        }))
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut server = McpServer::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if l.is_empty() => continue,
            Ok(l) => l,
            Err(_) => break,
        };

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                let json = serde_json::to_string(&err_response)?;
                writeln!(out, "{}", json)?;
                out.flush()?;
                continue;
            }
        };

        if let Some(response) = server.handle(req) {
            let json = serde_json::to_string(&response)?;
            writeln!(out, "{}", json)?;
            out.flush()?;
        }
    }
    Ok(())
}
