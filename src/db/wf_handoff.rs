use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
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
