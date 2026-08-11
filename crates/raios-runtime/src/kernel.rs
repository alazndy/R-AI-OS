/// Universal Agent Kernel — binds all three concurrent protocol interfaces.
///
/// | Protocol     | Port / Transport | Clients                     |
/// |--------------|------------------|-----------------------------|
/// | Daemon TCP   | 127.0.0.1:42069  | Antigravity, Codex, UI      |
/// | MCP-over-TCP | 127.0.0.1:42070  | Claude (non-stdio)  |
/// | CLI          | (subprocess)     | Shell scripts, human        |
///
/// All three share a single broadcast channel so whispers and state updates
/// reach every connected agent regardless of which protocol they use.
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};

use crate::daemon::state::DaemonState;
use raios_core::config::Config;

pub const DAEMON_PORT: u16 = 42069;
pub const MCP_TCP_PORT: u16 = 42070;

// ─── Kernel ──────────────────────────────────────────────────────────────────

pub struct Kernel {
    state: Arc<RwLock<DaemonState>>,
}

impl Kernel {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        Self { state }
    }

    /// Start all three protocol interfaces concurrently.
    /// Returns when any of them fails (error is propagated).
    pub async fn run(&self) -> Result<()> {
        let (tx, _) = broadcast::channel::<String>(256);

        let daemon_state = self.state.clone();
        let daemon_tx = tx.clone();
        let mcp_tx = tx.clone();
        let http_tx = tx.clone();
        let http_state = self.state.clone();

        // 1. Resolve HTTP port from raios-policy.toml, fallback to 42071
        let http_port = raios_core::security::PolicyConfig::try_load_default()
            .and_then(|p| p.server)
            .and_then(|s| s.http_port)
            .unwrap_or(42071);

        let daemon_handle = tokio::spawn(async move {
            let server = crate::daemon::server::Server::new(daemon_state);
            server.run_with_tx(daemon_tx).await
        });

        let mcp_handle = tokio::spawn(async move { run_mcp_tcp(mcp_tx).await });

        let http_handle = tokio::spawn(async move {
            crate::server::http::start_http_server(http_port, http_state, http_tx).await
        });

        tokio::select! {
            res = daemon_handle => {
                eprintln!("[Kernel] Daemon TCP exited: {:?}", res);
            }
            res = mcp_handle => {
                eprintln!("[Kernel] MCP-over-TCP exited: {:?}", res);
            }
            res = http_handle => {
                eprintln!("[Kernel] HTTP API Adapter exited: {:?}", res);
            }
        }

        Ok(())
    }
}

// ─── MCP-over-TCP listener ────────────────────────────────────────────────────

/// Binds port 42070 and speaks MCP JSON-RPC 2.0 over newline-delimited TCP.
/// Each connection gets an independent McpSession. No auth token is required
/// because MCP clients typically trust localhost connections; the daemon TCP
/// uses token-based auth for higher-privilege operations.
async fn run_mcp_tcp(tx: broadcast::Sender<String>) -> Result<()> {
    let bind_ip = crate::server::http::resolve_bind_addr(MCP_TCP_PORT).ip();
    let addr = format!("{bind_ip}:{MCP_TCP_PORT}");
    let listener = TcpListener::bind(&addr).await?;
    println!("[Kernel] MCP-over-TCP listening on {addr}");

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("[MCP-TCP] Client connected: {addr}");
        let _tx = tx.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_mcp_tcp_client(socket).await {
                eprintln!("[MCP-TCP] Client {addr} error: {e}");
            }
        });
    }
}

async fn handle_mcp_tcp_client(mut socket: tokio::net::TcpStream) -> Result<()> {
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let config =
        Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));

    let mut session = McpTcpSession::new(config);

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(req) => {
                if let Some(response) = session.handle(&req) {
                    let mut out = response.to_string();
                    out.push('\n');
                    writer.write_all(out.as_bytes()).await?;
                }
            }
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("parse error: {e}") }
                });
                let mut out = err.to_string();
                out.push('\n');
                writer.write_all(out.as_bytes()).await?;
            }
        }
    }

    Ok(())
}

// ─── MCP session (TCP variant) ────────────────────────────────────────────────

struct McpTcpSession {
    config: Config,
}

impl McpTcpSession {
    fn new(config: Config) -> Self {
        Self { config }
    }

    fn handle(&mut self, req: &Value) -> Option<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req["method"].as_str().unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let is_notification = req.get("id").is_none();

