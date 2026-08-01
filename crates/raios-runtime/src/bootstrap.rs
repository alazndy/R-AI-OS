use raios_core::config::BootstrapConfig;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub enum BootstrapAction {
    InstallNpmTool {
        name: String,
    },
    AddMarketplaceAndInstall {
        url: String,
        plugins: Vec<String>,
    },
    SyncRules {
        git_url: String,
        targets: Vec<PathBuf>,
    },
    EnablePlugin {
        name: String,
    },
}

impl BootstrapAction {
    pub fn describe(&self) -> String {
        match self {
            BootstrapAction::InstallNpmTool { name } => {
                format!("Install npm package \"{name}\" globally (skipped if already on PATH)")
            }
            BootstrapAction::AddMarketplaceAndInstall { url, plugins } => {
                format!(
                    "Add Claude Code marketplace {url} and install plugin(s): {}",
                    plugins.join(", ")
                )
            }
            BootstrapAction::SyncRules { git_url, targets } => {
                let target_list = targets
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Clone {git_url} and sync its rules/ into: {target_list}")
            }
            BootstrapAction::EnablePlugin { name } => {
                format!("Enable Claude Code plugin \"{name}\"")
            }
        }
    }
}

/// Mirrors the private `expand_tilde` in `proxy_store.rs` — small enough
/// that duplicating it here is simpler than introducing a shared-utils
/// module for one six-line helper.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

/// Builds the full list of actions `raios bootstrap` would take for the
/// given config. Pure: no process spawn, no filesystem write, no network —
/// safe to call for `--dry-run` or plan-printing with zero side effects.
pub fn build_plan(cfg: &BootstrapConfig) -> Vec<BootstrapAction> {
    let mut actions = Vec::new();

    for tool in &cfg.global_npm_tools {
        actions.push(BootstrapAction::InstallNpmTool { name: tool.clone() });
    }

    for marketplace in &cfg.claude_marketplaces {
        actions.push(BootstrapAction::AddMarketplaceAndInstall {
            url: marketplace.url.clone(),
            plugins: marketplace.plugins.clone(),
        });
    }

    for repo in &cfg.rule_sync_repos {
        let targets = repo
            .targets
            .iter()
            .map(|t| expand_tilde(Path::new(t)))
            .collect();
        actions.push(BootstrapAction::SyncRules {
            git_url: repo.git_url.clone(),
            targets,
        });
    }

    for plugin in &cfg.enable_claude_plugins {
        actions.push(BootstrapAction::EnablePlugin {
            name: plugin.clone(),
        });
    }

    actions
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    Ok,
    Skipped(String),
    Failed(String),
}

/// Executes every planned action best-effort: one failure doesn't stop the
/// rest. Callers decide whether to invoke this at all — see the dry-run/
/// empty-plan/confirmation gate in `raios-surface-cli`'s `cmd_bootstrap`.
pub fn execute(actions: &[BootstrapAction]) -> Vec<(String, ActionOutcome)> {
    actions
        .iter()
        .map(|action| (action.describe(), execute_one(action)))
        .collect()
}

fn execute_one(action: &BootstrapAction) -> ActionOutcome {
    match action {
        BootstrapAction::InstallNpmTool { name } => install_npm_tool(name),
        BootstrapAction::AddMarketplaceAndInstall { url, plugins } => {
            add_marketplace_and_install(url, plugins)
        }
        BootstrapAction::SyncRules { git_url, targets } => sync_rules(git_url, targets),
        BootstrapAction::EnablePlugin { name } => enable_plugin(name),
    }
}

fn install_npm_tool(name: &str) -> ActionOutcome {
    if which::which(name).is_ok() {
        return ActionOutcome::Skipped(format!("\"{name}\" already on PATH"));
    }
    if which::which("npm").is_err() {
        return ActionOutcome::Skipped("\"npm\" not found on PATH".to_string());
    }
    match Command::new("npm").args(["install", "-g", name]).status() {
        Ok(status) if status.success() => ActionOutcome::Ok,
        Ok(status) => ActionOutcome::Failed(format!("npm install exited with {status}")),
        Err(e) => ActionOutcome::Failed(format!("failed to run npm: {e}")),
    }
}

