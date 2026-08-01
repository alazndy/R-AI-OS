# Bootstrap Safety & Genericization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `raios bootstrap` safe for a stranger to run: move its hardcoded personal action list into config (empty by default), gate all execution behind an inspectable plan + explicit confirmation, and delete the hardcoded personal-identity file write.

**Architecture:** Following the existing 3-crate layering already used by `cmd_new`/`new_project::create` — plan/execute business logic lives in `raios-runtime` (a new `bootstrap` module), the action-list data structure lives in `raios-core::config` (alongside the existing `FactoryConfig`), and `raios-surface-cli` keeps only a thin wrapper: parse args, print the plan, ask for confirmation, call into `raios-runtime`, print results.

**Tech Stack:** Rust (existing workspace: `raios-core`, `raios-runtime`, `raios-surface-cli`), `clap` derive for CLI args, `which` crate (already a dependency) for cross-platform binary lookups, `walkdir` (already a dependency) for recursive copy, `toml`/`serde` for config. No new dependencies.

## Global Constraints

- No new Cargo dependencies — `which`, `walkdir`, `serde`, `toml`, `dirs` are already present in the relevant crates.
- Confirmation default is **No** (secure-by-default) — an empty `Enter` at the prompt must abort, not proceed.
- Exit code: `0` if the run completes with zero `Failed` outcomes (a `Skipped` outcome does not count as failure), `1` if at least one action `Failed`.
- `--dry-run` and an empty plan (`build_plan` returns `[]`) must never call `execute()` — no process spawn, no filesystem write, no network access. This must be true by control-flow construction (early `return`), not by runtime configuration.
- `build_plan()` must be a pure function: no `std::process::Command`, no `std::fs` writes, no network. It may call `dirs::home_dir()` for tilde-expansion (reads an env var, not I/O against a specific target).
- No literal `"Goktug"` or `"Vault101"` string may appear anywhere in `crates/raios-surface-cli/src/cli/new.rs` or `crates/raios-runtime/src/bootstrap.rs` after this work — enforced by a regression test.
- Follow the existing crate-layering convention: business logic in `raios-runtime`, thin argument-handling/printing in `raios-surface-cli`, data-only config structs in `raios-core`.

---

### Task 1: `BootstrapConfig` data structures in `raios-core`

**Files:**
- Modify: `crates/raios-core/src/config.rs`

