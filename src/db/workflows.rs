use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
pub fn project_id_for_file_path(conn: &Connection, path: &str) -> Option<i64> {
    let target = path.replace('\\', "/");
    let projects = load_all_projects(conn).ok()?;
    projects
        .into_iter()
        .filter_map(|project| {
            let root = project.path.replace('\\', "/");
            target
                .starts_with(&root)
                .then_some((project.id, root.len()))
        })
        .max_by_key(|(_, len)| *len)
        .map(|(id, _)| id)
}

pub fn create_file_change_workflow(
    conn: &Connection,
    path: &str,
    original_content: &str,
    new_content: &str,
    agent_name: &str,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let title = format!(
        "Review file change: {}",
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
    );
    let description = format!("Review and apply pending file mutation for {}", path);
    let metadata_json = serde_json::json!({
        "path": path,
        "agent_name": agent_name,
        "original_content": original_content,
        "new_content": new_content,
        "flow": "file_change_approval"
    })
    .to_string();

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 50, 'awaiting_approval',
             'agent', ?5, 'apply approved file change safely', ?6, ?6)",
        params![task_id, project_id, title, description, agent_name, now],
    )?;

    // Create a persisted run contract scoped to this specific file path (after cp_tasks insert)
    let workspace_root = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let allowed_paths = serde_json::to_string(&[path]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        &workspace_root,
        &allowed_paths,
        "[]",
        "[]",
        None,
        None,
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'daemon', ?4, ?5, 1, 'awaiting_approval', ?6)",
        params![agent_run_id, task_id, project_id, agent_name, run_contract_id, now],
    )?;
    conn.execute(
        "INSERT INTO cp_artifacts
            (id, task_id, agent_run_id, kind, status, path, content_ref, metadata_json, created_at)
         VALUES (?1, ?2, ?3, 'file_change', 'submitted', ?4, NULL, ?5, ?6)",
        params![artifact_id, task_id, agent_run_id, path, metadata_json, now],
    )?;
    conn.execute(
        "INSERT INTO cp_approvals
            (id, project_id, task_id, agent_run_id, artifact_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'file_write', ?6, 'pending', ?7)",
        params![
            approval_id,
            project_id,
            task_id,
            agent_run_id,
            artifact_id,
            format!("File change requested for {}", path),
            now
        ],
    )?;

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id,
        approval_id,
        project_id,
    })
}

pub fn mark_file_change_workflow_applied(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'approved', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'approved_and_applied'
         WHERE id = ?2",
        params![now, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn mark_file_change_workflow_rejected(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'rejected', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, reason, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn mark_file_change_workflow_apply_failed(
    conn: &Connection,
    ids: &crate::control_plane::FileChangeWorkflowIds,
    resolved_by: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'approved', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ids.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
        params![ids.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'failed', ended_at = ?1, exit_reason = ?2
         WHERE id = ?3",
        params![now, reason, ids.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'failed', updated_at = ?1
         WHERE id = ?2",
        params![now, ids.task_id],
    )?;
    Ok(())
}

pub fn create_handoff_workflow(
    conn: &Connection,
    project_path: &str,
    from_agent: &str,
    to_agent: &str,
    status: &str,
    msg: &str,
    diff_stat: Option<&str>,
) -> Result<crate::control_plane::HandoffWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, project_path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let status_lc = status.to_lowercase();
    let title = format!("Handoff → {}", to_agent);
    let description = format!(
        "Agent handoff from {} to {} (status: {}): {}",
        from_agent, to_agent, status_lc, msg
    );
    let metadata_json = serde_json::json!({
        "from": from_agent,
        "to": to_agent,
        "status": status_lc,
        "context_summary": msg,
        "diff_stat": diff_stat,
        "flow": "agent_handoff"
    })
    .to_string();

    let tx = conn.unchecked_transaction()?;

    // One active handoff per (project, assignee): supersede whatever this one replaces
    // instead of letting stale pending handovers pile up for the same agent.
    tx.execute(
        "UPDATE cp_approvals
         SET status = 'expired', resolved_at = ?1, resolved_by = 'superseded'
         WHERE approval_type = 'handover' AND status = 'pending'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?2 AND (?3 IS NULL OR project_id = ?3)
           )",
        params![now, to_agent, project_id],
    )?;
    tx.execute(
        "UPDATE cp_artifacts
         SET status = 'superseded'
         WHERE kind = 'handover_note' AND status = 'submitted'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?1 AND (?2 IS NULL OR project_id = ?2)
           )",
        params![to_agent, project_id],
    )?;
    tx.execute(
        "UPDATE cp_agent_runs
         SET status = 'cancelled', ended_at = ?1, exit_reason = 'superseded'
         WHERE provider = 'handoff' AND status = 'awaiting_approval'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?2 AND (?3 IS NULL OR project_id = ?3)
           )",
        params![now, to_agent, project_id],
    )?;
    tx.execute(
        "UPDATE cp_tasks
         SET status = 'cancelled', updated_at = ?1
         WHERE assignee_id = ?2 AND status = 'awaiting_approval'
           AND (?3 IS NULL OR project_id = ?3)",
        params![now, to_agent, project_id],
    )?;

    tx.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 50, 'awaiting_approval',
             'agent', ?5, 'deliver handover context to assignee exactly once', ?6, ?6)",
        params![task_id, project_id, title, description, to_agent, now],
    )?;

    let run_contract_id = cp_insert_run_contract(
        &tx,
        Some(&task_id),
        project_path,
        "[]",
        "[]",
        "[]",
        None,
        None,
    )?;

    tx.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'handoff', ?4, ?5, 1, 'awaiting_approval', ?6)",
        params![agent_run_id, task_id, project_id, from_agent, run_contract_id, now],
    )?;
    tx.execute(
        "INSERT INTO cp_artifacts
            (id, task_id, agent_run_id, kind, status, path, content_ref, metadata_json, created_at)
         VALUES (?1, ?2, ?3, 'handover_note', 'submitted', NULL, NULL, ?4, ?5)",
        params![artifact_id, task_id, agent_run_id, metadata_json, now],
    )?;
    tx.execute(
        "INSERT INTO cp_approvals
            (id, project_id, task_id, agent_run_id, artifact_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'handover', ?6, 'pending', ?7)",
        params![approval_id, project_id, task_id, agent_run_id, artifact_id, msg, now],
    )?;
    tx.commit()?;

    Ok(crate::control_plane::HandoffWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id,
        approval_id,
        project_id,
    })
}

