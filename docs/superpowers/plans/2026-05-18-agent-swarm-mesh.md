# Plan 2: Phase 5 — Agent Swarm Mesh (Git Worktree Isolation)

> **For agentic workers:** Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** Birden fazla ajan aynı anda farklı branch'lerde çakışmasız çalışsın. `raios swarm "task"` → izole worktree → ajan → review → merge.

**Architecture:** `src/swarm/` modülü: worktree CRUD + merge flow. `DaemonState`'e `active_swarms` eklenir. CLI: `swarm`, `swarm-list`, `swarm-review`. MCP tools: `get_swarm_status`, `approve_swarm`, `reject_swarm`.

**Tech Stack:** Rust, `git2 = "0.19"`, mevcut `ExecutionProxy`, `DaemonState`, `uuid`

**Mevcut durum:**
- `DaemonState` (state.rs satır 48-61): `active_agents`, `pending_diffs`, `pending_file_changes` var — `active_swarms` yok
- Server dispatch: `v["command"]` string matching, satır 200-324
- `dispatch_to_agent` (tasks.rs): clipboard + Windows Terminal launch

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `Cargo.toml` | `git2 = "0.19"` |
| `src/lib.rs` | `pub mod swarm;` |
| `src/swarm/mod.rs` | `SwarmTask`, `SwarmStatus` struct |
| `src/swarm/worktree.rs` | `create_worktree`, `remove_worktree` |
| `src/swarm/merge.rs` | `diff_summary`, `merge_branch`, `delete_branch` |
| `src/daemon/state.rs` | `active_swarms: Vec<SwarmTask>` alanı |
| `src/daemon/server.rs` | `GetSwarmStatus`, `ApproveSwarm`, `RejectSwarm` |
| `src/cli.rs` | `Commands::Swarm`, `SwarmList`, `SwarmReview` |

---

## Task 1: Bağımlılık + Modül Scaffold

**Files:** `Cargo.toml`, `src/lib.rs`, `src/swarm/mod.rs`

- [ ] **Step 1.1: `Cargo.toml`'a `git2` ekle**

`[dependencies]` bloğuna:
```toml
git2 = "0.19"
```

- [ ] **Step 1.2: `src/lib.rs`'e `pub mod swarm;` ekle**

Mevcut `pub mod` satırları arasına (alfabetik sırayla):
```rust
pub mod swarm;
```

- [ ] **Step 1.3: `src/swarm/mod.rs` oluştur**

```rust
pub mod merge;
pub mod worktree;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: Uuid,
    pub project_name: String,
    pub project_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub task_description: String,
    pub agent: String,
    pub status: SwarmStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SwarmStatus {
    Initializing,
    Running,
    AwaitingReview,
    Merged,
    Rejected,
    Failed(String),
}
```

- [ ] **Step 1.4: `cargo check`**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 1.5: Commit**

```bash
git add Cargo.toml src/lib.rs src/swarm/mod.rs
git commit -m "feat(swarm): SwarmTask struct + module scaffold + git2 dep"
```

---

## Task 2: Worktree Yönetimi

**Files:** `src/swarm/worktree.rs`

- [ ] **Step 2.1: `src/swarm/worktree.rs` oluştur**

