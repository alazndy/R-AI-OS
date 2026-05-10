//! MCP (Model Context Protocol) server — stdio transport, JSON-RPC 2.0
//!
//! Launch: `raios mcp-server`
//! Claude Code integration (settings.json):
//!   "mcpServers": { "raios": { "command": "raios", "args": ["mcp-server"] } }

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;

// ─── JSON-RPC types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }
    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message: message.into() }),
        }
    }
}

// ─── Server context ───────────────────────────────────────────────────────────

struct McpServer {
    config: Config,
}

impl McpServer {
    fn new() -> Self {
        let config = Config::load().unwrap_or_else(|| {
            let detected = Config::auto_detect();
            Config {
                dev_ops_path:   detected.dev_ops.unwrap_or_else(|| PathBuf::from(".")),
                master_md_path: detected.master_md.unwrap_or_else(|| PathBuf::from("MASTER.md")),
                skills_path:    detected.skills.unwrap_or_else(|| PathBuf::from(".agents/skills")),
                vault_projects_path: detected.vault_projects.unwrap_or_default(),
            }
        });
        Self { config }
    }

    fn handle(&mut self, req: RpcRequest) -> Option<RpcResponse> {
        let id = req.id.clone().unwrap_or(Value::Null);
        let is_notification = req.id.is_none();

        let result = match req.method.as_str() {
            "initialize"          => self.handle_initialize(&req.params),
            "initialized"         => return None, // notification, no reply
            "ping"                => Ok(json!({})),
            "resources/list"      => self.handle_resources_list(),
            "resources/read"      => self.handle_resources_read(&req.params),
            "tools/list"          => self.handle_tools_list(),
            "tools/call"          => self.handle_tools_call(&req.params),
            other => Err(format!("Unknown method: {}", other)),
        };

        if is_notification {
            return None;
        }

        Some(match result {
            Ok(v) => RpcResponse::ok(id, v),
            Err(msg) => RpcResponse::err(id, -32601, msg),
        })
    }