/// Most recent pending handover addressed to `to_agent` (optionally scoped to a project).
/// Returns `None` when no handover is waiting, so callers can spawn normally.
pub fn cp_take_pending_handoff(
    conn: &Connection,
    project_id: Option<i64>,
    to_agent: &str,
) -> Result<Option<crate::control_plane::HandoffContext>> {
    let row = conn
        .query_row(
            "SELECT ap.id, ap.artifact_id, ap.agent_run_id, ap.task_id, art.metadata_json
             FROM cp_approvals ap
             JOIN cp_tasks t ON t.id = ap.task_id
             JOIN cp_artifacts art ON art.id = ap.artifact_id
             WHERE ap.approval_type = 'handover' AND ap.status = 'pending'
               AND t.assignee_id = ?1
               AND (?2 IS NULL OR t.project_id = ?2)
             ORDER BY ap.requested_at DESC
             LIMIT 1",
            params![to_agent, project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;

    let Some((approval_id, artifact_id, agent_run_id, task_id, metadata_json)) = row else {
        return Ok(None);
    };
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_json).unwrap_or(serde_json::Value::Null);
    let from_agent = metadata["from"].as_str().unwrap_or_default().to_string();
    let status = metadata["status"].as_str().unwrap_or_default().to_string();
    let context_summary = metadata["context_summary"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let diff_stat = metadata["diff_stat"].as_str().map(|s| s.to_string());

    Ok(Some(crate::control_plane::HandoffContext {
        approval_id,
        artifact_id,
        agent_run_id,
        task_id,
        from_agent,
        to_agent: to_agent.to_string(),
        status,
        context_summary,
        diff_stat,
    }))
}

/// Marks a handover as delivered: approval approved, artifact applied, run + task completed.
/// Idempotent in effect — once consumed, `cp_take_pending_handoff` no longer returns it.
pub fn cp_consume_handoff(
    conn: &Connection,
    ctx: &crate::control_plane::HandoffContext,
    resolved_by: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals
         SET status = 'approved', resolved_at = ?1, resolved_by = ?2
         WHERE id = ?3",
        params![now, resolved_by, ctx.approval_id],
    )?;
    conn.execute(
        "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
        params![ctx.artifact_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'handover_delivered'
         WHERE id = ?2",
        params![now, ctx.agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks
         SET status = 'completed', updated_at = ?1
         WHERE id = ?2",
        params![now, ctx.task_id],
    )?;
    Ok(())
}

/// Open a wrapper session row in cp_agent_runs. Returns (task_id, run_id).
/// Called by agent_runner::run_agent() at the moment the child process spawns.
pub fn cp_session_start(
    conn: &Connection,
    agent_identity: &str,
    project_id: Option<i64>,
) -> Result<(String, String)> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let task_id = uuid::Uuid::new_v4().to_string();
    let run_id = uuid::Uuid::new_v4().to_string();
    let workspace_root = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".to_string());

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 50, 'in_progress',
             'agent', ?5, 'interactive wrapper session', ?6, ?6)",
        params![
            task_id,
            project_id,
            format!("Session: {}", agent_identity),
            format!("Wrapper-routed interactive session for {}", agent_identity),
            agent_identity,
            now
        ],
    )?;

    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        &workspace_root,
        "[]",
        "[]",
        "[]",
        None,
        None,
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'wrapper', ?4, ?5, 1, 'running', ?6)",
        params![run_id, task_id, project_id, agent_identity, run_contract_id, now],
    )?;

    Ok((task_id, run_id))
}