```rust
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

/// Create an isolated Git worktree for a swarm task.
/// Returns (worktree_path, branch_name).
pub fn create_worktree(
    project_path: &Path,
    task_id: Uuid,
    task_description: &str,
) -> Result<(PathBuf, String)> {
    let slug = make_slug(task_description);
    let branch = format!("swarm/{}-{}", &task_id.to_string()[..8], slug);

    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let worktree_base = home
        .join(".raios")
        .join("worktrees")
        .join(project_path.file_name().unwrap_or_default());
    let worktree_path = worktree_base.join(task_id.to_string());

    std::fs::create_dir_all(&worktree_base)
        .context("Failed to create worktree base directory")?;

    let status = Command::new("git")
        .args([
            "worktree", "add",
            "-b", &branch,
            worktree_path.to_str().unwrap_or("."),
            "HEAD",
        ])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree add")?;

    anyhow::ensure!(status.success(), "git worktree add failed with status: {}", status);

    Ok((worktree_path, branch))
}

/// Remove a worktree and prune stale entries.
pub fn remove_worktree(project_path: &Path, worktree_path: &Path) -> Result<()> {
    Command::new("git")
        .args([
            "worktree", "remove",
            "--force",
            worktree_path.to_str().unwrap_or("."),
        ])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree remove")?;

    Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(project_path)
        .status()
        .context("Failed to run git worktree prune")?;

    Ok(())
}

/// List all active worktrees for a project.
pub fn list_worktrees(project_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_path)
        .output()
        .context("Failed to run git worktree list")?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let paths: Vec<String> = text
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .map(|l| l["worktree ".len()..].to_string())
        .collect();

    Ok(paths)
}

fn make_slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_replaces_spaces() {
        assert_eq!(make_slug("feat: add dark mode"), "feat-add-dark-mode");
    }

    #[test]
    fn slug_truncates_long_descriptions() {
        let long = "implement full authentication system with oauth and jwt";
        let slug = make_slug(long);
        assert!(slug.split('-').count() <= 5);
    }
}
```

- [ ] **Step 2.2: `cargo test --lib swarm::worktree`**

```bash
cargo test --lib swarm::worktree::tests -- --nocapture
```

Beklenen: `test result: ok. 2 passed`

- [ ] **Step 2.3: Commit**

```bash
git add src/swarm/worktree.rs
git commit -m "feat(swarm): worktree create/remove/list helpers"
```

---

## Task 3: Merge Flow

**Files:** `src/swarm/merge.rs`

- [ ] **Step 3.1: `src/swarm/merge.rs` oluştur**

```rust
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub diff_text: String,
}

/// Get a summary of changes between HEAD and the swarm branch.
pub fn diff_summary(project_path: &Path, branch: &str) -> Result<DiffSummary> {
    // Stat summary
    let stat = Command::new("git")
        .args(["diff", "--stat", &format!("HEAD...{branch}")])
        .current_dir(project_path)
        .output()
        .context("git diff --stat failed")?;

    let stat_text = String::from_utf8_lossy(&stat.stdout).to_string();
    let (files, ins, del) = parse_stat(&stat_text);

    // Full diff
    let diff = Command::new("git")
        .args(["diff", &format!("HEAD...{branch}")])
        .current_dir(project_path)
        .output()
        .context("git diff failed")?;

    Ok(DiffSummary {
        files_changed: files,
        insertions: ins,
        deletions: del,
        diff_text: String::from_utf8_lossy(&diff.stdout).to_string(),
    })
}

/// Merge the swarm branch into HEAD using --no-ff.
pub fn merge_branch(project_path: &Path, branch: &str, message: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["merge", "--no-ff", branch, "-m", message])
        .current_dir(project_path)
        .status()
        .context("git merge failed")?;

    anyhow::ensure!(status.success(), "git merge exited with status: {}", status);
    Ok(())
}

/// Delete a local branch (force).
pub fn delete_branch(project_path: &Path, branch: &str) -> Result<()> {
    Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(project_path)
        .status()
        .context("git branch -D failed")?;
    Ok(())
}

/// Parse "N files changed, X insertions(+), Y deletions(-)" from git diff --stat output.
fn parse_stat(text: &str) -> (usize, usize, usize) {
    let last = text.lines().last().unwrap_or("");
    (
        extract_num(last, "file"),
        extract_num(last, "insertion"),
        extract_num(last, "deletion"),
    )
}

fn extract_num(s: &str, keyword: &str) -> usize {
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        if word.contains(keyword) && i > 0 {
            if let Ok(n) = words[i - 1].parse::<usize>() {
                return n;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_extracts_numbers() {
        let text = " 3 files changed, 45 insertions(+), 2 deletions(-)";
        let (f, i, d) = parse_stat(text);
        assert_eq!(f, 3);
        assert_eq!(i, 45);
        assert_eq!(d, 2);
    }

    #[test]
    fn parse_stat_zero_deletions() {
        let text = " 1 file changed, 10 insertions(+)";
        let (f, i, d) = parse_stat(text);
        assert_eq!(f, 1);
        assert_eq!(i, 10);
        assert_eq!(d, 0);
    }
}
```

