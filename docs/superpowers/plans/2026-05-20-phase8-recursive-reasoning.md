# Phase 8: Recursive Reasoning — Task Decomposition DAG

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let agents submit a directed acyclic graph (DAG) of tasks with dependencies, and have R-AI-OS execute them in dependency order — running independent nodes in parallel, feeding results of completed nodes into dependent ones.

**Architecture:** A new `TaskGraph` module manages DAG state in SQLite. The agent submits a graph via `CreateTaskGraph { nodes: [{id, description, shell_cmd, deps: [id]}] }`. R-AI-OS executes leaf nodes first using `Factory::submit()`, watches for completions on the broadcast channel, and unlocks downstream nodes. Hard limits: max 50 nodes per graph, max depth 5, execution timeout 10 minutes per node. The graph's final status is queryable via `GetTaskGraph`.

**Tech Stack:** `crate::factory::{Factory, Job}`, `rusqlite`, `tokio::sync::broadcast`, no new dependencies.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Create | `src/task_graph.rs` | `TaskGraph`, `GraphNode`, `GraphStore` — DAG state + SQLite persistence |
| Modify | `src/db.rs` | Add `task_graphs` + `task_graph_nodes` tables |
| Modify | `src/lib.rs` | `pub mod task_graph;` |
| Modify | `src/daemon/server.rs` | `CreateTaskGraph`, `ExecuteTaskGraph`, `GetTaskGraph` TCP commands |

---

### Task 1: Add task graph tables to SQLite

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Add tables inside `migrate()`**

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS task_graphs (
        id          TEXT PRIMARY KEY,
        goal        TEXT NOT NULL,
        agent       TEXT NOT NULL,
        status      TEXT NOT NULL DEFAULT 'pending',
        created_at  TEXT NOT NULL DEFAULT (datetime('now')),
        completed_at TEXT
    );

    CREATE TABLE IF NOT EXISTS task_graph_nodes (
        id          TEXT NOT NULL,
        graph_id    TEXT NOT NULL REFERENCES task_graphs(id) ON DELETE CASCADE,
        description TEXT NOT NULL,
        shell_cmd   TEXT NOT NULL,
        deps        TEXT NOT NULL DEFAULT '[]',
        status      TEXT NOT NULL DEFAULT 'pending',
        factory_job_id TEXT,
        result      TEXT,
        error       TEXT,
        PRIMARY KEY (graph_id, id)
    );
    CREATE INDEX IF NOT EXISTS idx_tgn_graph ON task_graph_nodes(graph_id);
    ",
)?;
```

- [ ] **Step 2: Write test**

```rust
#[test]
fn task_graph_tables_exist() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_graphs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 3: Run test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test db::tests::task_graph_tables_exist
```

Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add src/db.rs
git commit -m "feat(graph): add task_graphs and task_graph_nodes tables to SQLite"
```

---

### Task 2: Implement `src/task_graph.rs`

**Files:**
- Create: `src/task_graph.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `src/task_graph.rs` with only the test block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_store(tmp: &tempfile::TempDir) -> GraphStore {
        GraphStore::new(tmp.path().join("test.db"))
    }

    #[test]
    fn create_graph_and_list_nodes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);

        let nodes = vec![
            NodeSpec { id: "a".into(), description: "build".into(), shell_cmd: "cargo build".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "test".into(), shell_cmd: "cargo test".into(), deps: vec!["a".into()] },
        ];
        let graph_id = store.create("build and test", "claude", nodes).unwrap();
        let graph = store.get(&graph_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn ready_nodes_excludes_nodes_with_pending_deps() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);

        let nodes = vec![
            NodeSpec { id: "a".into(), description: "first".into(), shell_cmd: "echo a".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "second".into(), shell_cmd: "echo b".into(), deps: vec!["a".into()] },
        ];
        let gid = store.create("goal", "claude", nodes).unwrap();
        let ready = store.ready_nodes(&gid);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");
    }

    #[test]
    fn node_count_enforces_max_limit() {
        let too_many: Vec<NodeSpec> = (0..51)
            .map(|i| NodeSpec {
                id: i.to_string(),
                description: format!("task {i}"),
                shell_cmd: "echo x".into(),
                deps: vec![],
            })
            .collect();
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let result = store.create("too big", "claude", too_many);
        assert!(result.is_err());
    }

    #[test]
    fn mark_node_complete_unlocks_dependent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);

        let nodes = vec![
            NodeSpec { id: "a".into(), description: "a".into(), shell_cmd: "echo a".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "b".into(), shell_cmd: "echo b".into(), deps: vec!["a".into()] },
        ];
        let gid = store.create("test", "claude", nodes).unwrap();
        store.mark_node_complete(&gid, "a", "ok", "job-1");
        let ready = store.ready_nodes(&gid);
        assert_eq!(ready[0].id, "b");
    }
}
```