    fn handle_initialize(&self, _params: &Value) -> Result<Value, String> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "resources": { "subscribe": false, "listChanged": false },
                "tools": {}
            },
            "serverInfo": {
                "name": "raios",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    fn handle_resources_list(&self) -> Result<Value, String> {
        Ok(json!({
            "resources": [
                {
                    "uri": "raios://memory",
                    "name": "Agent Memory",
                    "description": "memory.md — shared agent state and session notes",
                    "mimeType": "text/markdown"
                },
                {
                    "uri": "raios://tasks",
                    "name": "Task List",
                    "description": "tasks.md — active and completed tasks with agent assignments",
                    "mimeType": "text/markdown"
                },
                {
                    "uri": "raios://master",
                    "name": "MASTER Rules",
                    "description": "MASTER.md — agent constitution and mandatory rules",
                    "mimeType": "text/markdown"
                }
            ]
        }))
    }

    fn handle_resources_read(&self, params: &Value) -> Result<Value, String> {
        let uri = params["uri"].as_str().ok_or("missing uri")?;

        let (path, name) = match uri {
            "raios://memory" => {
                // Find the most recent memory.md in dev_ops
                let mem = crate::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
                    .into_iter()
                    .next()
                    .map(|e| e.path)
                    .unwrap_or_else(|| self.config.dev_ops_path.join("memory.md"));
                (mem, "Agent Memory")
            }
            "raios://tasks" => (self.config.dev_ops_path.join("tasks.md"), "Tasks"),
            "raios://master" => (self.config.master_md_path.clone(), "MASTER Rules"),
            _ => return Err(format!("Unknown resource: {}", uri)),
        };

        let content = if path.exists() {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|e| format!("# Error reading file\n{}", e))
        } else {
            format!("# {} not found\nPath: {}", name, path.display())
        };

        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/markdown",
                "text": content
            }]
        }))
    }

    fn handle_tools_list(&self) -> Result<Value, String> {
        Ok(json!({
            "tools": [
                {
                    "name": "update_state",
                    "description": "Update the shared memory.md with agent progress. Call this after completing any significant action.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "agent":   { "type": "string", "description": "Agent name (claude, gemini, antigravity)" },
                            "action":  { "type": "string", "description": "What was done" },
                            "summary": { "type": "string", "description": "Detailed summary to append to memory" }
                        },
                        "required": ["agent", "action", "summary"]
                    }
                },
                {
                    "name": "handover",
                    "description": "Hand off the current task to another agent. Use when you cannot continue or another agent is better suited.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "enum": ["claude", "gemini", "antigravity"],
                                "description": "Target agent name"
                            },
                            "instruction": {
                                "type": "string",
                                "description": "Specific instruction for the target agent"
                            },
                            "context": {
                                "type": "string",
                                "description": "Summary of what has been done so far"
                            }
                        },
                        "required": ["target", "instruction"]
                    }
                },
                {
                    "name": "add_task",
                    "description": "Add a new task to tasks.md",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "text":    { "type": "string", "description": "Task description" },
                            "agent":   { "type": "string", "description": "Assigned agent (optional)" },
                            "project": { "type": "string", "description": "Project name (optional)" }
                        },
                        "required": ["text"]
                    }
                },
                {
                    "name": "get_health",
                    "description": "Get health report for one or all projects (git status, compliance grade, memory.md presence).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project": { "type": "string", "description": "Project name filter (leave empty for all)" }
                        }
                    }
                },
                {
                    "name": "list_projects",
                    "description": "List all known projects from entities.json with their status and category.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "filter": { "type": "string", "description": "Name/category filter (optional)" },
                            "status": { "type": "string", "description": "Status filter: active | archived (optional)" }
                        }
                    }
                },
                {
                    "name": "get_stats",
                    "description": "Get portfolio-wide statistics: total projects, grade distribution, dirty count, local-only count.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "semantic_search",
                    "description": "Semantic (intent-aware) search across the entire Dev Ops workspace. Finds relevant code, docs, and notes by meaning, not just keywords. Uses local vector embeddings — no data leaves the machine.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Natural language search query" },
                            "top_k": { "type": "integer", "description": "Number of results to return (default 8, max 20)" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "ask_architect",
                    "description": "Consult the Architectural Memory. Use this to ask questions about where to put new modules, how to follow project conventions, or previous architectural decisions. Searches MASTER.md rules and memory.md decision logs.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string", "description": "The architectural question (e.g. 'Where should I add a new API endpoint?')" }
                        },
                        "required": ["question"]
                    }
                },
                {
                    "name": "get_validation_errors",
                    "description": "Get latest compilation (cargo check) or compliance errors for a project. Useful for self-healing after a code change.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "project": { "type": "string", "description": "Project name (optional)" }
                        }
                    }
                }
            ]
        }))
    }

    fn handle_tools_call(&mut self, params: &Value) -> Result<Value, String> {
        let name = params["name"].as_str().ok_or("missing tool name")?;
        let args = &params["arguments"];

        match name {
            "update_state"    => self.tool_update_state(args),
            "handover"        => self.tool_handover(args),
            "add_task"        => self.tool_add_task(args),
            "get_health"      => self.tool_get_health(args),
            "list_projects"   => self.tool_list_projects(args),
            "get_stats"       => self.tool_get_stats(),
            "semantic_search" => self.tool_semantic_search(args),
            "ask_architect"   => self.tool_ask_architect(args),
            "get_validation_errors" => self.tool_get_validation_errors(args),
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }

    fn tool_ask_architect(&self, args: &Value) -> Result<Value, String> {
        let question = args["question"].as_str().ok_or("missing question")?;
        
        // Use Cortex for semantic search but specifically mention rules and decisions
        let mut cortex = crate::cortex::Cortex::init().unwrap();
        let _ = cortex.index_file(&self.config.master_md_path);
        
        // Find memory files and index them
        let memory_files = crate::filebrowser::discover_memory_files(&self.config.dev_ops_path, 10);
        for mem in memory_files {
            let _ = cortex.index_file(&mem.path);
        }

        let hits = cortex.search(question, 5).map_err(|e| e.to_string())?;

        let results: Vec<serde_json::Value> = hits.iter().map(|r| {
            json!({
                "source": r.path,
                "rule_or_decision": r.text,
                "score": r.score
            })
        }).collect();

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Architectural Guidance for '{}':\n\n{}", question, serde_json::to_string_pretty(&results).unwrap_or_default())
            }]
        }))
    }

    fn tool_get_validation_errors(&self, args: &Value) -> Result<Value, String> {
        let project_filter = args["project"].as_str().map(|s| s.to_lowercase());

        // Connect to daemon and ask for state
        use std::io::{Read};
        let mut stream = std::net::TcpStream::connect("127.0.0.1:42069")
            .map_err(|e| format!("Could not connect to daemon: {}", e))?;
        
        // Auth
        let token_path = Config::config_file().parent().unwrap().join(".ipc_token");
        if let Ok(token) = std::fs::read_to_string(token_path) {
            let _ = stream.write_all(format!("AUTH {}\n", token.trim()).as_bytes());
        }

        let _ = stream.write_all(b"{\"command\":\"GetState\"}\n");
        
        let mut buffer = [0; 32768];
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        let response = String::from_utf8_lossy(&buffer[..n]);
        
        for line in response.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if v["event"] == "StateSync" {
                    let mut errors = v["latest_errors"].as_array().cloned().unwrap_or_default();
                    
                    if let Some(ref filter) = project_filter {
                        // In a real scenario, ValidationError would have a project field.
                        // For now we filter by file path matching project path or just return all if filter matches name
                        // Let's assume the user wants errors for the project they mentioned.
                        // We can't perfectly filter without project field in ValidationError, 
                        // but we can check if any error file path contains the filter.
                        errors.retain(|e| {
                            e["file"].as_str().map_or(false, |f| f.to_lowercase().contains(filter))
                        });
                    }

                    return Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": if errors.is_empty() { 
                                "No validation errors found.".into() 
                            } else {
                                serde_json::to_string_pretty(&errors).unwrap_or_default()
                            }
                        }]
                    }));
                }
            }
        }

        Err("Failed to retrieve validation errors from daemon".into())
    }

    fn tool_update_state(&self, args: &Value) -> Result<Value, String> {
        let agent   = args["agent"].as_str().unwrap_or("unknown");
        let action  = args["action"].as_str().unwrap_or("");
        let summary = args["summary"].as_str().unwrap_or("");

        // Find memory.md path
        let mem_path = crate::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
            .into_iter()
            .next()
            .map(|e| e.path)
            .unwrap_or_else(|| self.config.dev_ops_path.join("memory.md"));

        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();

        // Append a timestamped entry using safe_io
        let entry = format!(
            "\n<!-- MCP update by {} at {} -->\n- [{}] **{}**: {}\n",
            agent, now, now, action, summary
        );

        let existing = std::fs::read_to_string(&mem_path).unwrap_or_default();
        let new_content = format!("{}{}", existing, entry);

        crate::safe_io::safe_write(&mem_path, &new_content)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Memory updated: {} — {}", action, summary)
            }]
        }))
    }

    fn tool_handover(&self, args: &Value) -> Result<Value, String> {
        let target      = args["target"].as_str().unwrap_or("unknown");
        let instruction = args["instruction"].as_str().unwrap_or("");
        let context     = args["context"].as_str().unwrap_or("(no context)");

        // 1. Log handover to _session_notes.md
        let notes_path = self.config.dev_ops_path.join("_session_notes.md");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let entry = format!(
            "- [{}] HANDOVER → {}: {}\n  Context: {}\n",
            now, target, instruction, context
        );

        let existing = std::fs::read_to_string(&notes_path).unwrap_or_default();
        let _ = crate::safe_io::safe_write(&notes_path, &format!("{}{}", existing, entry));

        // 2. Notify Daemon over TCP
        if let Ok(mut stream) = std::net::TcpStream::connect("127.0.0.1:42069") {
            let msg = json!({
                "command": "Handover",
                "target": target,
                "instruction": instruction,
                "project_path": self.config.dev_ops_path.to_str().unwrap_or("")
            });
            let _ = stream.write_all(format!("{}\n", msg).as_bytes());
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "Handover logged and sent to R-AI-OS Daemon → {}.\nInstruction: {}\nContext saved to _session_notes.md",
                    target, instruction
                )
            }]
        }))
    }

    fn tool_add_task(&self, args: &Value) -> Result<Value, String> {
        let text    = args["text"].as_str().ok_or("missing text")?;
        let agent   = args["agent"].as_str();
        let project = args["project"].as_str();

        let fake_line = match (agent, project) {
            (Some(a), Some(p)) => format!("- [ ] {} @{} #{}", text, a, p),
            (Some(a), None)    => format!("- [ ] {} @{}", text, a),
            (None, Some(p))    => format!("- [ ] {} #{}", text, p),
            (None, None)       => format!("- [ ] {}", text),
        };

        if let Some(task) = crate::tasks::parse_task_line(&fake_line) {
            if let Ok(mut tasks) = crate::tasks::load_tasks(&self.config.dev_ops_path) {
                tasks.push(task);
                let _ = crate::tasks::save_tasks(&self.config.dev_ops_path, &tasks);
            }
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Task added: {}", text)
            }]
        }))
    }

    fn tool_get_health(&self, args: &Value) -> Result<Value, String> {
        let filter = args["project"].as_str().map(str::to_lowercase);
        let projects = crate::entities::load_entities(&self.config.dev_ops_path);

        let reports: Vec<_> = projects.iter()
            .filter(|p| {
                filter.as_ref().map_or(true, |f| {
                    p.name.to_lowercase().contains(f.as_str())
                })
            })
            .map(|p| {
                let h = crate::health::check_project(p);
                json!({
                    "name": h.name,
                    "status": h.status,
                    "git_dirty": h.git_dirty,
                    "compliance_grade": h.compliance_grade,
                    "compliance_score": h.compliance_score,
                    "has_memory": h.has_memory,
                    "remote_url": h.remote_url,
                    "graphify_done": h.graphify_done,
                    "constitution_issues": h.constitution_issues,
                })
            })
            .collect();

        let summary = format!("{} project(s) checked", reports.len());
        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("{}\n\n{}", summary, serde_json::to_string_pretty(&reports).unwrap_or_default())
            }]
        }))
    }

    fn tool_list_projects(&self, args: &Value) -> Result<Value, String> {
        let filter = args["filter"].as_str().map(str::to_lowercase);
        let status_filter = args["status"].as_str().map(str::to_lowercase);
        let projects = crate::entities::load_entities(&self.config.dev_ops_path);

        let list: Vec<_> = projects.iter()
            .filter(|p| {
                let name_ok = filter.as_ref().map_or(true, |f| {
                    p.name.to_lowercase().contains(f.as_str())
                    || p.category.to_lowercase().contains(f.as_str())
                });
                let status_ok = status_filter.as_ref().map_or(true, |s| {
                    p.status.to_lowercase().contains(s.as_str())
                });
                name_ok && status_ok
            })
            .map(|p| json!({
                "name": p.name,
                "category": p.category,
                "status": p.status,
                "github": p.github,
                "local_path": p.local_path.display().to_string(),
                "stars": p.stars,
                "last_commit": p.last_commit,
            }))
            .collect();

        let summary = format!("{} project(s) found", list.len());
        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("{}\n\n{}", summary, serde_json::to_string_pretty(&list).unwrap_or_default())
            }]
        }))
    }

    fn tool_get_stats(&self) -> Result<Value, String> {
        use std::collections::HashMap;
        let projects = crate::entities::load_entities(&self.config.dev_ops_path);
        let total = projects.len();

        let mut active = 0usize;
        let mut archived = 0usize;
        let mut dirty = 0usize;
        let mut no_memory = 0usize;
        let mut local_only = 0usize;
        let mut grade_counts: HashMap<&str, usize> = HashMap::new();

        for p in &projects {
            match p.status.as_str() {
                "archived" | "legacy" => archived += 1,
                _ => active += 1,
            }
            if p.github.is_none() { local_only += 1; }
            if !p.local_path.join("memory.md").exists() { no_memory += 1; }
            if crate::filebrowser::git_is_dirty(&p.local_path) == Some(true) { dirty += 1; }
            let h = crate::health::check_project(p);
            let grade: &'static str = match h.compliance_grade.as_str() {
                "A" => "A", "B" => "B", "C" => "C", _ => "D",
            };
            *grade_counts.entry(grade).or_insert(0) += 1;
        }

        let stats = json!({
            "total": total,
            "active": active,
            "archived": archived,
            "dirty": dirty,
            "no_memory": no_memory,
            "local_only": local_only,
            "grades": {
                "A": grade_counts.get("A").copied().unwrap_or(0),
                "B": grade_counts.get("B").copied().unwrap_or(0),
                "C": grade_counts.get("C").copied().unwrap_or(0),
                "D": grade_counts.get("D").copied().unwrap_or(0),
            }
        });

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&stats).unwrap_or_default()
            }]
        }))
    }

    fn tool_semantic_search(&self, args: &Value) -> Result<Value, String> {
        let query = args["query"].as_str().ok_or("missing query")?;
        let top_k = args["top_k"].as_u64().unwrap_or(8).min(20) as usize;

        // Initialise Cortex (loads stored index from disk)
        let mut cortex = crate::cortex::Cortex::init().unwrap();

        // Incremental index of the workspace
        let _indexed = cortex.index_workspace(&self.config.dev_ops_path)
            .unwrap_or(0);

        // Semantic search
        let vector_hits = cortex.search(query, top_k)
            .map_err(|e| format!("Search failed: {e}"))?;

        // Also run BM25 for hybrid RRF
        let bm25_hits = {
            let idx = crate::indexer::ProjectIndex::build(&self.config.dev_ops_path)
                .map_err(|e| format!("BM25 index build failed: {e}"))?;
            idx.search(query)
        };

        let fused = crate::hybrid_search::fuse(bm25_hits, vector_hits, top_k);

        let results: Vec<serde_json::Value> = fused.iter().map(|r| {
            json!({
                "path": r.path.to_string_lossy(),
                "project": r.project,
                "snippet": r.snippet,
                "line": r.start_line,
                "rrf_score": format!("{:.4}", r.rrf_score),
                "source": r.source.label(),
            })
        }).collect();

        let stats = cortex.chunk_count();
        let summary = format!(
            "Semantic search for '{}' -> {} result(s) (index: {} chunks, {} files)",
            query, results.len(), stats, cortex.file_count()
        );

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("{}\n\n{}", summary, serde_json::to_string_pretty(&results).unwrap_or_default())
            }]
        }))
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

/// Run the MCP server over stdio. Reads newline-delimited JSON-RPC 2.0 from
/// stdin, writes responses to stdout.
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
                writeln!(out, "{}", err_response)?;
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
