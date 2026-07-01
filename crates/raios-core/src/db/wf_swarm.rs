use super::*;
use rusqlite::{params, Connection, Result};
pub fn create_swarm_workflow(
    conn: &Connection,
    project_path: &str,
    description: &str,
    agent_name: &str,
) -> Result<raios_core::control_plane::FileChangeWorkflowIds> {
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

    Ok(raios_core::control_plane::FileChangeWorkflowIds {
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