- [ ] **Step 3.2: `cargo test --lib swarm::merge`**

```bash
cargo test --lib swarm::merge::tests -- --nocapture
```

Beklenen: 2 test geçiyor.

- [ ] **Step 3.3: Commit**

```bash
git add src/swarm/merge.rs
git commit -m "feat(swarm): diff_summary, merge_branch, delete_branch + tests"
```

---

## Task 4: `DaemonState` + Server Endpoints

**Files:** `src/daemon/state.rs`, `src/daemon/server.rs`

- [ ] **Step 4.1: `DaemonState`'e `active_swarms` ekle**

`src/daemon/state.rs` satır 48-61'deki `DaemonState` struct'ına, `pending_diffs` satırından sonra:

```rust
    pub active_swarms: Vec<crate::swarm::SwarmTask>,
```

`Default` impl'da (veya mevcut `Default` derive'ı kullanılıyorsa): `SwarmTask` `Serialize+Deserialize` implement ettiği için `Default` derive yeterli — `active_swarms: vec![]` otomatik gelir.

- [ ] **Step 4.2: Server'a 3 yeni RPC endpoint ekle**

`src/daemon/server.rs`'deki `else if v["command"] == "RejectDiff"` bloğundan sonra:

```rust
else if v["command"] == "GetSwarmStatus" {
    let s = state_for_client.read().await;
    let response = serde_json::json!({
        "event": "SwarmStatus",
        "swarms": s.active_swarms
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}
else if v["command"] == "ApproveSwarm" {
    let swarm_id_str = v["id"].as_str().unwrap_or("").to_string();
    let swarm_id = match uuid::Uuid::parse_str(&swarm_id_str) {
        Ok(id) => id,
        Err(_) => {
            let response = serde_json::json!({"event":"SwarmError","error":"invalid UUID"});
            let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
            return;
        }
    };
    let mut s = state_for_client.write().await;
    if let Some(pos) = s.active_swarms.iter().position(|t| t.id == swarm_id) {
        let task = &s.active_swarms[pos];
        let result = crate::swarm::merge::merge_branch(
            &task.project_path,
            &task.branch_name,
            &format!("swarm: {}", task.task_description),
        );
        let _ = crate::swarm::worktree::remove_worktree(
            &task.project_path.clone(),
            &task.worktree_path.clone(),
        );
        let _ = crate::swarm::merge::delete_branch(
            &task.project_path.clone(),
            &task.branch_name.clone(),
        );
        s.active_swarms.remove(pos);
        drop(s);
        let response = match result {
            Ok(()) => serde_json::json!({"event":"SwarmApproved","id":swarm_id_str}),
            Err(e) => serde_json::json!({"event":"SwarmError","error":e.to_string()}),
        };
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    } else {
        drop(s);
        let response = serde_json::json!({"event":"SwarmError","error":"swarm not found"});
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    }
}
else if v["command"] == "RejectSwarm" {
    let swarm_id_str = v["id"].as_str().unwrap_or("").to_string();
    let swarm_id = match uuid::Uuid::parse_str(&swarm_id_str) {
        Ok(id) => id,
        Err(_) => {
            let response = serde_json::json!({"event":"SwarmError","error":"invalid UUID"});
            let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
            return;
        }
    };
    let mut s = state_for_client.write().await;
    if let Some(pos) = s.active_swarms.iter().position(|t| t.id == swarm_id) {
        let task = &s.active_swarms[pos];
        let _ = crate::swarm::worktree::remove_worktree(
            &task.project_path.clone(),
            &task.worktree_path.clone(),
        );
        let _ = crate::swarm::merge::delete_branch(
            &task.project_path.clone(),
            &task.branch_name.clone(),
        );
        s.active_swarms.remove(pos);
    }
    drop(s);
    let response = serde_json::json!({"event":"SwarmRejected","id":swarm_id_str});
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}
```