fn add_marketplace_and_install(url: &str, plugins: &[String]) -> ActionOutcome {
    if which::which("claude").is_err() {
        return ActionOutcome::Skipped("\"claude\" not found on PATH".to_string());
    }
    match Command::new("claude")
        .args(["plugin", "marketplace", "add", url])
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => return ActionOutcome::Failed(format!("marketplace add exited with {status}")),
        Err(e) => return ActionOutcome::Failed(format!("failed to add marketplace: {e}")),
    }
    for plugin in plugins {
        match Command::new("claude")
            .args(["plugin", "install", plugin, "--scope", "user"])
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return ActionOutcome::Failed(format!(
                    "install of \"{plugin}\" exited with {status}"
                ))
            }
            Err(e) => {
                return ActionOutcome::Failed(format!("failed to install \"{plugin}\": {e}"))
            }
        }
    }
    ActionOutcome::Ok
}

fn sanitize_repo_name(url: &str) -> String {
    url.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn sync_rules(git_url: &str, targets: &[PathBuf]) -> ActionOutcome {
    if which::which("git").is_err() {
        return ActionOutcome::Skipped("\"git\" not found on PATH".to_string());
    }
    let temp_dir = std::env::temp_dir().join(format!(
        "raios-bootstrap-{}",
        sanitize_repo_name(git_url)
    ));
    let clone_result = if temp_dir.exists() {
        Command::new("git")
            .current_dir(&temp_dir)
            .args(["pull"])
            .status()
    } else {
        Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                git_url,
                &temp_dir.to_string_lossy(),
            ])
            .status()
    };
    match clone_result {
        Ok(status) if status.success() => {}
        Ok(status) => return ActionOutcome::Failed(format!("git exited with {status}")),
        Err(e) => return ActionOutcome::Failed(format!("failed to run git: {e}")),
    }

    let src = temp_dir.join("rules");
    if !src.is_dir() {
        return ActionOutcome::Failed(format!("no rules/ directory in {git_url}"));
    }

    // Check if source directory has any files to copy
    let source_has_files = {
        use walkdir::WalkDir;
        WalkDir::new(&src)
            .into_iter()
            .filter_map(|e| e.ok())
            .any(|e| e.path().is_file())
    };

    for target in targets {
        if let Err(e) = std::fs::create_dir_all(target) {
            return ActionOutcome::Failed(format!(
                "failed to create {}: {e}",
                target.display()
            ));
        }
        let copied = copy_dir_recursive(&src, target);
        if source_has_files && copied == 0 {
            return ActionOutcome::Failed(format!(
                "no files copied to {} (0 files succeeded)",
                target.display()
            ));
        }
    }
    ActionOutcome::Ok
}

fn enable_plugin(name: &str) -> ActionOutcome {
    if which::which("claude").is_err() {
        return ActionOutcome::Skipped("\"claude\" not found on PATH".to_string());
    }
    match Command::new("claude")
        .args(["plugin", "enable", name])
        .status()
    {
        Ok(status) if status.success() => ActionOutcome::Ok,
        Ok(status) => ActionOutcome::Failed(format!("enable exited with {status}")),
        Err(e) => ActionOutcome::Failed(format!("failed to enable: {e}")),
    }
}

