use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub startup_bm25_indexing: bool,
    pub startup_cortex_indexing: bool,
    pub enable_health_worker: bool,
    pub health_interval_secs: u64,
    pub git_interval_secs: u64,
    pub enable_sentinel_worker: bool,
    pub sentinel_interval_secs: u64,
    pub enable_port_monitor: bool,
    pub port_monitor_interval_secs: u64,
    pub port_probe_timeout_ms: u64,
    /// Auto-lifecycle: days without commit before → beklemede
    pub lifecycle_standby_days: u64,
    /// Auto-lifecycle: days without commit before → archived
    pub lifecycle_archive_days: u64,
    /// How often the lifecycle worker runs (seconds)
    pub lifecycle_interval_secs: u64,
    pub enable_scheduler_worker: bool,
    pub scheduler_interval_secs: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let windows = cfg!(target_os = "windows");
        Self {
            startup_bm25_indexing: false,
            // True only if this crate was built with its own "cortex" feature.
            // raios-core has no direct dependency on the real embedding libs —
            // this flag exists so callers know whether real embeddings are
            // available. It must be forwarded explicitly from raios-runtime's
            // and raios-surface-cli's own "cortex" features (see their
            // Cargo.toml): cfg checks don't cross crate boundaries on their
            // own, so without that forwarding this silently stays false
            // even when the real embedding model is compiled in elsewhere.
            startup_cortex_indexing: cfg!(feature = "cortex"),
            enable_health_worker: true,
            health_interval_secs: if windows { 900 } else { 300 },
            git_interval_secs: if windows { 300 } else { 120 },
            enable_sentinel_worker: !windows,
            sentinel_interval_secs: if windows { 300 } else { 30 },
            enable_port_monitor: true,
            port_monitor_interval_secs: if windows { 30 } else { 10 },
            port_probe_timeout_ms: if windows { 75 } else { 100 },
            lifecycle_standby_days: 14,
            lifecycle_archive_days: 90,
            lifecycle_interval_secs: 3600,
            enable_scheduler_worker: true,
            scheduler_interval_secs: 60,
        }
    }
}

/// Runtime config — loaded from ~/.config/raios/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Root workspace folder (e.g. /home/user/dev)
    pub dev_ops_path: PathBuf,
    /// Path to AGENT_CONSTITUTION.md (central agent constitution)
    pub master_md_path: PathBuf,
    /// Path to .agents/skills directory
    pub skills_path: PathBuf,
    /// Path to Obsidian Vault Projects folder (optional — can be empty)
    #[serde(default)]
    pub vault_projects_path: PathBuf,
    /// K-AI-RA system name (default: "k-ai-ra")
    #[serde(default = "Config::default_system_name")]
    pub system_name: String,
    /// GitHub username
    #[serde(default)]
    pub github_user: String,
    /// Whether agent wrapper shell functions are installed
    #[serde(default)]
    pub agent_wrapper_enabled: bool,
    #[serde(default)]
    pub daemon: DaemonConfig,
    /// Product Factory is opt-in until its lifecycle services are implemented
    /// and explicitly enabled by the control owner.
    #[serde(default)]
    pub factory: FactoryConfig,
    /// `raios bootstrap` action list — empty by default (safe no-op).
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
}

/// Configuration boundary for the Product Factory domain.
///
/// This is intentionally data-only in the skeleton phase. It does not create
/// databases, move existing indexes, or enable any external integration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FactoryConfig {
    /// Kill switch for every Product Factory entry point. Defaults to deny.
    pub enabled: bool,
    /// Optional future locations for data separated by durability class.
    pub storage: FactoryStorageConfig,
}

/// Planned storage locations. `None` means "do not create or migrate yet".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FactoryStorageConfig {
    /// Rebuildable search cache target, planned as `search.db`.
    pub search_cache_path: Option<PathBuf>,
    /// Content-addressed artifact root.
    pub artifact_root: Option<PathBuf>,
    /// Recovery snapshot root.
    pub snapshot_root: Option<PathBuf>,
}