- [ ] **Step 4.3: `cargo check`**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 4.4: Commit**

```bash
git add src/daemon/state.rs src/daemon/server.rs
git commit -m "feat(daemon): active_swarms state + GetSwarmStatus/ApproveSwarm/RejectSwarm RPC"
```

---

## Task 5: CLI — `raios swarm` + `swarm-list` + `swarm-review`

**Files:** `src/cli.rs`

- [ ] **Step 5.1: `Commands` enum'a 3 varyant ekle**

Mevcut son varyanttan sonra:

```rust
/// Spawn an agent in an isolated Git worktree for conflict-free parallel work
Swarm {
    /// Task description (what the agent should do)
    task: String,
    /// Agent to use: claude | gemini | codex
    #[arg(short, long, default_value = "claude")]
    agent: String,
    /// Project name or path (default: current directory)
    #[arg(short, long)]
    project: Option<String>,
},
/// List all active swarm tasks
SwarmList,
/// Review a swarm task diff and merge or reject it
SwarmReview {
    /// Swarm task ID (from raios swarm-list)
    id: String,
},
```

- [ ] **Step 5.2: Match arms ekle**

`match cmd` bloğuna:

```rust
Commands::Swarm { task, agent, project } => {
    cmd_swarm(&task, &agent, project, &cfg.dev_ops_path, cli.json);
}
Commands::SwarmList => {
    cmd_swarm_list(&cfg.dev_ops_path, cli.json);
}
Commands::SwarmReview { id } => {
    cmd_swarm_review(&id, &cfg.dev_ops_path, cli.json);
}
```

- [ ] **Step 5.3: `cmd_swarm()` fonksiyonu ekle**

