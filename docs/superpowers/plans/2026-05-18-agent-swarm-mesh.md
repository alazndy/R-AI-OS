# Plan 2: Phase 5 — Agent Swarm Mesh (Git Worktree Isolation)

> **Subagent-Driven Development** ile execute et. Tessera'dan ilham alındı.

**Goal:** Birden fazla ajan aynı anda farklı özellikleri çakışmasız geliştirebilsin. Her görev için izole Git worktree oluşturulur, tamamlanınca merge review yapılır.

**Architecture:**
```
raios swarm "feat: dark mode" --agent claude
    → SwarmCoordinator
    → create_worktree(~/.raios/worktrees/<proj>/<uuid>/)
    → spawn_agent_in_worktree()
    → diff_review → merge veya discard
```

**Tech Stack:** Rust, `git2 = "0.19"`, mevcut `ExecutionProxy`, `DaemonState`

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/swarm/mod.rs` | `SwarmTask`, `SwarmStatus` |
| `src/swarm/worktree.rs` | `create_worktree`, `remove_worktree` |
| `src/swarm/merge.rs` | `diff_summary`, `merge_branch`, `delete_branch` |
| `src/daemon/swarm.rs` | `SwarmWorker` for aiosd |
| `src/daemon/state.rs` | `active_swarms: Vec<SwarmTask>` |
| `src/daemon/server.rs` | Swarm RPC endpoints |
| `src/cli.rs` | `Commands::Swarm`, `SwarmList`, `SwarmReview` |
| `Cargo.toml` | `git2 = "0.19"` |

---

## Task 1: `SwarmTask` struct + module scaffold

**Files:** `Cargo.toml`, `src/swarm/mod.rs`, `src/lib.rs`

- [ ] `Cargo.toml`'a: `git2 = "0.19"`
- [ ] `src/lib.rs`'e: `pub mod swarm;`
- [ ] `src/swarm/mod.rs`:
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
- [ ] `cargo check` → temiz
- [ ] Commit: `feat(swarm): SwarmTask struct + module scaffold`

---

## Task 2: Worktree yönetimi

**Files:** `src/swarm/worktree.rs`

- [ ] `create_worktree(project_path, task_id, task_slug) -> Result<(PathBuf, String)>`:
  - Branch: `swarm/<uuid-short>-<slug>`
  - Path: `~/.raios/worktrees/<project>/<uuid>/`
  - `git worktree add -b <branch> <path> HEAD` çalıştır

- [ ] `remove_worktree(project_path, worktree_path) -> Result<()>`:
  - `git worktree remove --force <path>`
  - `git worktree prune`

- [ ] `cargo check` → temiz
- [ ] Commit: `feat(swarm): worktree create/remove helpers`

---

## Task 3: Merge flow

**Files:** `src/swarm/merge.rs`

- [ ] `diff_summary(project_path, branch) -> Result<DiffSummary>`:
  - `git diff --stat HEAD...<branch>` → parse files/insertions/deletions
  - `git diff HEAD...<branch>` → full diff text

- [ ] `merge_branch(project_path, branch, msg) -> Result<()>`:
  - `git merge --no-ff <branch> -m <msg>`

- [ ] `delete_branch(project_path, branch) -> Result<()>`

- [ ] Commit: `feat(swarm): diff_summary, merge_branch, delete_branch`

---

## Task 4: CLI — `raios swarm` + `swarm-list` + `swarm-review`

**Files:** `src/cli.rs`

- [ ] `Commands` enum'a:
```rust
/// Spawn agent in isolated Git worktree
Swarm {
    task: String,
    #[arg(short, long, default_value = "claude")]
    agent: String,
    #[arg(short, long)]
    project: Option<String>,
},
/// List active swarm tasks
SwarmList,
/// Review and merge or reject a swarm task
SwarmReview { id: String },
```

- [ ] `cmd_swarm()`: worktree oluştur → agent spawn → bekle/arka plan
- [ ] `cmd_swarm_list()`: aktif SwarmTask'ları listele
- [ ] `cmd_swarm_review()`: diff göster → y/n → merge veya reject
- [ ] Commit: `feat(cli): raios swarm/swarm-list/swarm-review`

---

## Task 5: Daemon `SwarmWorker` + MCP tools

**Files:** `src/daemon/swarm.rs`, `src/daemon/state.rs`, `src/daemon/server.rs`

- [ ] `DaemonState`'e: `active_swarms: Vec<SwarmTask>`
- [ ] RPC endpoints: `GetSwarmStatus`, `ApproveSwarm { id }`, `RejectSwarm { id }`
- [ ] MCP tools: `get_swarm_status`, `approve_swarm`, `reject_swarm`
- [ ] VS Code extension — swarm notification hook (DiffInboxProvider'ı extend et)
- [ ] Commit: `feat(daemon,mcp): SwarmWorker + swarm MCP tools`