- [ ] **Step 2: Run — confirm compile error**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test task_graph::tests 2>&1 | Select-Object -Last 5
```

Expected: compile error

- [ ] **Step 3: Implement `src/task_graph.rs`**

```rust
//! Recursive Reasoning — Task Decomposition DAG
//!
//! Agents submit a directed acyclic graph of tasks. R-AI-OS executes nodes
//! in dependency order, running independent nodes in parallel via Factory Mode.
use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_NODES: usize = 50;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub description: String,
    pub shell_cmd: String,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub graph_id: String,
    pub description: String,
    pub shell_cmd: String,
    pub deps: Vec<String>,
    pub status: NodeStatus,
    pub factory_job_id: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NodeStatus::Pending => "pending",
            NodeStatus::Running => "running",
            NodeStatus::Completed => "completed",
            NodeStatus::Failed => "failed",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub agent: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub nodes: Vec<GraphNode>,
}

// ─── GraphStore ───────────────────────────────────────────────────────────────

pub struct GraphStore {
    db_path: PathBuf,
}

impl GraphStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        let db_path = db_path.into();
        let store = Self { db_path };
        store.ensure_tables();
        store
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("workspace.db")
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn ensure_tables(&self) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS task_graphs (
                    id TEXT PRIMARY KEY, goal TEXT NOT NULL, agent TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')), completed_at TEXT
                );
                CREATE TABLE IF NOT EXISTS task_graph_nodes (
                    id TEXT NOT NULL, graph_id TEXT NOT NULL,
                    description TEXT NOT NULL, shell_cmd TEXT NOT NULL,
                    deps TEXT NOT NULL DEFAULT '[]',
                    status TEXT NOT NULL DEFAULT 'pending',
                    factory_job_id TEXT, result TEXT, error TEXT,
                    PRIMARY KEY (graph_id, id)
                );",
            );
        }
    }

    /// Create a new task graph. Returns the graph ID.
    pub fn create(&self, goal: &str, agent: &str, nodes: Vec<NodeSpec>) -> Result<String> {
        if nodes.len() > MAX_NODES {
            bail!("Too many nodes: {} (max {})", nodes.len(), MAX_NODES);
        }

        // Validate depth
        validate_dag(&nodes)?;

        let graph_id = Uuid::new_v4().to_string();
        let conn = self.connect()?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        conn.execute(
            "INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,?2,?3,'pending',?4)",
            params![graph_id, goal, agent, now],
        )?;

        for node in &nodes {
            let deps_json = serde_json::to_string(&node.deps).unwrap_or_else(|_| "[]".into());
            conn.execute(
                "INSERT INTO task_graph_nodes (id, graph_id, description, shell_cmd, deps)
                 VALUES (?1,?2,?3,?4,?5)",
                params![node.id, graph_id, node.description, node.shell_cmd, deps_json],
            )?;
        }

        Ok(graph_id)
    }

    pub fn get(&self, graph_id: &str) -> Option<TaskGraph> {
        let conn = self.connect().ok()?;

        let graph = conn.query_row(
            "SELECT id, goal, agent, status, created_at, completed_at FROM task_graphs WHERE id=?1",
            params![graph_id],
            |row| Ok((
                row.get::<_,String>(0)?, row.get::<_,String>(1)?,
                row.get::<_,String>(2)?, row.get::<_,String>(3)?,
                row.get::<_,String>(4)?, row.get::<_,Option<String>>(5)?,
            )),
        ).ok()?;

        let nodes = self.load_nodes(graph_id, &conn);

        Some(TaskGraph {
            id: graph.0, goal: graph.1, agent: graph.2,
            status: graph.3, created_at: graph.4, completed_at: graph.5,
            nodes,
        })
    }

    fn load_nodes(&self, graph_id: &str, conn: &Connection) -> Vec<GraphNode> {
        let mut stmt = conn.prepare(
            "SELECT id, graph_id, description, shell_cmd, deps, status, factory_job_id, result, error
             FROM task_graph_nodes WHERE graph_id=?1"
        ).ok();

        match &mut stmt {
            Some(s) => s.query_map(params![graph_id], |row| {
                let deps_str: String = row.get(4)?;
                let deps: Vec<String> = serde_json::from_str(&deps_str).unwrap_or_default();
                let status_str: String = row.get(5)?;
                let status = match status_str.as_str() {
                    "running" => NodeStatus::Running,
                    "completed" => NodeStatus::Completed,
                    "failed" => NodeStatus::Failed,
                    _ => NodeStatus::Pending,
                };
                Ok(GraphNode {
                    id: row.get(0)?, graph_id: row.get(1)?,
                    description: row.get(2)?, shell_cmd: row.get(3)?,
                    deps, status,
                    factory_job_id: row.get(6)?, result: row.get(7)?, error: row.get(8)?,
                })
            }).ok().map(|r| r.flatten().collect()).unwrap_or_default(),
            None => vec![],
        }
    }

    /// Return nodes whose dependencies are all completed and which are still pending.
    pub fn ready_nodes(&self, graph_id: &str) -> Vec<GraphNode> {
        let conn = match self.connect() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let all = self.load_nodes(graph_id, &conn);

        let completed_ids: std::collections::HashSet<&str> = all
            .iter()
            .filter(|n| n.status == NodeStatus::Completed)
            .map(|n| n.id.as_str())
            .collect();

        all.into_iter()
            .filter(|n| {
                n.status == NodeStatus::Pending
                    && n.deps.iter().all(|dep| completed_ids.contains(dep.as_str()))
            })
            .collect()
    }

    pub fn mark_node_running(&self, graph_id: &str, node_id: &str, job_id: &str) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET status='running', factory_job_id=?3
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, job_id],
            );
        }
    }

    pub fn mark_node_complete(&self, graph_id: &str, node_id: &str, result: &str, job_id: &str) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET status='completed', result=?3, factory_job_id=?4
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, result, job_id],
            );
            self.maybe_complete_graph(graph_id, &conn);
        }
    }

    pub fn mark_node_failed(&self, graph_id: &str, node_id: &str, error: &str) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET status='failed', error=?3
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, error],
            );
            let _ = conn.execute(
                "UPDATE task_graphs SET status='failed' WHERE id=?1",
                params![graph_id],
            );
        }
    }

    fn maybe_complete_graph(&self, graph_id: &str, conn: &Connection) {
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_graph_nodes
                 WHERE graph_id=?1 AND status NOT IN ('completed','failed')",
                params![graph_id],
                |r| r.get(0),
            )
            .unwrap_or(1);

        if pending == 0 {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let _ = conn.execute(
                "UPDATE task_graphs SET status='completed', completed_at=?2 WHERE id=?1",
                params![graph_id, now],
            );
        }
    }
}