/// Close a wrapper session opened with cp_session_start.
pub fn cp_session_end(
    conn: &Connection,
    task_id: &str,
    run_id: &str,
    success: bool,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let run_status = if success { "succeeded" } else { "failed" };
    let task_status = if success { "completed" } else { "failed" };
    let exit_reason = if success { "clean_exit" } else { "nonzero_exit" };

    conn.execute(
        "UPDATE cp_agent_runs SET status=?1, ended_at=?2, exit_reason=?3 WHERE id=?4",
        params![run_status, now, exit_reason, run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status=?1, updated_at=?2 WHERE id=?3",
        params![task_status, now, task_id],
    )?;
    Ok(())
}

/// List recent wrapper sessions (most recent first).
pub struct SessionRow {
    pub run_id: String,
    pub agent_name: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_reason: Option<String>,
}

pub fn cp_sessions_list(conn: &Connection, limit: usize) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_name, status, started_at, ended_at, exit_reason
         FROM cp_agent_runs
         WHERE provider = 'wrapper'
         ORDER BY started_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        Ok(SessionRow {
            run_id: r.get(0)?,
            agent_name: r.get(1)?,
            status: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            exit_reason: r.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>>>()
}

pub fn create_swarm_workflow(
    conn: &Connection,
    project_path: &str,
    description: &str,
    agent_name: &str,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, project_path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();

    conn.execute(
        "INSERT INTO cp_tasks
            (id, project_id, plan_id, parent_task_id, title, description, priority, status,
             assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, ?4, 60, 'queued',
             'agent', ?5, 'merge or reject swarm worktree cleanly', ?6, ?6)",
        params![
            task_id,
            project_id,
            format!("Swarm task: {}", description),
            description,
            agent_name,
            now
        ],
    )?;

    // Create a persisted run contract scoped to the project path (after cp_tasks insert)
    let allowed_paths = serde_json::to_string(&[project_path]).unwrap_or_else(|_| "[]".into());
    let run_contract_id = cp_insert_run_contract(
        conn,
        Some(&task_id),
        project_path,
        &allowed_paths,
        "[]",
        "[]",
        None,
        Some(3600),
    )?;

    conn.execute(
        "INSERT INTO cp_agent_runs
            (id, task_id, project_id, provider, agent_name, run_contract_id, attempt, status, started_at)
         VALUES (?1, ?2, ?3, 'swarm', ?4, ?5, 1, 'pending', ?6)",
        params![agent_run_id, task_id, project_id, agent_name, run_contract_id, now],
    )?;

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id: String::new(),
        approval_id: String::new(),
        project_id,
    })
}

pub fn mark_swarm_workflow_running(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs SET status = 'running', started_at = COALESCE(started_at, ?1) WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    Ok(())
}

pub fn create_task_graph_node_workflow(
    conn: &Connection,
    graph_id: &str,
    node_id: &str,
    description: &str,
    shell_cmd: &str,
    agent_name: &str,
    ready: bool,
) -> Result<crate::control_plane::FileChangeWorkflowIds> {
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

    Ok(crate::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id: String::new(),
        approval_id: String::new(),
        project_id: None,
    })
}