/// Configuration for `raios bootstrap` — an explicit, opt-in list of global
/// tools, Claude Code marketplaces/plugins, and rule-sync repos to install.
/// Empty by default: an unconfigured install is a safe no-op.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BootstrapConfig {
    /// Global npm packages to install if missing (e.g. "sigmap").
    pub global_npm_tools: Vec<String>,
    /// Claude Code plugin marketplaces to add, each with its plugins to install.
    pub claude_marketplaces: Vec<ClaudeMarketplace>,
    /// Git repos whose `rules/` directory gets synced into local agent rule dirs.
    pub rule_sync_repos: Vec<RuleSyncRepo>,
    /// Plugin names to enable from the official Claude Code marketplace.
    pub enable_claude_plugins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMarketplace {
    pub url: String,
    #[serde(default)]
    pub plugins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSyncRepo {
    pub git_url: String,
    #[serde(default)]
    pub targets: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dev_ops_path: PathBuf::new(),
            master_md_path: PathBuf::new(),
            skills_path: PathBuf::new(),
            vault_projects_path: PathBuf::new(),
            system_name: Self::default_system_name(),
            github_user: String::new(),
            agent_wrapper_enabled: false,
            daemon: DaemonConfig::default(),
            factory: FactoryConfig::default(),
            bootstrap: BootstrapConfig::default(),
        }
    }
}

impl Config {
    fn default_system_name() -> String {
        "k-ai-ra".to_string()
    }

    /// Returns the path to the config file.
    pub fn config_file() -> PathBuf {
        dirs::config_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("config.toml")
    }

