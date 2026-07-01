use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::security::quarantine::QuarantineConfig;
use crate::security::rate_limiter::RateLimitConfig;

// ─── Config Schema ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub server: Option<ServerPolicy>,
    pub filesystem: FilesystemPolicy,
    pub tools: ToolsPolicy,
    /// Optional agent-run preflight gate. Absent means disabled so older
    /// policy files continue to behave exactly as before.
    pub preflight: Option<PreflightPolicy>,
    /// Optional egress (network) filtering rules
    pub egress: Option<EgressPolicy>,
    /// Optional rate limiting rules (Phase 13)
    pub rate_limits: Option<RateLimitConfig>,
    /// Optional quarantine rules (Phase 14)
    pub quarantine: Option<QuarantineConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPolicy {
    pub http_port: Option<u16>,
    /// Optional hub (remote access) configuration
    pub hub: Option<HubPolicy>,
}

/// Controls how the Cortex Hub exposes its ports to the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubPolicy {
    /// "localhost" (default) | "tailscale" | "all"
    #[serde(default = "HubPolicy::default_bind_mode")]
    pub bind_mode: String,
    /// Trusted CIDR for remote clients (default: Tailscale range 100.64.0.0/10)
    #[serde(default = "HubPolicy::default_trusted_cidr")]
    pub trusted_cidr: String,
    /// SHA-256 hex hash of the remote API key (never store the key in plaintext)
    pub api_key_hash: Option<String>,
}

impl HubPolicy {
    fn default_bind_mode() -> String { "localhost".into() }
    fn default_trusted_cidr() -> String { "100.64.0.0/10".into() }
}