// ─── DAG validation ───────────────────────────────────────────────────────────

fn validate_dag(nodes: &[NodeSpec]) -> Result<()> {
    let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    for node in nodes {
        for dep in &node.deps {
            if !ids.contains(dep.as_str()) {
                bail!("Node '{}' depends on '{}' which is not in the graph", node.id, dep);
            }
            if dep == &node.id {
                bail!("Node '{}' depends on itself", node.id);
            }
        }
    }
    // Simple cycle detection via DFS
    let mut visited = std::collections::HashSet::new();
    let mut in_stack = std::collections::HashSet::new();
    let adj: std::collections::HashMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.deps.iter().map(|d| d.as_str()).collect()))
        .collect();

    for node in nodes {
        if has_cycle(&adj, node.id.as_str(), &mut visited, &mut in_stack) {
            bail!("Task graph contains a cycle involving '{}'", node.id);
        }
    }
    Ok(())
}

fn has_cycle<'a>(
    adj: &std::collections::HashMap<&'a str, Vec<&'a str>>,
    node: &'a str,
    visited: &mut std::collections::HashSet<&'a str>,
    in_stack: &mut std::collections::HashSet<&'a str>,
) -> bool {
    if in_stack.contains(node) { return true; }
    if visited.contains(node) { return false; }
    visited.insert(node);
    in_stack.insert(node);
    if let Some(deps) = adj.get(node) {
        for dep in deps {
            if has_cycle(adj, dep, visited, in_stack) {
                return true;
            }
        }
    }
    in_stack.remove(node);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store(tmp: &tempfile::TempDir) -> GraphStore {
        GraphStore::new(tmp.path().join("test.db"))
    }

    #[test]
    fn create_graph_and_list_nodes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let nodes = vec![
            NodeSpec { id: "a".into(), description: "build".into(), shell_cmd: "cargo build".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "test".into(), shell_cmd: "cargo test".into(), deps: vec!["a".into()] },
        ];
        let graph_id = store.create("build and test", "claude", nodes).unwrap();
        let graph = store.get(&graph_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn ready_nodes_excludes_nodes_with_pending_deps() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let nodes = vec![
            NodeSpec { id: "a".into(), description: "first".into(), shell_cmd: "echo a".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "second".into(), shell_cmd: "echo b".into(), deps: vec!["a".into()] },
        ];
        let gid = store.create("goal", "claude", nodes).unwrap();
        let ready = store.ready_nodes(&gid);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");
    }

    #[test]
    fn node_count_enforces_max_limit() {
        let too_many: Vec<NodeSpec> = (0..51).map(|i| NodeSpec {
            id: i.to_string(), description: format!("task {i}"),
            shell_cmd: "echo x".into(), deps: vec![],
        }).collect();
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        assert!(store.create("too big", "claude", too_many).is_err());
    }

    #[test]
    fn mark_node_complete_unlocks_dependent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let nodes = vec![
            NodeSpec { id: "a".into(), description: "a".into(), shell_cmd: "echo a".into(), deps: vec![] },
            NodeSpec { id: "b".into(), description: "b".into(), shell_cmd: "echo b".into(), deps: vec!["a".into()] },
        ];
        let gid = store.create("test", "claude", nodes).unwrap();
        store.mark_node_complete(&gid, "a", "ok", "job-1");
        let ready = store.ready_nodes(&gid);
        assert_eq!(ready[0].id, "b");
    }

    #[test]
    fn cycle_detection_rejects_cyclic_graph() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = make_store(&tmp);
        let nodes = vec![
            NodeSpec { id: "a".into(), description: "a".into(), shell_cmd: "echo".into(), deps: vec!["b".into()] },
            NodeSpec { id: "b".into(), description: "b".into(), shell_cmd: "echo".into(), deps: vec!["a".into()] },
        ];
        assert!(store.create("cyclic", "claude", nodes).is_err());
    }
}
```

- [ ] **Step 4: Register in `src/lib.rs`**

Add `pub mod task_graph;` after `pub mod evolution;`.

- [ ] **Step 5: Run tests**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test task_graph::tests
```

