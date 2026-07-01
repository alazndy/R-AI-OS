use super::*;
use rusqlite::{params, Connection, Result};
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
) -> Result<raios_core::control_plane::FileChangeWorkflowIds> {
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

    Ok(raios_core::control_plane::FileChangeWorkflowIds {
        task_id,
        agent_run_id,
        artifact_id,
        approval_id,
        project_id,
    })
}

pub fn mark_file_change_workflow_applied(
    conn: &Connection,
    ids: &raios_core::control_plane::FileChangeWorkflowIds,
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
    ids: &raios_core::control_plane::FileChangeWorkflowIds,
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
    ids: &raios_core::control_plane::FileChangeWorkflowIds,
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