```rust
fn cmd_swarm(
    task: &str,
    agent: &str,
    project: Option<String>,
    dev_ops: &std::path::Path,
    _json: bool,
) {
    use crate::swarm::{worktree, SwarmStatus, SwarmTask};

    let projects = crate::entities::load_entities(dev_ops);
    let proj = if let Some(ref name) = project {
        let n = name.to_lowercase();
        projects.into_iter().find(|p| p.name.to_lowercase().contains(&n))
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        projects.into_iter().find(|p| p.local_path == cwd)
    };

    let proj = match proj {
        Some(p) => p,
        None => {
            eprintln!("Project not found. Try: raios swarm \"task\" --project <name>");
            std::process::exit(1);
        }
    };

    let task_id = uuid::Uuid::new_v4();
    println!("Creating isolated worktree for task: {}", task);
    println!("Project: {} | Agent: {} | ID: {}", proj.name, agent, task_id);

    match worktree::create_worktree(&proj.local_path, task_id, task) {
        Ok((wt_path, branch)) => {
            println!("Worktree created: {}", wt_path.display());
            println!("Branch: {}", branch);
            println!();

            // Launch agent in worktree
            let agent_task = crate::tasks::Task {
                text: task.to_string(),
                completed: false,
                agent: Some(agent.to_string()),
                project: Some(proj.name.clone()),
            };
            let result = crate::tasks::dispatch_to_agent(
                &agent_task,
                agent,
                Some(&wt_path),
                None,
            );
            println!("{}", result);
            println!();
            println!("When done, run: raios swarm-review  (ID: {})", task_id);
        }
        Err(e) => {
            eprintln!("Failed to create worktree: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 5.4: `cmd_swarm_list()` fonksiyonu ekle**

```rust
fn cmd_swarm_list(dev_ops: &std::path::Path, json: bool) {
    // List worktrees for all projects
    let projects = crate::entities::load_entities(dev_ops);
    let raios_wt_base = dirs::home_dir()
        .unwrap_or_default()
        .join(".raios")
        .join("worktrees");

    if !raios_wt_base.exists() {
        if json {
            println!("[]");
        } else {
            println!("No active swarm tasks.");
        }
        return;
    }

    let mut found = false;
    for proj in &projects {
        let wt_dir = raios_wt_base.join(&proj.name);
        if !wt_dir.exists() { continue; }
        if let Ok(entries) = std::fs::read_dir(&wt_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if !entry.path().is_dir() { continue; }
                let id = entry.file_name().to_string_lossy().to_string();
                let branch_output = std::process::Command::new("git")
                    .args(["branch", "--show-current"])
                    .current_dir(&entry.path())
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if !json {
                    println!("ID: {}  Project: {}  Branch: {}  Path: {}",
                        &id[..8], proj.name, branch_output, entry.path().display());
                }
                found = true;
            }
        }
    }
    if !found {
        if json { println!("[]"); } else { println!("No active swarm tasks."); }
    }
}
```

- [ ] **Step 5.5: `cmd_swarm_review()` fonksiyonu ekle**

```rust
fn cmd_swarm_review(id: &str, dev_ops: &std::path::Path, _json: bool) {
    use crate::swarm::{merge, worktree};

    // Find worktree by ID prefix
    let raios_wt_base = dirs::home_dir()
        .unwrap_or_default()
        .join(".raios")
        .join("worktrees");

    let projects = crate::entities::load_entities(dev_ops);
    let mut found_wt: Option<(std::path::PathBuf, std::path::PathBuf, String)> = None;

    'outer: for proj in &projects {
        let wt_dir = raios_wt_base.join(&proj.name);
        if !wt_dir.exists() { continue; }
        if let Ok(entries) = std::fs::read_dir(&wt_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let entry_id = entry.file_name().to_string_lossy().to_string();
                if entry_id.starts_with(id) {
                    let branch = std::process::Command::new("git")
                        .args(["branch", "--show-current"])
                        .current_dir(&entry.path())
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default();
                    found_wt = Some((proj.local_path.clone(), entry.path(), branch));
                    break 'outer;
                }
            }
        }
    }

    let (project_path, worktree_path, branch) = match found_wt {
        Some(f) => f,
        None => {
            eprintln!("Swarm task not found: {}. Run: raios swarm-list", id);
            std::process::exit(1);
        }
    };

    // Show diff summary
    match merge::diff_summary(&project_path, &branch) {
        Ok(summary) => {
            println!("Diff: {} files changed, +{} -{}", 
                summary.files_changed, summary.insertions, summary.deletions);
            if !summary.diff_text.is_empty() {
                let lines: Vec<&str> = summary.diff_text.lines().take(30).collect();
                println!("{}", lines.join("\n"));
                if summary.diff_text.lines().count() > 30 {
                    println!("... ({} more lines)", summary.diff_text.lines().count() - 30);
                }
            }
        }
        Err(e) => eprintln!("Could not get diff: {e}"),
    }

    print!("\nMerge this branch? [y/N]: ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);

    if input.trim().to_lowercase() == "y" {
        match merge::merge_branch(&project_path, &branch, &format!("swarm: {}", branch)) {
            Ok(()) => {
                println!("Merged successfully.");
                let _ = worktree::remove_worktree(&project_path, &worktree_path);
                let _ = merge::delete_branch(&project_path, &branch);
                println!("Worktree and branch cleaned up.");
            }
            Err(e) => eprintln!("Merge failed: {e}"),
        }
    } else {
        println!("Rejected. Worktree preserved at: {}", worktree_path.display());
        println!("To discard: git worktree remove --force {}", worktree_path.display());
    }
}
```

- [ ] **Step 5.6: `cargo build --bin raios`**

```bash
cargo build --bin raios 2>&1 | tail -5
```

Beklenen: `Finished` satırı.

- [ ] **Step 5.7: Smoke test**

```bash
cargo run --bin raios -- swarm-list 2>&1
```

Beklenen: `No active swarm tasks.`

- [ ] **Step 5.8: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | grep "test result"
```

- [ ] **Step 5.9: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios swarm/swarm-list/swarm-review commands"
```
