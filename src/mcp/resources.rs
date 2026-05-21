use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn handle_resources_list(&self) -> Result<Value, String> {
        Ok(json!({
            "resources": [
                { "uri": "raios://memory",          "name": "Agent Memory",      "description": "memory.md — shared agent state and session notes",            "mimeType": "text/markdown"      },
                { "uri": "raios://tasks",            "name": "Task List",         "description": "tasks.md — active and completed tasks with agent assignments", "mimeType": "text/markdown"      },
                { "uri": "raios://master",           "name": "MASTER Rules",      "description": "MASTER.md — agent constitution and mandatory rules",           "mimeType": "text/markdown"      },
                { "uri": "raios://session/current",  "name": "Current Session",   "description": "Most recent open agent session — events, notes, context",      "mimeType": "application/json"   },
                { "uri": "raios://session/recent",   "name": "Recent Sessions",   "description": "Last 10 completed sessions",                                   "mimeType": "application/json"   }
            ]
        }))
    }

    pub(super) fn handle_resources_read(&self, params: &Value) -> Result<Value, String> {
        let uri = params["uri"].as_str().ok_or("missing uri")?;

        match uri {
            "raios://memory" => {
                let path = crate::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
                    .into_iter().next().map(|e| e.path)
                    .unwrap_or_else(|| self.config.dev_ops_path.join("memory.md"));
                let content = if path.exists() {
                    std::fs::read_to_string(&path).unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# Memory not found\nPath: {}", path.display())
                };
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }))
            }
            "raios://tasks" => {
                let path = self.config.dev_ops_path.join("tasks.md");
                let content = if path.exists() {
                    std::fs::read_to_string(&path).unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# Tasks not found\nPath: {}", path.display())
                };
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }))
            }
            "raios://master" => {
                let path = self.config.master_md_path.clone();
                let content = if path.exists() {
                    std::fs::read_to_string(&path).unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# MASTER Rules not found\nPath: {}", path.display())
                };
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }))
            }
            "raios://session/current" => {
                let store = crate::session::SessionStore::new(crate::session::SessionStore::default_path());
                match store.current_open() {
                    Some(sess) => {
                        let events = store.events(&sess.id);
                        let payload = json!({ "session": sess, "events": events });
                        Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }] }))
                    }
                    None => Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": json!({"session":null}).to_string() }] })),
                }
            }
            "raios://session/recent" => {
                let store = crate::session::SessionStore::new(crate::session::SessionStore::default_path());
                let sessions = store.recent(10);
                let payload = json!({ "sessions": sessions });
                Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }] }))
            }
            _ => Err(format!("Unknown resource: {}", uri)),
        }
    }
}