pub fn create_task_graph_edges(
    conn: &Connection,
    graph_id: &str,
    node_task_ids: &std::collections::HashMap<String, String>,
    nodes: &[crate::task_graph::NodeSpec],
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

pub fn ensure_swarm_review_artifacts(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    project_id: Option<i64>,
    project_path: &str,
    branch_name: &str,
    worktree_path: &str,
) -> Result<(String, String)> {
    let existing = conn.query_row(
        "SELECT id FROM cp_artifacts WHERE task_id = ?1 AND kind = 'diff' ORDER BY created_at DESC LIMIT 1",
        params![task_id],
        |row| row.get::<_, String>(0),
    ).ok();
    let existing_approval = conn.query_row(
        "SELECT id FROM cp_approvals WHERE task_id = ?1 AND approval_type = 'merge' ORDER BY requested_at DESC LIMIT 1",
        params![task_id],
        |row| row.get::<_, String>(0),
    ).ok();
    if let (Some(artifact_id), Some(approval_id)) = (existing, existing_approval) {
        return Ok((artifact_id, approval_id));
    }

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let metadata_json = serde_json::json!({
        "project_path": project_path,
        "branch_name": branch_name,
        "worktree_path": worktree_path,
        "flow": "swarm_review"
    })
    .to_string();

    conn.execute(
        "INSERT INTO cp_artifacts
            (id, task_id, agent_run_id, kind, status, path, content_ref, metadata_json, created_at)
         VALUES (?1, ?2, ?3, 'diff', 'submitted', ?4, ?5, ?6, ?7)",
        params![
            artifact_id,
            task_id,
            agent_run_id,
            worktree_path,
            branch_name,
            metadata_json,
            now
        ],
    )?;
    conn.execute(
        "INSERT INTO cp_approvals
            (id, project_id, task_id, agent_run_id, artifact_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'merge', ?6, 'pending', ?7)",
        params![
            approval_id,
            project_id,
            task_id,
            agent_run_id,
            artifact_id,
            format!("Review swarm merge for branch {}", branch_name),
            now
        ],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'awaiting_approval', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    conn.execute(
        "UPDATE cp_agent_runs SET status = 'awaiting_approval' WHERE id = ?1",
        params![agent_run_id],
    )?;

    Ok((artifact_id, approval_id))
}

pub fn mark_swarm_workflow_merged(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    artifact_id: Option<&str>,
    approval_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Some(artifact_id) = artifact_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
            params![artifact_id],
        )?;
    }
    if let Some(approval_id) = approval_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_approvals
             SET status = 'approved', resolved_at = ?1, resolved_by = 'human'
             WHERE id = ?2",
            params![now, approval_id],
        )?;
    }
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'succeeded', ended_at = ?1, exit_reason = 'merged'
         WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'completed', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    Ok(())
}

pub fn mark_swarm_workflow_rejected(
    conn: &Connection,
    task_id: &str,
    agent_run_id: &str,
    artifact_id: Option<&str>,
    approval_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Some(artifact_id) = artifact_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_artifacts SET status = 'rejected' WHERE id = ?1",
            params![artifact_id],
        )?;
    }
    if let Some(approval_id) = approval_id.filter(|id| !id.is_empty()) {
        conn.execute(
            "UPDATE cp_approvals
             SET status = 'rejected', resolved_at = ?1, resolved_by = 'human'
             WHERE id = ?2",
            params![now, approval_id],
        )?;
    }
    conn.execute(
        "UPDATE cp_agent_runs
         SET status = 'cancelled', ended_at = ?1, exit_reason = 'rejected'
         WHERE id = ?2",
        params![now, agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status = 'cancelled', updated_at = ?1 WHERE id = ?2",
        params![now, task_id],
    )?;
    Ok(())
}

// ── Hydrated pending file-change approvals ────────────────────────────────────

/// All data needed to reconstruct a FileChangeApproval from canonical DB state.
pub struct FileChangeApprovalData {
    /// cp_approvals.id — the canonical, stable identifier.
    pub approval_id: String,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub artifact_id: Option<String>,
    pub path: String,
    pub original_content: String,
    pub new_content: String,
    pub agent_name: String,
}

/// Load all pending file-write approvals from canonical tables, hydrated with
/// path/content from cp_artifacts.metadata_json.
pub fn cp_load_pending_file_change_approvals(
    conn: &Connection,
) -> Result<Vec<FileChangeApprovalData>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id,
                ap.task_id,
                (SELECT ar.id        FROM cp_agent_runs ar WHERE ar.task_id = ap.task_id LIMIT 1),
                (SELECT a.id         FROM cp_artifacts  a  WHERE a.task_id  = ap.task_id LIMIT 1),
                (SELECT a.metadata_json FROM cp_artifacts a WHERE a.task_id = ap.task_id LIMIT 1),
                (SELECT ar.agent_name   FROM cp_agent_runs ar WHERE ar.task_id = ap.task_id LIMIT 1)
         FROM cp_approvals ap
         WHERE ap.status = 'pending' AND ap.approval_type = 'file_write'
         ORDER BY ap.requested_at DESC",
    )?;

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(approval_id, task_id, agent_run_id, artifact_id, meta_json, run_agent_name)| {
                let meta: serde_json::Value = meta_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                FileChangeApprovalData {
                    approval_id,
                    task_id,
                    agent_run_id,
                    artifact_id,
                    path: meta["path"].as_str().unwrap_or("").to_string(),
                    original_content: meta["original_content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    new_content: meta["new_content"].as_str().unwrap_or("").to_string(),
                    agent_name: meta["agent_name"]
                        .as_str()
                        .or(run_agent_name.as_deref())
                        .unwrap_or("unknown")
                        .to_string(),
                }
            },
        )
        .collect();

    Ok(rows)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub task_description: String,
    pub project_id: Option<String>,
    pub interval_secs: i64,
    pub status: String,
    pub last_run_at: Option<String>,
    pub next_run_at: String,
    pub created_at: String,
    pub run_count: i64,
}

