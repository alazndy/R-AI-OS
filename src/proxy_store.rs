/// Universal Capability Layer — Proxy-Store Bridge.
///
/// Agents interact with R-AI-OS via a generic tool action name.
/// The Store holds capability descriptors; the Proxy translates a generic
/// request into the correct native backend execution.
///
/// Supported backends:
///   - `internal`   — Rust function registered at compile time
///   - `python`     — Python script in ~/.agents/skills/
///   - `shell`      — Shell command / CLI wrapper
///   - `mcp_bridge` — Delegates to another MCP server (tool_name@server)
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::edge::EdgeRouter;

// ─── Capability descriptor ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// A built-in Rust handler identified by name.
    Internal { handler: String },
    /// A Python skill script.
    Python { script: PathBuf },
    /// An arbitrary shell command. `{input}` is substituted with JSON input.
    Shell { command: String },
    /// Delegates to a tool on another MCP server (format: "tool@server_name").
    McpBridge { tool: String, server: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Unique name agents use to request this capability.
    pub name: String,
    pub description: String,
    pub backend: Backend,
    /// Which agent ecosystems this capability is available in.
    pub platforms: Vec<String>,
}

// ─── Store ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct CapabilityStore {
    entries: HashMap<String, Capability>,
}

impl CapabilityStore {
    pub fn new() -> Self {
        let mut store = Self::default();
        store.register_defaults();
        store
    }

    pub fn register(&mut self, cap: Capability) {
        self.entries.insert(cap.name.clone(), cap);
    }

    pub fn get(&self, name: &str) -> Option<&Capability> {
        self.entries.get(name)
    }

    pub fn list(&self) -> Vec<&Capability> {
        let mut caps: Vec<&Capability> = self.entries.values().collect();
        caps.sort_by_key(|c| c.name.as_str());
        caps
    }

    fn register_defaults(&mut self) {
        let defaults = vec![
            Capability {
                name: "health_check".into(),
                description: "Run R-AI-OS health scan on a project".into(),
                backend: Backend::Internal {
                    handler: "health_check".into(),
                },
                platforms: vec![
                    "claude".into(),
                    "gemini".into(),
                    "codex".into(),
                    "antigravity".into(),
                ],
            },
            Capability {
                name: "list_projects".into(),
                description: "List all known projects in the workspace".into(),
                backend: Backend::Internal {
                    handler: "list_projects".into(),
                },
                platforms: vec![
                    "claude".into(),
                    "gemini".into(),
                    "codex".into(),
                    "antigravity".into(),
                ],
            },
            Capability {
                name: "search_codebase".into(),
                description: "Hybrid BM25+vector search across all indexed projects".into(),
                backend: Backend::Internal {
                    handler: "search_codebase".into(),
                },
                platforms: vec![
                    "claude".into(),
                    "gemini".into(),
                    "codex".into(),
                    "antigravity".into(),
                ],
            },
            Capability {
                name: "run_sentinel".into(),
                description: "Run compile/lint checks on a project".into(),
                backend: Backend::Shell {
                    command: "raios sentinel --project {input}".into(),
                },
                platforms: vec!["claude".into(), "gemini".into()],
            },
            Capability {
                name: "graphify".into(),
                description: "Build a knowledge graph from project files".into(),
                backend: Backend::Python {
                    script: PathBuf::from("~/.agents/skills/graphify/run.py"),
                },
                platforms: vec!["gemini".into(), "claude".into()],
            },
            Capability {
                name: "prompt_master".into(),
                description: "Optimize a prompt before sending it to an AI tool".into(),
                backend: Backend::Python {
                    script: PathBuf::from("~/.agents/skills/prompt-master/run.py"),
                },
                platforms: vec!["claude".into(), "gemini".into(), "antigravity".into()],
            },
        ];

        for cap in defaults {
            self.entries.insert(cap.name.clone(), cap);
        }
    }
}

// ─── Proxy ────────────────────────────────────────────────────────────────────

/// Translates a generic capability invocation to the native backend.
pub struct CapabilityProxy {
    store: CapabilityStore,
    router: EdgeRouter,
    /// Registered internal handlers: name → Box<dyn Fn(String) -> Result<String>>
    internal_handlers: HashMap<String, Box<dyn Fn(String) -> Result<String> + Send + Sync>>,
}

impl CapabilityProxy {
    pub fn new(store: CapabilityStore) -> Self {
        let descriptions = store
            .list()
            .iter()
            .map(|c| (c.name.clone(), c.description.clone()))
            .collect();
        let router = EdgeRouter::new(descriptions);
        let mut proxy = Self {
            store,
            router,
            internal_handlers: HashMap::new(),
        };
        proxy.register_internal_handlers();
        proxy
    }