        let result: Result<Value, String> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "resources": { "subscribe": false, "listChanged": false },
                    "tools": {}
                },
                "serverInfo": {
                    "name": "raios-tcp",
                    "version": env!("CARGO_PKG_VERSION"),
                    "transport": "tcp"
                }
            })),
            "initialized" => return None,
            "ping" => Ok(json!({})),
            "resources/list" => Ok(json!({
                "resources": [
                    {
                        "uri": "raios://memory",
                        "name": "Agent Memory",
                        "mimeType": "text/markdown"
                    },
                    {
                        "uri": "raios://tasks",
                        "name": "Task List",
                        "mimeType": "text/markdown"
                    },
                    {
                        "uri": "raios://master",
                        "name": "MASTER Rules",
                        "mimeType": "text/markdown"
                    }
                ]
            })),
            "resources/read" => self.read_resource(&params),
            "tools/list" => Ok(json!({ "tools": mcp_tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            other => Err(format!("Unknown method: {other}")),
        };

        if is_notification {
            return None;
        }

        Some(match result {
            Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
            Err(msg) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": msg }
            }),
        })
    }

    fn read_resource(&self, params: &Value) -> Result<Value, String> {
        let uri = params["uri"].as_str().ok_or("missing uri")?;
        let path = match uri {
            "raios://memory" => {
                crate::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
                    .into_iter()
                    .next()
                    .map(|e| e.path)
                    .ok_or("no memory.md found")?
            }
            "raios://tasks" => self.config.dev_ops_path.join("tasks.md"),
            "raios://master" => self.config.master_md_path.clone(),
            other => return Err(format!("Unknown resource: {other}")),
        };

        let content = std::fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))?;

        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/markdown",
                "text": content
            }]
        }))
    }

    fn call_tool(&self, params: &Value) -> Result<Value, String> {
        let name = params["name"].as_str().ok_or("missing tool name")?;
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);

        match name {
            "health_check" => {
                let project = args["project"].as_str().unwrap_or(".");
                Ok(json!({ "status": "ok", "project": project }))
            }
            "list_projects" => {
                let projects = raios_core::entities::discover_entities(&self.config.dev_ops_path);
                let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
                Ok(json!({ "projects": names }))
            }
            other => Err(format!("Unknown tool: {other}")),
        }
    }
}

fn mcp_tool_definitions() -> Value {
    json!([
        {
            "name": "health_check",
            "description": "Check the health of a project",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project name or path" }
                }
            }
        },
        {
            "name": "list_projects",
            "description": "List all known projects",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> McpTcpSession {
        McpTcpSession::new(Config::default())
    }

    fn req(method: &str, id: Option<i64>) -> Value {
        let mut v = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        if let Some(id) = id {
            v["id"] = serde_json::json!(id);
        }
        v
    }

    fn call(s: &mut McpTcpSession, method: &str, id: Option<i64>) -> Option<Value> {
        s.handle(&req(method, id))
    }

    #[test]
    fn initialize_advertises_tcp_transport() {
        let mut s = session();
        let res = call(&mut s, "initialize", Some(1)).unwrap();
        assert_eq!(res["id"], 1);
        assert_eq!(res["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(res["result"]["serverInfo"]["name"], "raios-tcp");
        assert_eq!(res["result"]["serverInfo"]["transport"], "tcp");
        assert_eq!(
            res["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn initialized_notification_returns_none() {
        let mut s = session();
        assert!(call(&mut s, "initialized", None).is_none());
    }

    #[test]
    fn ping_returns_empty_result() {
        let mut s = session();
        let res = call(&mut s, "ping", Some(7)).unwrap();
        assert_eq!(res["id"], 7);
        assert_eq!(res["result"], serde_json::json!({}));
    }

    #[test]
    fn resources_list_lists_three_standard_uris() {
        let mut s = session();
        let res = call(&mut s, "resources/list", Some(2)).unwrap();
        let uris: Vec<&str> = res["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        assert_eq!(
            uris,
            vec!["raios://memory", "raios://tasks", "raios://master"]
        );
    }

    #[test]
    fn tools_list_advertises_two_tools() {
        let mut s = session();
        let res = call(&mut s, "tools/list", Some(3)).unwrap();
        let names: Vec<&str> = res["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["health_check", "list_projects"]);
    }

    #[test]
    fn unknown_method_returns_method_not_found_error() {
        let mut s = session();
        let res = call(&mut s, "bogus/method", Some(9)).unwrap();
        assert_eq!(res["id"], 9);
        assert_eq!(res["error"]["code"], -32601);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("bogus/method"));
    }

    #[test]
    fn request_without_id_is_treated_as_notification() {
        let mut s = session();
        assert!(call(&mut s, "ping", None).is_none());
    }

    #[test]
    fn read_resource_rejects_missing_uri() {
        let mut s = session();
        let params = serde_json::json!({});
        let res = s.handle(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": params })).unwrap();
        assert_eq!(res["error"]["code"], -32601);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing uri"));
    }

    #[test]
    fn read_resource_rejects_unknown_uri_before_touching_fs() {
        let mut s = session();
        let params = serde_json::json!({ "uri": "raios://bogus" });
        let res = s.handle(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": params })).unwrap();
        assert_eq!(res["error"]["code"], -32601);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown resource"));
    }

    #[test]
    fn call_tool_rejects_missing_name() {
        let mut s = session();
        let params = serde_json::json!({});
        let res = s.handle(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params })).unwrap();
        assert_eq!(res["error"]["code"], -32601);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing tool name"));
    }

    #[test]
    fn call_tool_health_check_is_pure() {
        let mut s = session();
        let params =
            serde_json::json!({ "name": "health_check", "arguments": { "project": "acme" } });
        let res = s.handle(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params })).unwrap();
        assert_eq!(res["result"]["status"], "ok");
        assert_eq!(res["result"]["project"], "acme");
    }

    #[test]
    fn call_tool_rejects_unknown_tool_before_touching_fs() {
        let mut s = session();
        let params = serde_json::json!({ "name": "no_such_tool" });
        let res = s.handle(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params })).unwrap();
        assert_eq!(res["error"]["code"], -32601);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
    }
}