Expected: 5 tests PASS

- [ ] **Step 6: Commit**

```powershell
git add src/task_graph.rs src/lib.rs
git commit -m "feat(graph): implement TaskGraph DAG with dependency ordering, cycle detection, and SQLite persistence"
```

---

### Task 3: Add TCP commands — `CreateTaskGraph`, `ExecuteTaskGraph`, `GetTaskGraph`

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Add `graph_store_for_client` clone before spawn**

In `run_inner()`, where `factory_for_client` is cloned, add:
```rust
let graph_store_for_client = std::sync::Arc::new(
    crate::task_graph::GraphStore::new(crate::task_graph::GraphStore::default_path())
);
```

- [ ] **Step 2: Add 3 handlers to the dispatch loop**

After `ListRunning`, add:

```rust
} else if v["command"] == "CreateTaskGraph" {
    let goal = v["goal"].as_str().unwrap_or("unnamed goal").to_string();
    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();

    let nodes_val = v["nodes"].as_array().cloned().unwrap_or_default();
    let nodes: Vec<crate::task_graph::NodeSpec> = nodes_val
        .iter()
        .filter_map(|n| {
            Some(crate::task_graph::NodeSpec {
                id: n["id"].as_str()?.to_string(),
                description: n["description"].as_str()?.to_string(),
                shell_cmd: n["shell_cmd"].as_str()?.to_string(),
                deps: n["deps"].as_array()
                    .map(|arr| arr.iter().filter_map(|d| d.as_str().map(ToOwned::to_owned)).collect())
                    .unwrap_or_default(),
            })
        })
        .collect();

    match graph_store_for_client.create(&goal, &agent_name, nodes) {
        Ok(graph_id) => {
            let response = serde_json::json!({
                "event": "TaskGraphCreated",
                "graph_id": graph_id
            });
            let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
        }
        Err(e) => {
            let err = serde_json::json!({
                "event": "TaskGraphError",
                "error": e.to_string()
            });
            let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
        }
    }
} else if v["command"] == "ExecuteTaskGraph" {
    if let Some(graph_id) = v["graph_id"].as_str().map(|s| s.to_string()) {
        let store = graph_store_for_client.clone();
        let factory = factory_for_client.clone();
        let graph_id_clone = graph_id.clone();

        tokio::spawn(async move {
            execute_graph_async(store, factory, graph_id_clone).await;
        });

        let response = serde_json::json!({
            "event": "TaskGraphExecuting",
            "graph_id": graph_id
        });
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    }
} else if v["command"] == "GetTaskGraph" {
    if let Some(graph_id) = v["graph_id"].as_str() {
        match graph_store_for_client.get(graph_id) {
            Some(graph) => {
                let response = serde_json::json!({
                    "event": "TaskGraphState",
                    "graph": graph
                });
                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
            }
            None => {
                let err = serde_json::json!({
                    "event": "TaskGraphError",
                    "error": format!("graph {} not found", graph_id)
                });
                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
            }
        }
    }
```