    fn register_internal_handlers(&mut self) {
        self.internal_handlers.insert(
            "health_check".into(),
            Box::new(|input| {
                let project = if input.trim().is_empty() {
                    "."
                } else {
                    input.trim()
                };
                Ok(serde_json::json!({ "status": "ok", "project": project }).to_string())
            }),
        );

        self.internal_handlers.insert(
            "list_projects".into(),
            Box::new(|_input| {
                let config = crate::config::Config::load();
                let projects = config
                    .map(|c| crate::entities::discover_entities(&c.dev_ops_path))
                    .unwrap_or_default();
                let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
                Ok(serde_json::json!({ "projects": names }).to_string())
            }),
        );

        self.internal_handlers.insert(
            "search_codebase".into(),
            Box::new(|input| {
                Ok(serde_json::json!({
                    "query": input,
                    "note": "connect to aiosd on port 42069 and send VectorSearch command for live results"
                })
                .to_string())
            }),
        );
    }

    /// Execute a capability by name with JSON-encoded input.
    /// Returns a JSON-encoded result string.
    pub fn execute(&self, capability: &str, input: &str) -> Result<String> {
        let cap = self
            .store
            .get(capability)
            .ok_or_else(|| anyhow::anyhow!("Unknown capability: {capability}"))?;

        match &cap.backend {
            Backend::Internal { handler } => {
                let h = self
                    .internal_handlers
                    .get(handler.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Internal handler '{handler}' not registered")
                    })?;
                h(input.to_string())
            }

            Backend::Python { script } => {
                let expanded = expand_tilde(script);
                if !expanded.exists() {
                    bail!("Python skill not found: {}", expanded.display());
                }
                let output = std::process::Command::new("python")
                    .args([expanded.to_str().unwrap_or(""), input])
                    .output()?;
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }

            Backend::Shell { command } => {
                let cmd = command.replace("{input}", input);
                let output = if cfg!(target_os = "windows") {
                    std::process::Command::new("cmd")
                        .args(["/C", &cmd])
                        .output()?
                } else {
                    std::process::Command::new("sh")
                        .args(["-c", &cmd])
                        .output()?
                };
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }

            Backend::McpBridge { tool, server } => {
                // Emit a JSON blob the caller can forward to the target MCP server
                Ok(serde_json::json!({
                    "mcp_delegate": true,
                    "server": server,
                    "tool": tool,
                    "input": input,
                })
                .to_string())
            }
        }
    }

    pub fn route(&self, query: &str, input: &str) -> Result<String> {
        let cap_name = self.router.route(query).unwrap_or(query);
        self.execute(cap_name, input)
    }

    pub fn store(&self) -> &CapabilityStore {
        &self.store
    }
}

fn expand_tilde(path: &PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&s[2..]);
        }
    }
    path.clone()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_proxy() -> CapabilityProxy {
        CapabilityProxy::new(CapabilityStore::new())
    }

    #[test]
    fn store_has_default_capabilities() {
        let store = CapabilityStore::new();
        assert!(store.get("health_check").is_some());
        assert!(store.get("list_projects").is_some());
        assert!(store.get("graphify").is_some());
        assert!(store.get("prompt_master").is_some());
    }

    #[test]
    fn unknown_capability_returns_error() {
        let proxy = make_proxy();
        assert!(proxy.execute("does_not_exist", "{}").is_err());
    }

    #[test]
    fn internal_health_check_executes() {
        let proxy = make_proxy();
        let result = proxy.execute("health_check", "my-project").unwrap();
        let val: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(val["project"], "my-project");
        assert_eq!(val["status"], "ok");
    }

    #[test]
    fn mcp_bridge_returns_delegate_payload() {
        let mut store = CapabilityStore::new();
        store.register(Capability {
            name: "remote_search".into(),
            description: "Search via remote MCP server".into(),
            backend: Backend::McpBridge {
                tool: "search".into(),
                server: "codebase-mcp".into(),
            },
            platforms: vec!["claude".into()],
        });
        let proxy = CapabilityProxy::new(store);
        let result = proxy
            .execute("remote_search", r#"{"query":"foo"}"#)
            .unwrap();
        let val: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(val["mcp_delegate"], true);
        assert_eq!(val["server"], "codebase-mcp");
    }

    #[test]
    fn list_returns_sorted_capabilities() {
        let store = CapabilityStore::new();
        let caps = store.list();
        let names: Vec<&str> = caps.iter().map(|c| c.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn route_natural_language_finds_capability() {
        let proxy = make_proxy();
        let result = proxy.route("check project health", "my-project");
        assert!(result.is_ok());
    }

    #[test]
    fn route_exact_name_still_works() {
        let proxy = make_proxy();
        let result = proxy.route("health_check", "my-project");
        assert!(result.is_ok());
    }
}
