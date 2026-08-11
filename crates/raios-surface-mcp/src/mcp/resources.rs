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
                let path =
                    raios_runtime::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
                        .into_iter()
                        .next()
                        .map(|e| e.path)
                        .unwrap_or_else(|| self.config.dev_ops_path.join("memory.md"));
                let content = if path.exists() {
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# Memory not found\nPath: {}", path.display())
                };
                Ok(
                    json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }),
                )
            }
            "raios://tasks" => {
                let path = self.config.dev_ops_path.join("tasks.md");
                let content = if path.exists() {
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# Tasks not found\nPath: {}", path.display())
                };
                Ok(
                    json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }),
                )
            }
            "raios://master" => {
                let path = self.config.master_md_path.clone();
                let content = if path.exists() {
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| format!("# Error reading file\n{}", e))
                } else {
                    format!("# MASTER Rules not found\nPath: {}", path.display())
                };
                Ok(
                    json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": content }] }),
                )
            }
            "raios://session/current" => {
                let store = raios_runtime::session::SessionStore::new(
                    raios_runtime::session::SessionStore::default_path(),
                );
                match store.current_open() {
                    Some(sess) => {
                        let events = store.events(&sess.id);
                        let payload = json!({ "session": sess, "events": events });
                        Ok(
                            json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }] }),
                        )
                    }
                    None => Ok(
                        json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": json!({"session":null}).to_string() }] }),
                    ),
                }
            }
            "raios://session/recent" => {
                let store = raios_runtime::session::SessionStore::new(
                    raios_runtime::session::SessionStore::default_path(),
                );
                let sessions = store.recent(10);
                let payload = json!({ "sessions": sessions });
                Ok(
                    json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }] }),
                )
            }
            _ => Err(format!("Unknown resource: {}", uri)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::McpServer;
    use raios_core::config::Config;
    use raios_core::security::quarantine::QuarantineStore;
    use raios_core::security::rate_limiter::RateLimiter;
    use raios_core::security::{EgressFilter, Umai};
    use serde_json::json;
    use std::path::Path;

    // McpServer::new() has global side effects (opens the shared workspace
    // DB, loads real policy config) unsafe under parallel `cargo test`. Its
    // fields are private but visible here since this module is a descendant
    // of `mcp`, so tests build one directly with disabled security subsystems
    // and a config pointed at an isolated temp dir.
    fn test_server(dev_ops: &Path, master_md: &Path) -> McpServer {
        McpServer {
            config: Config {
                dev_ops_path: dev_ops.to_path_buf(),
                master_md_path: master_md.to_path_buf(),
                ..Default::default()
            },
            rate_limiter: RateLimiter::disabled(),
            quarantine: QuarantineStore::disabled(),
            pin_broken: false,
            umai: Umai::new(None),
            egress: EgressFilter::disabled(),
            blocked_paths: vec![],
        }
    }

    #[test]
    fn resources_list_advertises_all_five_known_uris() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let result = server.handle_resources_list().unwrap();
        let uris: Vec<&str> = result["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        assert_eq!(
            uris,
            [
                "raios://memory",
                "raios://tasks",
                "raios://master",
                "raios://session/current",
                "raios://session/recent",
            ]
        );
    }

    #[test]
    fn resources_read_rejects_missing_uri_param() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let err = server.handle_resources_read(&json!({})).unwrap_err();
        assert_eq!(err, "missing uri");
    }

    #[test]
    fn resources_read_rejects_unknown_uri() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let err = server
            .handle_resources_read(&json!({"uri": "raios://nonsense"}))
            .unwrap_err();
        assert_eq!(err, "Unknown resource: raios://nonsense");
    }

    #[test]
    fn resources_read_memory_falls_back_when_no_memory_md_exists() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let result = server
            .handle_resources_read(&json!({"uri": "raios://memory"}))
            .unwrap();
        let text = result["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("Memory not found"));
    }

    #[test]
    fn resources_read_memory_returns_discovered_file_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("memory.md"), "# Project Memory\nHello.").unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let result = server
            .handle_resources_read(&json!({"uri": "raios://memory"}))
            .unwrap();
        let text = result["contents"][0]["text"].as_str().unwrap();
        assert_eq!(text, "# Project Memory\nHello.");
        assert_eq!(result["contents"][0]["mimeType"], "text/markdown");
    }

    #[test]
    fn resources_read_tasks_falls_back_when_no_tasks_md_exists() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let result = server
            .handle_resources_read(&json!({"uri": "raios://tasks"}))
            .unwrap();
        let text = result["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("Tasks not found"));
    }

    #[test]
    fn resources_read_tasks_returns_file_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tasks.md"), "- [ ] do the thing").unwrap();
        let server = test_server(dir.path(), &dir.path().join("MASTER.md"));
        let result = server
            .handle_resources_read(&json!({"uri": "raios://tasks"}))
            .unwrap();
        assert_eq!(
            result["contents"][0]["text"].as_str().unwrap(),
            "- [ ] do the thing"
        );
    }

    #[test]
    fn resources_read_master_falls_back_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing_master = dir.path().join("does-not-exist.md");
        let server = test_server(dir.path(), &missing_master);
        let result = server
            .handle_resources_read(&json!({"uri": "raios://master"}))
            .unwrap();
        let text = result["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("MASTER Rules not found"));
    }

    #[test]
    fn resources_read_master_returns_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let master_path = dir.path().join("MASTER.md");
        std::fs::write(&master_path, "# Constitution").unwrap();
        let server = test_server(dir.path(), &master_path);
        let result = server
            .handle_resources_read(&json!({"uri": "raios://master"}))
            .unwrap();
        assert_eq!(
            result["contents"][0]["text"].as_str().unwrap(),
            "# Constitution"
        );
    }
}
