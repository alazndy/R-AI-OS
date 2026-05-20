//! Recursive Reasoning — Task Decomposition DAG
//!
//! Agents submit a directed acyclic graph of tasks. R-AI-OS executes nodes
//! in dependency order, running independent nodes in parallel via Factory Mode.
use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

        let completed_ids: std::collections::HashSet<String> = all
            .iter()
            .filter(|n| n.status == NodeStatus::Completed)
            .map(|n| n.id.clone())
            .collect();

        all.into_iter()
            .filter(|n| {
                n.status == NodeStatus::Pending
                    && n.deps.iter().all(|dep| completed_ids.contains(dep))
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