    /// Load config from disk. Returns None if it doesn't exist yet.
    pub fn load() -> Option<Self> {
        let path = Self::config_file();
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Persist config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Try to build a config by auto-detecting common paths.
    /// Returns a DetectResult with found paths and a list of what is missing.
    pub fn auto_detect() -> DetectResult {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let desktop = dirs::desktop_dir().unwrap_or_else(|| home.join("Desktop"));

        // ── dev_ops_path ─────────────────────────────────────────────────────
        let dev_ops = find_dev_ops(&desktop, &home);

        // ── master_md_path ────────────────────────────────────────────────────
        let master_md = find_master_md(&home, dev_ops.as_deref());

        // ── skills_path ───────────────────────────────────────────────────────
        let skills = find_skills(dev_ops.as_deref());

        // ── vault_projects_path ──────────────────────────────────────────────
        let vault_projects = find_vault_projects(&home, master_md.as_deref());

        DetectResult {
            dev_ops,
            master_md,
            skills,
            vault_projects,
        }
    }

    pub fn from_detect_result(detected: DetectResult) -> Self {
        Self {
            dev_ops_path: detected.dev_ops.unwrap_or_else(|| PathBuf::from(".")),
            master_md_path: detected
                .master_md
                .unwrap_or_else(|| PathBuf::from("AGENT_CONSTITUTION.md")),
            skills_path: detected
                .skills
                .unwrap_or_else(|| PathBuf::from(".agents/skills")),
            vault_projects_path: detected.vault_projects.unwrap_or_default(),
            system_name: Self::default_system_name(),
            github_user: String::new(),
            agent_wrapper_enabled: false,
            daemon: DaemonConfig::default(),
            factory: FactoryConfig::default(),
            bootstrap: BootstrapConfig::default(),
        }
    }
}

pub struct DetectResult {
    pub dev_ops: Option<PathBuf>,
    pub master_md: Option<PathBuf>,
    pub skills: Option<PathBuf>,
    pub vault_projects: Option<PathBuf>,
}

// ─── Auto-detect helpers ──────────────────────────────────────────────────────

fn find_dev_ops(desktop: &Path, home: &Path) -> Option<PathBuf> {
    // Common names for the workspace root
    let candidates = [
        desktop.join("Dev Ops"),
        home.join("Dev Ops"),
        desktop.join("devops"),
        home.join("devops"),
        desktop.join("workspace"),
        home.join("workspace"),
        home.join("projects"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

fn find_master_md(home: &Path, dev_ops: Option<&Path>) -> Option<PathBuf> {
    // 1. Search Obsidian vaults
    let obsidian_root = home.join("Documents").join("Obsidian Vaults");
    if obsidian_root.is_dir() {
        if let Ok(vaults) = std::fs::read_dir(&obsidian_root) {
            for vault in vaults.filter_map(|e| e.ok()) {
                let candidate = vault.path().join("MASTER.md");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. Check AI OS / System inside Dev Ops
    if let Some(base) = dev_ops {
        let candidate = base.join("AI OS").join("System").join("MASTER.md");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 3. Home dir
    let h = home.join("MASTER.md");
    if h.exists() {
        return Some(h);
    }

    None
}

fn find_skills(dev_ops: Option<&Path>) -> Option<PathBuf> {
    // Common locations under dev_ops and home
    if let Some(base) = dev_ops {
        let candidate = base
            .join("AI OS")
            .join("System")
            .join(".agents")
            .join("skills");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn find_vault_projects(home: &Path, master_md: Option<&Path>) -> Option<PathBuf> {
    if let Some(master) = master_md {
        if let Some(vault_root) = master.parent() {
            let candidate = vault_root.join("Projeler");
            if candidate.is_dir() {
                return Some(candidate);
            }
            let candidate2 = vault_root.join("01_Projects");
            if candidate2.is_dir() {
                return Some(candidate2);
            }
            let candidate3 = vault_root.join("Dev Ops Projeleri");
            if candidate3.is_dir() {
                return Some(candidate3);
            }
        }
    }

    let obsidian_root = home.join("Documents").join("Obsidian Vaults");
    if obsidian_root.is_dir() {
        if let Ok(vaults) = std::fs::read_dir(&obsidian_root) {
            for vault in vaults.filter_map(|e| e.ok()) {
                let candidate = vault.path().join("Projeler");
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{Config, DaemonConfig};

    #[test]
    fn config_defaults_include_daemon_tuning() {
        let config = Config::default();
        assert!(!config.daemon.startup_bm25_indexing); // disabled by default to prevent startup CPU spike
        assert!(config.daemon.enable_port_monitor);
        assert!(config.daemon.port_monitor_interval_secs > 0);
        assert!(!config.factory.enabled);
        assert!(config.factory.storage.search_cache_path.is_none());
    }

    #[test]
    fn deserialize_legacy_config_uses_daemon_defaults() {
        let config: Config = toml::from_str(
            r#"
dev_ops_path = "/tmp/devops"
master_md_path = "/tmp/MASTER.md"
skills_path = "/tmp/.agents/skills"
"#,
        )
        .unwrap();

        assert_eq!(config.dev_ops_path, std::path::PathBuf::from("/tmp/devops"));
        assert_eq!(
            config.daemon.git_interval_secs,
            DaemonConfig::default().git_interval_secs
        );
        assert!(!config.factory.enabled);
    }

    #[test]
    fn bootstrap_config_defaults_to_empty() {
        let config = Config::default();
        assert!(config.bootstrap.global_npm_tools.is_empty());
        assert!(config.bootstrap.claude_marketplaces.is_empty());
        assert!(config.bootstrap.rule_sync_repos.is_empty());
        assert!(config.bootstrap.enable_claude_plugins.is_empty());
    }

    #[test]
    fn deserialize_legacy_config_without_bootstrap_section_uses_empty_default() {
        let config: Config = toml::from_str(
            r#"
dev_ops_path = "/tmp/devops"
master_md_path = "/tmp/MASTER.md"
skills_path = "/tmp/.agents/skills"
"#,
        )
        .unwrap();
        assert!(config.bootstrap.global_npm_tools.is_empty());
    }

    #[test]
    fn deserialize_config_with_bootstrap_section() {
        let config: Config = toml::from_str(
            r#"
dev_ops_path = "/tmp/devops"
master_md_path = "/tmp/MASTER.md"
skills_path = "/tmp/.agents/skills"

[bootstrap]
global_npm_tools = ["sigmap"]
enable_claude_plugins = ["github@claude-plugins-official"]

[[bootstrap.claude_marketplaces]]
url = "https://github.com/example/repo.git"
plugins = ["plugin@marketplace"]

[[bootstrap.rule_sync_repos]]
git_url = "https://github.com/example/rules.git"
targets = ["~/.claude/rules"]
"#,
        )
        .unwrap();
        assert_eq!(
            config.bootstrap.global_npm_tools,
            vec!["sigmap".to_string()]
        );
        assert_eq!(config.bootstrap.claude_marketplaces.len(), 1);
        assert_eq!(
            config.bootstrap.claude_marketplaces[0].url,
            "https://github.com/example/repo.git"
        );
        assert_eq!(
            config.bootstrap.claude_marketplaces[0].plugins,
            vec!["plugin@marketplace".to_string()]
        );
        assert_eq!(
            config.bootstrap.rule_sync_repos[0].targets,
            vec!["~/.claude/rules".to_string()]
        );
    }
}
