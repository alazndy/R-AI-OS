use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use uuid::Uuid;

use super::dag::{control_status_to_node_status, legacy_status_to_node_status, validate_dag};
use super::types::{GraphNode, NodeSpec, NodeStatus, TaskGraph};
use super::MAX_NODES;

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

    pub(crate) fn connect(&self) -> Result<Connection> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        raios_core::db::migrate_existing(&conn)?;
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
                    cp_task_id TEXT, cp_agent_run_id TEXT,
                    factory_job_id TEXT, result TEXT, error TEXT,
                    PRIMARY KEY (graph_id, id)
                );",
            );
            let _ = conn.execute_batch(
                "ALTER TABLE task_graph_nodes ADD COLUMN cp_task_id TEXT;
                 ALTER TABLE task_graph_nodes ADD COLUMN cp_agent_run_id TEXT;",
            );
        }
    }

    /// Create a new task graph. Returns the graph ID.
    pub fn create(&self, goal: &str, agent: &str, nodes: Vec<NodeSpec>) -> Result<String> {
        if nodes.len() > MAX_NODES {
            anyhow::bail!("Too many nodes: {} (max {})", nodes.len(), MAX_NODES);
        }

        validate_dag(&nodes)?;

        let graph_id = Uuid::new_v4().to_string();
        let conn = self.connect()?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        conn.execute(
            "INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,?2,?3,'pending',?4)",
            params![graph_id, goal, agent, now],
        )?;
        conn.execute(
            "INSERT INTO cp_task_graphs (graph_id, goal, agent, created_at) VALUES (?1,?2,?3,?4)",
            params![graph_id, goal, agent, now],
        )?;

        let mut node_task_ids = std::collections::HashMap::new();
        for node in &nodes {
            let deps_json = serde_json::to_string(&node.deps).unwrap_or_else(|_| "[]".into());
            let workflow = raios_core::db::create_task_graph_node_workflow(
                &conn,
                &graph_id,
                &node.id,
                &node.description,
                &node.shell_cmd,
                agent,
                node.deps.is_empty(),
            )?;
            conn.execute(
                "INSERT INTO task_graph_nodes
                    (id, graph_id, description, shell_cmd, deps, cp_task_id, cp_agent_run_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    node.id,
                    graph_id,
                    node.description,
                    node.shell_cmd,
                    deps_json,
                    workflow.task_id,
                    workflow.agent_run_id
                ],
            )?;
            node_task_ids.insert(node.id.clone(), workflow.task_id);
        }
        raios_core::db::create_task_graph_edges(&conn, &graph_id, &node_task_ids, &nodes)?;
        self.refresh_legacy_cache(&graph_id, &conn);

        Ok(graph_id)
    }

    pub fn get(&self, graph_id: &str) -> Option<TaskGraph> {
        let conn = self.connect().ok()?;
        let graph = self.load_graph_metadata(graph_id, &conn)?;
        let nodes = self.load_nodes(graph_id, &conn);

        Some(TaskGraph {
            id: graph.0,
            goal: graph.1,
            agent: graph.2,
            status: graph.3,
            created_at: graph.4,
            completed_at: graph.5,
            nodes,
        })
    }

    fn load_nodes(&self, graph_id: &str, conn: &Connection) -> Vec<GraphNode> {
        let graph_deps = raios_core::db::load_graph_node_dependencies(conn, graph_id);
        let control_nodes = raios_core::db::load_control_graph_nodes(conn, graph_id);
        if !control_nodes.is_empty() {
            return control_nodes
                .into_iter()
                .map(|row| {
                    let status = control_status_to_node_status(&row.task_status);
                    let factory_job_id = matches!(
                        row.run_status.as_str(),
                        "running" | "succeeded" | "failed" | "cancelled"
                    )
                    .then_some(row.run_contract_id)
                    .filter(|id| !id.is_empty());
                    let result = matches!(row.run_status.as_str(), "succeeded")
                        .then_some(row.summary)
                        .flatten();
                    let error = matches!(row.run_status.as_str(), "failed" | "cancelled")
                        .then_some(row.exit_reason)
                        .flatten();
                    GraphNode {
                        id: row.node_id.clone(),
                        graph_id: graph_id.to_string(),
                        task_id: Some(row.task_id),
                        agent_run_id: Some(row.agent_run_id),
                        description: row.description,
                        shell_cmd: row.shell_cmd,
                        deps: graph_deps.get(&row.node_id).cloned().unwrap_or_default(),
                        status,
                        factory_job_id,
                        result,
                        error,
                    }
                })
                .collect();
        }

        let cp_statuses = raios_core::db::load_graph_control_task_statuses(conn, graph_id);
        let mut stmt = conn
            .prepare(
                "SELECT id, graph_id, description, shell_cmd, deps, status, cp_task_id, cp_agent_run_id, factory_job_id, result, error
                 FROM task_graph_nodes WHERE graph_id=?1",
            )
            .ok();

        match &mut stmt {
            Some(s) => s
                .query_map(params![graph_id], |row| {
                    let node_id: String = row.get(0)?;
                    let deps_str: String = row.get(4)?;
                    let legacy_deps: Vec<String> =
                        serde_json::from_str(&deps_str).unwrap_or_default();
                    let legacy_status: String = row.get(5)?;
                    let task_id: Option<String> = row.get(6)?;
                    let status = task_id
                        .as_ref()
                        .and_then(|id| cp_statuses.get(id))
                        .map(|status| control_status_to_node_status(status))
                        .unwrap_or_else(|| legacy_status_to_node_status(&legacy_status));
                    let deps = graph_deps
                        .get(&node_id)
                        .cloned()
                        .unwrap_or(legacy_deps);
                    Ok(GraphNode {
                        id: node_id,
                        graph_id: row.get(1)?,
                        task_id,
                        agent_run_id: row.get(7)?,
                        description: row.get(2)?,
                        shell_cmd: row.get(3)?,
                        deps,
                        status,
                        factory_job_id: row.get(8)?,
                        result: row.get(9)?,
                        error: row.get(10)?,
                    })
                })
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
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
            .filter(|node| node.status == NodeStatus::Completed)
            .map(|n| n.id.clone())
            .collect();

        all.into_iter()
            .filter(|node| {
                let deps_ready = node.deps.iter().all(|dep| completed_ids.contains(dep));
                if node.status == NodeStatus::Pending && deps_ready {
                    if let Some(task_id) = node.task_id.as_deref() {
                        let _ = raios_core::db::mark_control_task_ready(&conn, task_id);
                    }
                }
                node.status == NodeStatus::Pending && deps_ready
            })
            .collect()
    }

    pub fn mark_node_running(&self, graph_id: &str, node_id: &str, job_id: &str) {
        if let Ok(conn) = self.connect() {
            let ids = self.control_workflow_ids(&conn, graph_id, node_id);
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET factory_job_id=?3
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, job_id],
            );
            if let Some((task_id, agent_run_id)) = ids {
                let _ = raios_core::db::mark_control_task_running(&conn, &task_id, &agent_run_id, job_id);
            }
            self.sync_graph_status(graph_id, &conn);
            self.refresh_legacy_cache(graph_id, &conn);
        }
    }

    pub fn mark_node_complete(&self, graph_id: &str, node_id: &str, result: &str, job_id: &str) {
        if let Ok(conn) = self.connect() {
            let ids = self.control_workflow_ids(&conn, graph_id, node_id);
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET result=?3, factory_job_id=?4
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, result, job_id],
            );
            if let Some((task_id, agent_run_id)) = ids {
                let _ = raios_core::db::mark_control_task_completed(&conn, &task_id, &agent_run_id, result);
            }
            self.sync_graph_status(graph_id, &conn);
            self.refresh_legacy_cache(graph_id, &conn);
        }
    }

    pub fn mark_node_failed(&self, graph_id: &str, node_id: &str, error: &str) {
        if let Ok(conn) = self.connect() {
            let ids = self.control_workflow_ids(&conn, graph_id, node_id);
            let _ = conn.execute(
                "UPDATE task_graph_nodes SET error=?3
                 WHERE graph_id=?1 AND id=?2",
                params![graph_id, node_id, error],
            );
            if let Some((task_id, agent_run_id)) = ids {
                let _ = raios_core::db::mark_control_task_failed(&conn, &task_id, &agent_run_id, error);
            }
            self.sync_graph_status(graph_id, &conn);
            self.refresh_legacy_cache(graph_id, &conn);
        }
    }

    fn control_workflow_ids(
        &self,
        conn: &Connection,
        graph_id: &str,
        node_id: &str,
    ) -> Option<(String, String)> {
        conn.query_row(
            "SELECT task_id, agent_run_id FROM cp_task_graph_nodes
             WHERE graph_id=?1 AND node_id=?2",
            params![graph_id, node_id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .ok()
        .and_then(|(task_id, agent_run_id)| task_id.zip(agent_run_id))
    }

    fn load_graph_metadata(
        &self,
        graph_id: &str,
        conn: &Connection,
    ) -> Option<(String, String, String, String, String, Option<String>)> {
        if let Ok(graph) = conn.query_row(
            "SELECT graph_id, goal, agent, created_at, completed_at
             FROM cp_task_graphs WHERE graph_id=?1",
            params![graph_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    self.derived_graph_status(conn, graph_id),
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        ) {
            return Some(graph);
        }

        conn.query_row(
            "SELECT id, goal, agent, status, created_at, completed_at FROM task_graphs WHERE id=?1",
            params![graph_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .ok()
    }

    fn derived_graph_status(&self, conn: &Connection, graph_id: &str) -> String {
        let task_ids = self.control_task_ids_for_graph(conn, graph_id);
        let statuses = raios_core::db::load_control_task_statuses(conn, &task_ids);
        if statuses.is_empty() {
            return "pending".to_string();
        }

        let total = statuses.len();
        let completed = statuses.values().filter(|status| *status == "completed").count();
        let failed = statuses
            .values()
            .filter(|status| matches!(status.as_str(), "failed" | "cancelled"))
            .count();
        let running = statuses.values().filter(|status| *status == "running").count();

        if failed > 0 {
            "failed".to_string()
        } else if completed == total {
            "completed".to_string()
        } else if running > 0 {
            "running".to_string()
        } else {
            "pending".to_string()
        }
    }

    fn sync_graph_status(&self, graph_id: &str, conn: &Connection) {
        let graph_status = self.derived_graph_status(conn, graph_id);
        if graph_status == "pending" && self.control_task_ids_for_graph(conn, graph_id).is_empty() {
            return;
        }

        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let completed_at = (graph_status == "completed").then_some(now.as_str());

        let _ = conn.execute(
            "UPDATE task_graphs SET status=?2, completed_at=?3 WHERE id=?1",
            params![graph_id, graph_status, completed_at],
        );
        let _ = conn.execute(
            "UPDATE cp_task_graphs SET completed_at=?2 WHERE graph_id=?1",
            params![graph_id, completed_at],
        );
    }

    fn control_task_ids_for_graph(&self, conn: &Connection, graph_id: &str) -> Vec<String> {
        let mut stmt = match conn.prepare(
            "SELECT task_id FROM cp_task_graph_nodes WHERE graph_id=?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return vec![],
        };

        stmt.query_map(params![graph_id], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub(crate) fn refresh_legacy_cache(&self, graph_id: &str, conn: &Connection) {
        let Some((id, goal, agent, status, created_at, completed_at)) =
            self.load_graph_metadata(graph_id, conn)
        else {
            return;
        };
        let nodes = self.load_nodes(graph_id, conn);

        let _ = conn.execute(
            "INSERT INTO task_graphs (id, goal, agent, status, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                goal=excluded.goal,
                agent=excluded.agent,
                status=excluded.status,
                created_at=excluded.created_at,
                completed_at=excluded.completed_at",
            params![id, goal, agent, status, created_at, completed_at],
        );
        let _ = conn.execute("DELETE FROM task_graph_nodes WHERE graph_id=?1", params![graph_id]);
        for node in nodes {
            let deps_json = serde_json::to_string(&node.deps).unwrap_or_else(|_| "[]".into());
            let legacy_status = match node.status {
                NodeStatus::Pending => "pending",
                NodeStatus::Running => "running",
                NodeStatus::Completed => "completed",
                NodeStatus::Failed => "failed",
            };
            let _ = conn.execute(
                "INSERT INTO task_graph_nodes
                    (id, graph_id, description, shell_cmd, deps, status, cp_task_id, cp_agent_run_id, factory_job_id, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    node.id,
                    graph_id,
                    node.description,
                    node.shell_cmd,
                    deps_json,
                    legacy_status,
                    node.task_id,
                    node.agent_run_id,
                    node.factory_job_id,
                    node.result,
                    node.error,
                ],
            );
        }
    }
}
