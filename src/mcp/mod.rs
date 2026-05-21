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
use std::path::PathBuf;

use crate::config::Config;

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
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }
    pub(super) fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(RpcError { code, message: message.into() }) }
    }
}

// ─── Server context ───────────────────────────────────────────────────────────

pub(super) struct McpServer {
    pub(super) config: Config,
}

impl McpServer {
    fn new() -> Self {
        let config = Config::load().unwrap_or_else(|| {
            let detected = Config::auto_detect();
            Config {
                dev_ops_path: detected.dev_ops.unwrap_or_else(|| PathBuf::from(".")),
                master_md_path: detected.master_md.unwrap_or_else(|| PathBuf::from("MASTER.md")),
                skills_path: detected.skills.unwrap_or_else(|| PathBuf::from(".agents/skills")),
                vault_projects_path: detected.vault_projects.unwrap_or_default(),
            }
        });
        Self { config }
    }

    fn handle(&mut self, req: RpcRequest) -> Option<RpcResponse> {
        let id = req.id.clone().unwrap_or(Value::Null);
        let is_notification = req.id.is_none();

        let result = match req.method.as_str() {
            "initialize"  => self.handle_initialize(&req.params),
            "initialized" => return None,
            "ping"        => Ok(json!({})),
            "resources/list" => self.handle_resources_list(),
            "resources/read" => self.handle_resources_read(&req.params),
            "tools/list"  => self.handle_tools_list(),
            "tools/call"  => self.handle_tools_call(&req.params),
            other => Err(format!("Unknown method: {}", other)),
        };

        if is_notification { return None; }
        Some(match result {
            Ok(v)    => RpcResponse::ok(id, v),
            Err(msg) => RpcResponse::err(id, -32601, msg),
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
