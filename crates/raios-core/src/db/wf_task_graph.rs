use super::*;
use rusqlite::{params, Connection, Result};
pub fn create_task_graph_node_workflow(
    conn: &Connection,
    graph_id: &str,
    node_id: &str,
    description: &str,
    shell_cmd: &str,
    agent_name: &str,
    ready: bool,
) -> Result<raios_core::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let title = format!("Graph node {}: {}", node_id, description);
    let task_status = if ready { "ready" } else { "queued" };

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, NULL, ?2, NULL, ?3, ?4, 40, ?5,
             'agent', ?6, ?7, ?8, ?8)",
        params![
            task_id,
            graph_id,
            title,
            description,
            task_status,
            agent_name,
            format!("execute shell command: {}", shell_cmd),
            now
        ],
    )?;

    // Create a persisted run contract for this shell-execution task (after cp_tasks insert)
    let allowed_tools = serde_json::to_string(&["shell"]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        "",
        "[]",
        "[]",
        &allowed_tools,
        None,
        Some(600),
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, summary)
         VALUES (?1, ?2, NULL, 'task_graph', ?3, ?4, 1, 'pending', ?5)",
        params![agent_run_id, task_id, agent_name, run_contract_id, shell_cmd],
    )?;
    conn.execute(
        "INSERT INTO cp_task_graph_nodes
            (graph_id, node_id, task_id, agent_run_id, shell_cmd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![graph_id, node_id, task_id, agent_run_id, shell_cmd, now],
    )?;

    Ok(raios_core::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id: String::new(),
        approval_id: String::new(),
        project_id: None,
    })
}

pub fn insert_verify_gate_node(
    conn: &Connection,
    graph_id: &str,
    node_id: &str,
    description: &str,
    shell_cmd: &str,
    agent_name: &str,
    ready: bool,
) -> Result<raios_core::control_plane::FileChangeWorkflowIds> {
    let ids = create_task_graph_node_workflow(conn, graph_id, node_id, description, shell_cmd, agent_name, ready)?;
    conn.execute(
        "UPDATE cp_task_graph_nodes SET node_kind = 'verify_gate' WHERE graph_id = ?1 AND node_id = ?2",
        params![graph_id, node_id],
    )?;
    Ok(ids)
}

pub fn create_task_graph_edges(
    conn: &Connection,
    graph_id: &str,
    node_task_ids: &std::collections::HashMap<String, String>,
    nodes: &[raios_core::task_graph::NodeSpec],
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    for node in nodes {
        let Some(task_id) = node_task_ids.get(&node.id) else {
            continue;
        };
        for dep_node_id in &node.deps {
            let Some(depends_on_task_id) = node_task_ids.get(dep_node_id) else {
                continue;
            };
            conn.execute(
                "INSERT OR IGNORE INTO cp_task_edges
                    (graph_id, task_id, depends_on_task_id, edge_kind, created_at)
                 VALUES (?1, ?2, ?3, 'blocks', ?4)",
                params![graph_id, task_id, depends_on_task_id, now],
            )?;
        }
    }

    Ok(())
}

pub fn load_control_task_statuses(
    conn: &Connection,
    task_ids: &[String],
) -> std::collections::HashMap<String, String> {
    let mut statuses = std::collections::HashMap::new();
    let mut stmt = match conn.prepare("SELECT status FROM cp_tasks WHERE id = ?1") {
        Ok(stmt) => stmt,
        Err(_) => return statuses,
    };

    for task_id in task_ids {
        if let Ok(status) = stmt.query_row(params![task_id], |row| row.get::<_, String>(0)) {
            statuses.insert(task_id.clone(), status);
        }
    }

    statuses
}

pub fn load_graph_control_task_statuses(
    conn: &Connection,
    graph_id: &str,
) -> std::collections::HashMap<String, String> {
    let task_ids = {
        let mut stmt = match conn.prepare(
            "SELECT task_id FROM cp_task_graph_nodes WHERE graph_id = ?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return std::collections::HashMap::new(),
        };

        stmt.query_map(params![graph_id], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
    };

    load_control_task_statuses(conn, &task_ids)
}

pub fn load_graph_node_dependencies(
    conn: &Connection,
    graph_id: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut deps_by_node = std::collections::HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT node.node_id, dep_node.node_id
         FROM cp_task_edges edges
         JOIN cp_task_graph_nodes node
           ON node.graph_id = edges.graph_id AND node.task_id = edges.task_id
         JOIN cp_task_graph_nodes dep_node
           ON dep_node.graph_id = edges.graph_id AND dep_node.task_id = edges.depends_on_task_id
         WHERE edges.graph_id = ?1
         ORDER BY node.node_id, dep_node.node_id",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return deps_by_node,
    };

    let rows = match stmt.query_map(params![graph_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(rows) => rows,
        Err(_) => return deps_by_node,
    };

    for (node_id, dep_id) in rows.flatten() {
        deps_by_node.entry(node_id).or_insert_with(Vec::new).push(dep_id);
    }

    deps_by_node
}

pub struct ControlGraphNodeRow {
    pub node_id: String,
    pub task_id: String,
    pub agent_run_id: String,
    pub description: String,
    pub shell_cmd: String,
    pub task_status: String,
    pub run_status: String,
    pub run_contract_id: String,
    pub summary: Option<String>,
    pub exit_reason: Option<String>,
}

pub fn load_control_graph_nodes(conn: &Connection, graph_id: &str) -> Vec<ControlGraphNodeRow> {
    let mut stmt = match conn.prepare(
        "SELECT meta.node_id, meta.task_id, meta.agent_run_id, task.description, meta.shell_cmd,
                task.status, run.status, run.run_contract_id, run.summary, run.exit_reason
         FROM cp_task_graph_nodes meta
         JOIN cp_tasks task ON task.id = meta.task_id
         JOIN cp_agent_runs run ON run.id = meta.agent_run_id
         WHERE meta.graph_id = ?1
         ORDER BY meta.node_id",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return vec![],
    };

    stmt.query_map(params![graph_id], |row| {
        Ok(ControlGraphNodeRow {
            node_id: row.get(0)?,
            task_id: row.get(1)?,
            agent_run_id: row.get(2)?,
            description: row.get(3)?,
            shell_cmd: row.get(4)?,
            task_status: row.get(5)?,
            run_status: row.get(6)?,
            run_contract_id: row.get(7)?,
            summary: row.get(8)?,
            exit_reason: row.get(9)?,
        })
    })
    .ok()
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

pub fn mark_control_task_ready(conn: &Connection, task_id: &str) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'ready', updated_at = ?1
         WHERE id = ?2 AND status = 'queued'",
        params![now, task_id],
    )?;
    Ok(())
}

pub fn mark_control_task_running(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    job_id: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'running', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'running', started_at = COALESCE(started_at, ?1), run_contract_id = ?2
         WHERE id = ?3",
        params![now, job_id, agent_run_id],
    )?;
    Ok(())
}

pub fn mark_control_task_completed(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    result: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'completed', summary = ?2
         WHERE id = ?3",
        params![now, result, agent_run_id],
    )?;
    Ok(())
}

pub fn mark_control_task_failed(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    error: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, error, agent_run_id],
    )?;
    Ok(())
}