- [ ] **Step 3: Add `execute_graph_async` helper function** (outside `impl Server`, at the bottom of `server.rs`):

```rust
async fn execute_graph_async(
    store: std::sync::Arc<crate::task_graph::GraphStore>,
    factory: std::sync::Arc<crate::factory::Factory>,
    graph_id: String,
) {
    let timeout = std::time::Duration::from_secs(600);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            eprintln!("[TaskGraph] Execution timeout for graph {}", graph_id);
            break;
        }

        let ready = store.ready_nodes(&graph_id);
        if ready.is_empty() {
            // Check if graph is fully complete or failed
            if let Some(graph) = store.get(&graph_id) {
                if graph.status != "pending" && graph.status != "running" {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }

        for node in ready {
            let store2 = store.clone();
            let factory2 = factory.clone();
            let gid = graph_id.clone();
            let nid = node.id.clone();
            let cmd = node.shell_cmd.clone();
            let desc = node.description.clone();

            let job = crate::factory::Job::new(&desc, "graph-executor", None, None);
            let job_id = job.id;

            store2.mark_node_running(&gid, &nid, &job_id.to_string());

            factory2.submit(
                job,
                Box::pin(async move {
                    let output = tokio::process::Command::new("cmd")
                        .args(["/C", &cmd])
                        .output()
                        .await?;
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let result = if output.status.success() {
                        Ok(stdout)
                    } else {
                        Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
                    };
                    // Update graph node status
                    match &result {
                        Ok(r) => store2.mark_node_complete(&gid, &nid, r, &job_id.to_string()),
                        Err(e) => store2.mark_node_failed(&gid, &nid, &e.to_string()),
                    }
                    result
                }),
            );
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
```

- [ ] **Step 4: Build check + full tests**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo check
cargo test 2>&1 | Select-Object -Last 6
```

Expected: no new failures

- [ ] **Step 5: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "feat(graph): add CreateTaskGraph, ExecuteTaskGraph, GetTaskGraph TCP commands"
```

---

## TCP Command Reference

| Command | Required fields | Response |
|---------|----------------|----------|
| `CreateTaskGraph` | `goal`, `agent`, `nodes: [{id, description, shell_cmd, deps:[]}]` | `TaskGraphCreated {graph_id}` |
| `ExecuteTaskGraph` | `graph_id` | `TaskGraphExecuting` (async — poll with GetTaskGraph) |
| `GetTaskGraph` | `graph_id` | `TaskGraphState {graph}` |

**Node execution order:** leaf nodes first → results propagate up → parallel where deps allow → timeout 10 min per graph.
