use raios_core::config::BootstrapConfig;
use std::path::{Path, PathBuf};

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
