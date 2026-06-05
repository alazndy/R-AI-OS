use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::{Result, anyhow};

// ─── Config Schema ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub server: Option<ServerPolicy>,
    pub filesystem: FilesystemPolicy,
    pub tools: ToolsPolicy,
    /// Optional egress (network) filtering rules
    pub egress: Option<EgressPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPolicy {
    pub http_port: Option<u16>,
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
            PolicyAction::Allow   => "allow",
            PolicyAction::Deny    => "deny",
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
pub struct ToolRule {
    pub name: String,
    pub action: PolicyAction,
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
        let candidates = [
            std::env::current_dir()
                .ok()?
                .join("raios-policy.toml"),
            dirs::config_dir()?.join("raios").join("raios-policy.toml"),
        ];
        for path in &candidates {
            if path.exists() {
                if let Ok(config) = Self::load_from_file(path) {
                    return Some(config);
                }
            }
        }
        None
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
        assert!(config.filesystem.blocked_paths.contains(&"C:/Users/turha/.ssh".to_string()));
    }
}