/// Moved from `raios-surface-cli/src/cli/new.rs` unchanged. Returns the
/// number of files actually copied.
fn copy_dir_recursive(src: &Path, dst: &Path) -> usize {
    use walkdir::WalkDir;
    let mut copied = 0;
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let destination = dst.join(path.strip_prefix(src).expect("Path stripping failed"));
        if path.is_dir() {
            let _ = std::fs::create_dir_all(&destination);
        } else if std::fs::copy(path, &destination).is_ok() {
            copied += 1;
        }
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use raios_core::config::{ClaudeMarketplace, RuleSyncRepo};

    #[test]
    fn build_plan_of_empty_config_is_empty() {
        let cfg = BootstrapConfig::default();
        assert!(build_plan(&cfg).is_empty());
    }

    #[test]
    fn build_plan_includes_one_action_per_npm_tool() {
        let cfg = BootstrapConfig {
            global_npm_tools: vec!["sigmap".to_string(), "ctx7".to_string()],
            ..Default::default()
        };
        let plan = build_plan(&cfg);
        assert_eq!(plan.len(), 2);
        assert!(plan[0].describe().contains("sigmap"));
        assert!(plan[1].describe().contains("ctx7"));
    }

    #[test]
    fn build_plan_groups_marketplace_and_its_plugins_into_one_action() {
        let cfg = BootstrapConfig {
            claude_marketplaces: vec![ClaudeMarketplace {
                url: "https://github.com/example/repo.git".to_string(),
                plugins: vec!["a@b".to_string(), "c@d".to_string()],
            }],
            ..Default::default()
        };
        let plan = build_plan(&cfg);
        assert_eq!(plan.len(), 1);
        let desc = plan[0].describe();
        assert!(desc.contains("https://github.com/example/repo.git"));
        assert!(desc.contains("a@b"));
        assert!(desc.contains("c@d"));
    }

    #[test]
    fn build_plan_expands_tilde_in_rule_sync_targets() {
        let cfg = BootstrapConfig {
            rule_sync_repos: vec![RuleSyncRepo {
                git_url: "https://github.com/example/rules.git".to_string(),
                targets: vec!["~/.claude/rules".to_string()],
            }],
            ..Default::default()
        };
        let plan = build_plan(&cfg);
        assert_eq!(plan.len(), 1);
        match &plan[0] {
            BootstrapAction::SyncRules { targets, .. } => {
                assert!(!targets[0].to_string_lossy().contains('~'));
                assert!(targets[0].ends_with(".claude/rules"));
            }
            other => panic!("expected SyncRules, got {other:?}"),
        }
    }

    #[test]
    fn build_plan_never_mentions_goktug_or_vault101() {
        // Regression guard, mirrors constitution.rs's `/home/alaz` leak-guard
        // pattern: even a config that reproduces the old hardcoded defaults
        // (see the migration in the design spec) must never resurrect the
        // personal MASTER.md write this rewrite removes.
        let cfg = BootstrapConfig {
            global_npm_tools: vec![
                "sigmap".to_string(),
                "ctx7".to_string(),
                "vercel".to_string(),
                "firebase-tools".to_string(),
            ],
            claude_marketplaces: vec![
                ClaudeMarketplace {
                    url: "https://github.com/josstei/maestro-orchestrate.git".to_string(),
                    plugins: vec!["maestro@maestro-orchestrator".to_string()],
                },
                ClaudeMarketplace {
                    url: "https://github.com/affaan-m/everything-claude-code.git".to_string(),
                    plugins: vec!["everything-claude-code@everything-claude-code".to_string()],
                },
            ],
            rule_sync_repos: vec![RuleSyncRepo {
                git_url: "https://github.com/affaan-m/everything-claude-code.git".to_string(),
                targets: vec![
                    "~/.claude/rules".to_string(),
                    "~/.antigravity/rules".to_string(),
                ],
            }],
            enable_claude_plugins: vec![
                "superpowers@claude-plugins-official".to_string(),
                "context7@claude-plugins-official".to_string(),
                "frontend-design@claude-plugins-official".to_string(),
                "github@claude-plugins-official".to_string(),
            ],
        };
        let plan = build_plan(&cfg);
        assert!(!plan.is_empty());
        for action in &plan {
            let desc = action.describe();
            assert!(!desc.contains("Goktug"));
            assert!(!desc.contains("Vault101"));
        }
    }
}