**Interfaces:**
- Produces: `raios_core::config::BootstrapConfig { global_npm_tools: Vec<String>, claude_marketplaces: Vec<ClaudeMarketplace>, rule_sync_repos: Vec<RuleSyncRepo>, enable_claude_plugins: Vec<String> }`, `raios_core::config::ClaudeMarketplace { url: String, plugins: Vec<String> }`, `raios_core::config::RuleSyncRepo { git_url: String, targets: Vec<String> }`. `Config` gains a `pub bootstrap: BootstrapConfig` field, defaulting to `BootstrapConfig::default()` (all `Vec`s empty).

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/raios-core/src/config.rs` (after the existing `deserialize_legacy_config_uses_daemon_defaults` test):

```rust
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
        assert_eq!(config.bootstrap.global_npm_tools, vec!["sigmap".to_string()]);
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
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test -p raios-core config::tests:: 2>&1 | tail -30`
Expected: compile error — `no field \`bootstrap\` on type \`Config\`` (the type doesn't exist yet).

- [ ] **Step 3: Add the struct definitions**

In `crates/raios-core/src/config.rs`, immediately after the closing brace of `FactoryStorageConfig` (right before `impl Default for Config {`), insert:

```rust
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
```

- [ ] **Step 4: Add the field to `Config` and wire both constructors**

In the `Config` struct definition, after the `factory: FactoryConfig` field, add:

```rust
    /// `raios bootstrap` action list — empty by default (safe no-op).
    #[serde(default)]
    pub bootstrap: BootstrapConfig,
```

In `impl Default for Config`'s struct literal, after `factory: FactoryConfig::default(),`, add:

```rust
            bootstrap: BootstrapConfig::default(),
```

In `Config::from_detect_result`'s struct literal, after `factory: FactoryConfig::default(),`, add the same line:

```rust
            bootstrap: BootstrapConfig::default(),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p raios-core config::tests:: 2>&1 | tail -30`
Expected: `test result: ok. N passed; 0 failed` (N = previous count + 3).

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p raios-core --all-targets --all-features -- -D warnings 2>&1 | tail -30`
Expected: clean, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/raios-core/src/config.rs
git commit -m "feat(config): add BootstrapConfig for raios bootstrap"
```

---

### Task 2: `BootstrapAction` + pure `build_plan()` in `raios-runtime`

**Files:**
- Create: `crates/raios-runtime/src/bootstrap.rs`
- Modify: `crates/raios-runtime/src/lib.rs` (register the module)

**Interfaces:**
- Consumes: `raios_core::config::{BootstrapConfig, ClaudeMarketplace, RuleSyncRepo}` (Task 1).
- Produces: `raios_runtime::bootstrap::BootstrapAction` enum with a `.describe() -> String` method, and `raios_runtime::bootstrap::build_plan(cfg: &BootstrapConfig) -> Vec<BootstrapAction>` (pure — no I/O beyond `dirs::home_dir()` for tilde-expansion). Task 3 adds `ActionOutcome` and `execute()` to this same file.

- [ ] **Step 1: Register the module**

In `crates/raios-runtime/src/lib.rs`, replace:

```rust
pub mod agent_runner;
pub mod agent_wrapper;
pub mod anka;
```

with:

```rust
pub mod agent_runner;
pub mod agent_wrapper;
pub mod bootstrap;
pub mod anka;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/raios-runtime/src/bootstrap.rs` with only the type definitions, `describe()`, `build_plan()`, and this test module (execute-side code comes in Task 3):

```rust
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
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p raios-runtime bootstrap:: 2>&1 | tail -30`
Expected: `test result: ok. 5 passed; 0 failed`.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -p raios-runtime --all-targets --all-features -- -D warnings 2>&1 | tail -30`
Expected: clean, 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-runtime/src/bootstrap.rs crates/raios-runtime/src/lib.rs
git commit -m "feat(bootstrap): add pure BootstrapAction/build_plan"
```

---

### Task 3: `ActionOutcome` + `execute()` in `raios-runtime::bootstrap`

**Files:**
- Modify: `crates/raios-runtime/src/bootstrap.rs`

**Interfaces:**
- Consumes: `BootstrapAction` (Task 2).
- Produces: `raios_runtime::bootstrap::ActionOutcome` enum (`Ok`, `Skipped(String)`, `Failed(String)`), `raios_runtime::bootstrap::execute(actions: &[BootstrapAction]) -> Vec<(String, ActionOutcome)>`.

No new automated tests in this task: `execute()` shells out to `npm`/`claude`/`git` over the network, so unit-testing it would either mock away everything worth testing or make real network calls in CI (neither matches the design spec's testing section, which scopes automated coverage to `build_plan()` purity + the structural dry-run/empty-config guarantee + the string regression test — all already covered by Task 2's tests and Task 4's control-flow). `execute()` is verified manually in Task 6 via `raios bootstrap --dry-run` and a real confirmed run against Göktuğ's own migrated config.

- [ ] **Step 1: Add `ActionOutcome` and `execute()`**

Append to `crates/raios-runtime/src/bootstrap.rs`, after `build_plan()` and before the `#[cfg(test)]` block, add `use std::process::Command;` to the top-of-file `use` block, then add:

```rust
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
    if let Err(e) = Command::new("claude")
        .args(["plugin", "marketplace", "add", url])
        .status()
    {
        return ActionOutcome::Failed(format!("failed to add marketplace: {e}"));
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

    for target in targets {
        if let Err(e) = std::fs::create_dir_all(target) {
            return ActionOutcome::Failed(format!(
                "failed to create {}: {e}",
                target.display()
            ));
        }
        copy_dir_recursive(&src, target);
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
```

- [ ] **Step 2: Confirm the crate still compiles and existing tests still pass**

Run: `cargo test -p raios-runtime bootstrap:: 2>&1 | tail -30`
Expected: `test result: ok. 5 passed; 0 failed` (same 5 from Task 2 — this task adds no new tests, see rationale above).

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p raios-runtime --all-targets --all-features -- -D warnings 2>&1 | tail -30`
Expected: clean, 0 warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/raios-runtime/src/bootstrap.rs
git commit -m "feat(bootstrap): add execute() and move copy_dir_recursive from CLI crate"
```

---

### Task 4: Rewire the CLI — args, dispatch, and the thin `cmd_bootstrap` wrapper

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/args.rs` (the `Bootstrap` variant, ~line 233-234)
- Modify: `crates/raios-surface-cli/src/cli/mod.rs` (the `Commands::Bootstrap` dispatch arm, ~line 214)
- Modify: `crates/raios-surface-cli/src/cli/new.rs:92-243` (delete `cmd_bootstrap` body at 92-208, `copy_dir_recursive` at 210-225, `DEFAULT_MASTER_MD` at 227-243; replace with the new thin wrapper)

**Interfaces:**
- Consumes: `raios_runtime::bootstrap::{build_plan, execute, ActionOutcome}` (Tasks 2–3), `raios_core::config::BootstrapConfig` (Task 1), the already-in-scope `cfg: raios_core::config::Config` local variable in `mod.rs`'s dispatch function.
- Produces: `cmd_bootstrap(cfg: &raios_core::config::BootstrapConfig, dry_run: bool, yes: bool)` in `new.rs`, called from `mod.rs`.

- [ ] **Step 1: Update the `Bootstrap` CLI variant**

In `crates/raios-surface-cli/src/cli/args.rs`, replace:

```rust
    /// Install/Bootstrap the entire ECC, Maestro, and system architecture
    Bootstrap,
```

with:

```rust
    /// Install/enable the tools, Claude Code marketplaces/plugins, and
    /// rule-sync repos configured under [bootstrap] in config.toml
    Bootstrap {
        /// Print the plan and exit without making any changes
        #[arg(long)]
        dry_run: bool,
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
```

- [ ] **Step 2: Update the dispatch site**

In `crates/raios-surface-cli/src/cli/mod.rs`, replace:

```rust
        Commands::Bootstrap => new::cmd_bootstrap(),
```

with:

```rust
        Commands::Bootstrap { dry_run, yes } => {
            new::cmd_bootstrap(&cfg.bootstrap, dry_run, yes)
        }
```

- [ ] **Step 3: Replace `cmd_bootstrap` and delete the old personal-data code**

In `crates/raios-surface-cli/src/cli/new.rs`, delete the entire existing `cmd_bootstrap` function body (lines 92–208 in the pre-change file: from `pub(super) fn cmd_bootstrap() {` through its closing `}`), delete the `copy_dir_recursive` function (lines 210–225, now moved to `raios-runtime`), and delete the `DEFAULT_MASTER_MD` constant (lines 227–243). Replace all three with:

```rust
pub(super) fn cmd_bootstrap(cfg: &raios_core::config::BootstrapConfig, dry_run: bool, yes: bool) {
    let plan = raios_runtime::bootstrap::build_plan(cfg);

    if plan.is_empty() {
        println!("Nothing configured — see [bootstrap] in ~/.config/raios/config.toml");
        return;
    }

    println!("raios bootstrap plan ({} action(s)):", plan.len());
    for (i, action) in plan.iter().enumerate() {
        println!("  {}. {}", i + 1, action.describe());
    }
    println!();

    if dry_run {
        println!("(dry run — nothing executed)");
        return;
    }

    if !yes {
        print!("Proceed with {} action(s)? [y/N] ", plan.len());
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err()
            || !matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
        {
            println!("Aborted.");
            return;
        }
    }

    let results = raios_runtime::bootstrap::execute(&plan);

    println!();
    let mut failed = 0;
    for (desc, outcome) in &results {
        match outcome {
            raios_runtime::bootstrap::ActionOutcome::Ok => println!("  [ok] {desc}"),
            raios_runtime::bootstrap::ActionOutcome::Skipped(reason) => {
                println!("  [skipped: {reason}] {desc}")
            }
            raios_runtime::bootstrap::ActionOutcome::Failed(reason) => {
                println!("  [failed: {reason}] {desc}");
                failed += 1;
            }
        }
    }
    println!();
    if failed == 0 {
        println!("Bootstrap complete.");
    } else {
        println!("Bootstrap completed with {failed} failure(s).");
        std::process::exit(1);
    }
}
```

Note the control flow: `execute()` is only ever reached after both the `plan.is_empty()` and `dry_run` early returns, and (when `!yes`) only after the user typed `y`/`yes` — this is the structural "no I/O on dry-run/empty-config" guarantee called for in the design spec, verifiable by inspection rather than a runtime test.

- [ ] **Step 4: Check whether `new.rs`'s `use std::path::Path;` import is still needed**

`cmd_new` (line 8) still uses `dev_ops: &Path`, so the import stays. No action needed — just confirm via the build in the next step that nothing is now unused.

- [ ] **Step 5: Build and fix any fallout**

Run: `cargo build --workspace 2>&1 | tail -60`
Expected: clean build. If `dirs` is now an unused import/dependency in `raios-surface-cli/src/cli/new.rs` (it was only used for `dirs::home_dir()` in the deleted code), remove any now-unused `use` line the compiler flags — `cmd_new`/`cmd_task` don't use it.

- [ ] **Step 6: Run the full workspace test suite**

Run: `cargo test --workspace --lib 2>&1 | grep -E "^test result:|FAILED|^error"`
Expected: every crate reports `test result: ok`, 0 failed, matching (or exceeding, with the new tests) the pre-change baseline of 821 passed.

- [ ] **Step 7: Run clippy across the whole workspace**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 | tail -60`
Expected: clean, 0 warnings.

- [ ] **Step 8: Manual smoke test — empty config is a safe no-op**

At this point in the plan, `~/.config/raios/config.toml` has no `[bootstrap]` section yet (Task 6, which adds one, hasn't run).

Run: `cargo run --release --bin raios -- bootstrap --dry-run 2>&1`
Expected: `Nothing configured — see [bootstrap] in ~/.config/raios/config.toml`, exit code 0, no other output.

- [ ] **Step 9: Commit**

```bash
git add crates/raios-surface-cli/src/cli/args.rs crates/raios-surface-cli/src/cli/mod.rs crates/raios-surface-cli/src/cli/new.rs
git commit -m "feat(bootstrap): gate execution behind plan+confirm, remove hardcoded personal data"
```

---

### Task 5: Fix the stale `Dev_Ops_New` doc-comment example

**Files:**
- Modify: `crates/raios-runtime/src/cortex/mod.rs`

**Interfaces:** None — doc-comment only, no code behavior changes.

- [ ] **Step 1: Fix the example path**

In `crates/raios-runtime/src/cortex/mod.rs` line 12, replace:

```rust
//! cortex.index_workspace(Path::new("/path/to/Dev_Ops_New")).unwrap();
```

with:

```rust
//! cortex.index_workspace(Path::new("/path/to/your/workspace")).unwrap();
```

- [ ] **Step 2: Confirm it compiles (doc comments can still break doctests)**

Run: `cargo test -p raios-runtime --doc cortex 2>&1 | tail -20`
Expected: no failures (this is a `//!` module doc, not a runnable doctest, so this should be a no-op check — confirms nothing else references the old string).

- [ ] **Step 3: Commit**

```bash
git add crates/raios-runtime/src/cortex/mod.rs
git commit -m "docs: update stale Dev_Ops_New example path in cortex module docs"
```

---

### Task 6: Migrate Göktuğ's own config so his existing bootstrap workflow is preserved

**Files:**
- Modify (outside the git repo — Göktuğ's live machine config): `~/.config/raios/config.toml`

**Interfaces:** None — this is a data migration on one machine, not a code change.

- [ ] **Step 1: Read the current live config file**

Run: `cat ~/.config/raios/config.toml`
Note its exact current content so the next step appends rather than clobbers.

- [ ] **Step 2: Append the `[bootstrap]` section reproducing the old hardcoded defaults**

Append the following block to the end of `~/.config/raios/config.toml` (using a text edit, not a blind overwrite — preserve every existing key from Step 1):

```toml
[bootstrap]
global_npm_tools = ["sigmap", "ctx7", "vercel", "firebase-tools"]
enable_claude_plugins = [
  "superpowers@claude-plugins-official",
  "context7@claude-plugins-official",
  "frontend-design@claude-plugins-official",
  "github@claude-plugins-official",
]

[[bootstrap.claude_marketplaces]]
url = "https://github.com/josstei/maestro-orchestrate.git"
plugins = ["maestro@maestro-orchestrator"]

[[bootstrap.claude_marketplaces]]
url = "https://github.com/affaan-m/everything-claude-code.git"
plugins = ["everything-claude-code@everything-claude-code"]

[[bootstrap.rule_sync_repos]]
git_url = "https://github.com/affaan-m/everything-claude-code.git"
targets = ["~/.claude/rules", "~/.antigravity/rules"]
```

Note: the old code also created an empty `~/.config/opencode` directory with no content ever copied into it (dead weight — `copy_dir_recursive` was only ever called for `claude_rules` and `antigravity_rules`). This migration intentionally does not reproduce that no-op directory creation.

Note: the old hardcoded `DEFAULT_MASTER_MD` write to `~/Documents/Obsidian Vaults/Vault101/MASTER.md` is intentionally **not** reproduced here — see the design spec's "`DEFAULT_MASTER_MD` / Vault101 removal" section.

- [ ] **Step 3: Verify the dry-run plan matches the old behavior**

Run: `cargo run --release --bin raios -- bootstrap --dry-run`
Expected: a plan with **11 actions total** — 4 `InstallNpmTool` (one per `global_npm_tools` entry) + 2 `AddMarketplaceAndInstall` (one per `claude_marketplaces` entry) + 1 `SyncRules` (the single `rule_sync_repos` entry, listing both targets) + 4 `EnablePlugin` (one per `enable_claude_plugins` entry) — each line matching what the pre-change `cmd_bootstrap` used to do silently.

- [ ] **Step 4: Confirm a real run reproduces the old behavior**

Run: `cargo run --release --bin raios -- bootstrap` and answer `y` at the prompt.
Expected: same 11 actions execute; tools already installed report `[skipped: "<name>" already on PATH]` (most will already be present from prior manual/old-bootstrap runs); exit code `0`.

- [ ] **Step 5: Reinstall the release binary so `raios` on PATH reflects this change**

Run: `cd /home/alaz/dev/core/R-AI-OS && ./install.sh`
Expected: `~/.local/bin/raios` is rebuilt and replaced; `raios bootstrap --dry-run` from any directory now shows the same 11-action plan.

(No git commit for this task — it modifies only the live `~/.config/raios/config.toml`, not the repository.)

---

## Self-Review Notes

- **Spec coverage:** config schema (Task 1) → plan-then-confirm flow with `--dry-run`/`--yes`/default-No (Task 4) → skipped/failed reporting and exit codes (Tasks 3–4) → `DEFAULT_MASTER_MD`/Vault101 removal while keeping `enable_claude_plugins` (Task 4) → stale doc-comment fix (Task 5) → migration preserving Göktuğ's workflow (Task 6) → tests (build_plan purity, no-I/O structural guarantee, Goktug/Vault101 regression — Task 2). Every spec section maps to a task.
- **Placeholder scan:** no TBD/TODO; every step has complete code, not descriptions of code.
- **Type consistency:** `BootstrapConfig`/`ClaudeMarketplace`/`RuleSyncRepo` (Task 1) match the field names used in `build_plan` (Task 2) and the migration TOML (Task 6). `BootstrapAction`/`ActionOutcome` (Tasks 2–3) match the match arms in `cmd_bootstrap` (Task 4) exactly.