impl Default for HubPolicy {
    fn default() -> Self {
        Self {
            bind_mode: Self::default_bind_mode(),
            trusted_cidr: Self::default_trusted_cidr(),
            api_key_hash: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressPolicy {
    pub enabled: bool,
    pub deny_all: Option<bool>,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    pub enforce_sandbox: bool,
    pub allowed_paths: Vec<String>,
    pub blocked_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyAction {
    Allow,
    Deny,
    Confirm,
}

impl PolicyAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            PolicyAction::Allow => "allow",
            PolicyAction::Deny => "deny",
            PolicyAction::Confirm => "confirm",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsPolicy {
    pub default_action: PolicyAction,
    pub rules: Vec<ToolRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightPolicy {
    pub enforce_before_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRule {
    pub name: String,
    pub action: PolicyAction,
    /// Optional explicit capability declaration for this tool, overriding the
    /// built-in default from `security::capabilities::default_for`. Absent by
    /// default so existing `raios-policy.toml` files keep working unchanged.
    #[serde(default)]
    pub capabilities: Option<ToolCapabilities>,
}

/// Declares what a tool is allowed to touch — filesystem paths (as glob
/// patterns), network domains, and whether it may spawn subprocesses.
///
/// This is a *capability* grant, not a rule action: even an `Allow`-ed tool
/// with no declared capability cannot smuggle in a filesystem or network
/// access it never asked for ("no ambient authority"). See
/// `security::capabilities` for enforcement and the built-in default map.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolCapabilities {
    /// Glob patterns for paths this tool may read. `["*"]` means "whatever
    /// path the tool resolves for itself" (used by portfolio tools that
    /// operate on a caller-supplied project path rather than a fixed root).
    #[serde(default)]
    pub fs_read: Vec<String>,
    /// Glob patterns for paths this tool may write.
    #[serde(default)]
    pub fs_write: Vec<String>,
    /// Domains this tool may connect to (checked against the egress policy).
    #[serde(default)]
    pub network: Vec<String>,
    /// Whether this tool is expected to spawn subprocesses (cargo, git, …).
    /// Declarative only today — surfaced via `raios policy caps` for
    /// visibility, not yet enforced at a process-spawn chokepoint.
    #[serde(default)]
    pub exec: bool,
}

// ─── Load & Query ─────────────────────────────────────────────────────────────

impl PolicyConfig {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read policy file {:?}: {}", path, e))?;
        let config: PolicyConfig = toml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse policy TOML {:?}: {}", path, e))?;
        Ok(config)
    }

    /// Returns the resolved policy action for the given tool name.
    pub fn tool_action(&self, tool_name: &str) -> &PolicyAction {
        self.tools
            .rules
            .iter()
            .find(|r| r.name == tool_name)
            .map(|r| &r.action)
            .unwrap_or(&self.tools.default_action)
    }

    /// Returns `Ok(())` if the tool is allowed, `Err(...)` if denied,
    /// or `Err(...)` containing "confirm:" prefix if user confirmation is needed.
    pub fn validate_tool_call(&self, tool_name: &str) -> Result<()> {
        match self.tool_action(tool_name) {
            PolicyAction::Allow => Ok(()),
            PolicyAction::Deny => Err(anyhow!(
                "Policy Denied: tool '{}' is blocked by security policy",
                tool_name
            )),
            PolicyAction::Confirm => Err(anyhow!(
                "confirm:Policy requires user confirmation before running tool '{}'",
                tool_name
            )),
        }
    }

    /// Returns `true` if this tool needs interactive confirmation.
    pub fn needs_confirmation(&self, tool_name: &str) -> bool {
        matches!(self.tool_action(tool_name), PolicyAction::Confirm)
    }

    /// Try to load from default paths; returns `None` if not found (disabled).
    pub fn try_load_default() -> Option<Self> {
        // Try current dir first, then user config dir
        for path in Self::candidate_paths()? {
            if path.exists() {
                if let Ok(config) = Self::load_from_file(&path) {
                    return Some(config);
                }
            }
        }
        None
    }

    /// Resolves the same candidate paths as `try_load_default`, but returns
    /// the first one that *exists and parses successfully* — used by `raios
    /// policy suggest --apply` to know which file to append accepted
    /// suggestions to (must match what `try_load_default` would have loaded).
    pub fn default_path() -> Option<std::path::PathBuf> {
        Self::candidate_paths()?
            .into_iter()
            .find(|path| path.exists() && Self::load_from_file(path).is_ok())
    }

    fn candidate_paths() -> Option<[std::path::PathBuf; 2]> {
        Some([
            std::env::current_dir().ok()?.join("raios-policy.toml"),
            dirs::config_dir()?.join("raios").join("raios-policy.toml"),
        ])
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_policy(tmp: &TempDir, content: &str) -> std::path::PathBuf {
        let path = tmp.path().join("raios-policy.toml");
        fs::write(&path, content).unwrap();
        path
    }

    const SAMPLE_TOML: &str = r#"
[server]
http_port = 42071

[filesystem]
enforce_sandbox = true
allowed_paths = []
blocked_paths = ["C:/Users/turha/.ssh"]

[tools]
default_action = "confirm"

[[tools.rules]]
name = "list_projects"
action = "allow"

[[tools.rules]]
name = "run_build"
action = "deny"
"#;

    #[test]
    fn load_and_query_allow() {
        let tmp = TempDir::new().unwrap();
        let path = write_policy(&tmp, SAMPLE_TOML);
        let config = PolicyConfig::load_from_file(&path).unwrap();
        assert_eq!(config.tool_action("list_projects"), &PolicyAction::Allow);
        assert!(config.validate_tool_call("list_projects").is_ok());
    }

    #[test]
    fn load_and_query_deny() {
        let tmp = TempDir::new().unwrap();
        let path = write_policy(&tmp, SAMPLE_TOML);
        let config = PolicyConfig::load_from_file(&path).unwrap();
        assert_eq!(config.tool_action("run_build"), &PolicyAction::Deny);
        let err = config.validate_tool_call("run_build").unwrap_err();
        assert!(err.to_string().contains("Policy Denied"));
    }

    #[test]
    fn unknown_tool_falls_back_to_default_confirm() {
        let tmp = TempDir::new().unwrap();
        let path = write_policy(&tmp, SAMPLE_TOML);
        let config = PolicyConfig::load_from_file(&path).unwrap();
        assert_eq!(config.tool_action("get_stats"), &PolicyAction::Confirm);
        assert!(config.needs_confirmation("get_stats"));
        let err = config.validate_tool_call("get_stats").unwrap_err();
        assert!(err.to_string().starts_with("confirm:"));
    }

    #[test]
    fn filesystem_policy_fields_load_correctly() {
        let tmp = TempDir::new().unwrap();
        let path = write_policy(&tmp, SAMPLE_TOML);
        let config = PolicyConfig::load_from_file(&path).unwrap();
        assert!(config.filesystem.enforce_sandbox);
        assert!(config
            .filesystem
            .blocked_paths
            .contains(&"C:/Users/turha/.ssh".to_string()));
    }
}
