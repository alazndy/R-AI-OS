use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct HandoffListRow {
    pub id: String,
    pub from_agent: String,
    pub to_agent: String,
    pub project: Option<String>,
    pub directory: String,
    pub created_at: String,
    pub delivery_status: String,
    pub user_status: Option<String>,
    pub message: String,
}

pub fn cp_list_handoffs(
    conn: &Connection,
    all: bool,
    project: Option<&str>,
    limit: u32,
) -> Result<Vec<HandoffListRow>> {
    let mut query = conn.prepare(
        "SELECT ap.id,r.agent_name,t.assignee_id,p.name,c.workspace_root,ap.requested_at,ap.status,
                json_extract(a.metadata_json,'$.user_status'),ap.reason
         FROM cp_approvals ap JOIN cp_tasks t ON t.id=ap.task_id
         JOIN cp_agent_runs r ON r.id=ap.agent_run_id AND r.task_id=t.id
         JOIN cp_artifacts a ON a.id=ap.artifact_id AND a.task_id=t.id AND a.agent_run_id=r.id
         JOIN cp_run_contracts c ON c.id=r.run_contract_id
         LEFT JOIN projects p ON p.id=ap.project_id
         WHERE ap.approval_type='handover' AND r.provider='handoff' AND a.kind='handover_note'
           AND ap.owner_subject='local_control_owner'
           AND (?1 OR COALESCE(json_extract(a.metadata_json,'$.user_status'),'')!='removed')
           AND (?2 IS NULL OR p.name=?2 OR p.path=?2 OR c.workspace_root=?2)
         ORDER BY ap.requested_at DESC,ap.id DESC LIMIT ?3",
    )?;
    let rows = query
        .query_map(params![all, project, limit.min(20_000)], |r| {
            Ok(HandoffListRow {
                id: r.get(0)?,
                from_agent: r.get(1)?,
                to_agent: r.get(2)?,
                project: r.get(3)?,
                directory: r.get(4)?,
                created_at: r.get(5)?,
                delivery_status: r.get(6)?,
                user_status: r.get(7)?,
                message: r.get(8)?,
            })
        })?
        .collect();
    rows
}

/// Local owner's explicit completion/archive decision, committed with its audit record.
/// Native provider sessions and wrapper runs are never affected.
pub fn cp_manage_handoff(conn: &Connection, id: &str, remove: bool) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let (artifact, status, user_status): (String, String, Option<String>) = tx.query_row(
        "SELECT a.id,ap.status,json_extract(a.metadata_json,'$.user_status')
         FROM cp_approvals ap JOIN cp_tasks t ON t.id=ap.task_id
         JOIN cp_agent_runs r ON r.id=ap.agent_run_id AND r.task_id=t.id
         JOIN cp_artifacts a ON a.id=ap.artifact_id AND a.task_id=t.id AND a.agent_run_id=r.id
         WHERE ap.id=?1 AND ap.approval_type='handover' AND ap.owner_subject='local_control_owner'
           AND r.provider='handoff' AND a.kind='handover_note'",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let disposition = if remove { "removed" } else { "completed" };
    if user_status.as_deref() == Some(disposition) {
        return Ok(());
    }
    if !remove
        && (user_status.as_deref() == Some("removed")
            || !matches!(status.as_str(), "pending" | "approved"))
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "removed or rejected handoffs cannot be marked completed".into(),
        ));
    }
    if status == "pending"
        && !cp_resolve_handoff(
            &tx,
            id,
            "local_control_owner",
            "local_user",
            if remove {
                Some("Removed by local user")
            } else {
                None
            },
        )?
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "handoff delivery state is inconsistent; no changes applied".into(),
        ));
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    tx.execute("UPDATE cp_artifacts SET metadata_json=json_set(metadata_json,'$.user_status',?1,'$.user_status_at',?2) WHERE id=?3",params![disposition,now,artifact])?;
    crate::security::record_audit_event(
        &tx,
        if remove {
            "handover_removed_by_user"
        } else {
            "handover_completed_by_user"
        },
        "local_user",
        &serde_json::json!({"approval_id":id,"disposition":disposition}).to_string(),
    )
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    tx.commit()
}

/// Structured findings a handoff can carry instead of (or alongside) a bare
/// free-text `--msg`. Serialized into the existing `cp_artifacts.metadata_json`
/// blob under a `"report"` key — no new column, no new table.
///
/// `Default` makes `..Default::default()` cheap for the back-compat path: a
/// bare `--msg` string becomes `HandoffReport { findings: msg, ..Default::default() }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandoffReport {
    pub findings: String,
    pub evidence: Vec<String>,
    pub edge_cases_considered: Vec<String>,
    pub open_questions: Vec<String>,
    pub confidence: f32,
    pub what_i_did_not_check: Vec<String>,
}

/// Complete input contract for one atomic handoff workflow.
///
/// Grouping the payload keeps the workflow API stable as handoff metadata grows
/// while making every caller name its fields explicitly.
pub struct HandoffWorkflowInput<'a> {
    pub project_path: &'a str,
    pub from_agent: &'a str,
    pub to_agent: &'a str,
    pub status: &'a str,
    pub msg: &'a str,
    pub diff_stat: Option<&'a str>,
    pub report: Option<&'a HandoffReport>,
}

pub fn create_handoff_workflow(
    conn: &Connection,
    input: HandoffWorkflowInput<'_>,
) -> Result<raios_core::control_plane::HandoffWorkflowIds> {
    let tx = conn.unchecked_transaction()?;
    let ids = create_handoff_workflow_in_tx(&tx, input)?;
    tx.commit()?;
    Ok(ids)
}

/// Same workflow as [`create_handoff_workflow`] for callers that already own an open
/// transaction, such as the control-plane dispatcher, which commits the handoff together
/// with its idempotency record and audit entry. It never begins or commits a transaction.
pub fn create_handoff_workflow_in_tx(
    conn: &Connection,
    input: HandoffWorkflowInput<'_>,
) -> Result<raios_core::control_plane::HandoffWorkflowIds> {
    let HandoffWorkflowInput {
        project_path,
        from_agent,
        to_agent,
        status,
        msg,
        diff_stat,
        report,
    } = input;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let project_id = project_id_for_file_path(conn, project_path);
    let task_id = uuid::Uuid::new_v4().to_string();
    let agent_run_id = uuid::Uuid::new_v4().to_string();
    let artifact_id = uuid::Uuid::new_v4().to_string();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let status_lc = status.to_lowercase();

    if status_lc == "success" {
        let unpassed_gate: Option<String> = conn
            .query_row(
                "SELECT meta.node_id
                 FROM cp_task_graph_nodes meta
                 JOIN cp_tasks t ON t.id = meta.task_id
                 WHERE meta.node_kind = 'verify_gate'
                   AND t.status != 'completed'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(gate_node) = unpassed_gate {
            let msg = format!(
                "Handoff SUCCESS blocked: verify_gate node '{}' has not passed yet",
                gate_node
            );
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::other(msg),
            )));
        }
    }
    let title = format!("Handoff → {}", to_agent);
    let description = format!(
        "Agent handoff from {} to {} (status: {}): {}",
        from_agent, to_agent, status_lc, msg
    );
    let mut metadata_value = serde_json::json!({
        "from": from_agent,
        "to": to_agent,
        "status": status_lc,
        "context_summary": msg,
        "diff_stat": diff_stat,
        "flow": "agent_handoff"
    });
    if let Some(report) = report {
        metadata_value["report"] = serde_json::to_value(report).unwrap_or(serde_json::Value::Null);
    }
    let metadata_json = metadata_value.to_string();

    // The caller owns the transaction; `tx` aliases it so the statements below are unchanged.
    let tx = conn;

    // A successful result also acknowledges an incoming handoff handled outside
    // `raios run`. Otherwise the old delivery remains active after the work was
    // returned. Scope to this exact project/path and sender, never global inbox.
    if status_lc == "success" {
        if let Some(incoming) =
            select_pending_handoff(tx, project_id, Some(project_path), from_agent)?
        {
            if cp_resolve_handoff(
                tx,
                &incoming.approval_id,
                "local_control_owner",
                from_agent,
                None,
            )? {
                audit_handoff_delivery(tx, &incoming.approval_id, from_agent)?;
            }
        }
    }

    // One active handoff per (project, assignee): supersede whatever this one replaces
    // instead of letting stale pending handovers pile up for the same agent.
    tx.execute(
        "UPDATE cp_approvals
         SET status = 'expired', resolved_at = ?1, resolved_by = 'superseded',
             revision = revision + 1
         WHERE approval_type = 'handover' AND status = 'pending'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?2 AND (project_id = ?3 OR (?3 IS NULL AND id IN (
                   SELECT task_id FROM cp_run_contracts WHERE workspace_root = ?4
               )))
           )",
        params![now, to_agent, project_id, project_path],
    )?;
    tx.execute(
        "UPDATE cp_artifacts
         SET status = 'superseded'
         WHERE kind = 'handover_note' AND status = 'submitted'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?1 AND (project_id = ?2 OR (?2 IS NULL AND id IN (
                   SELECT task_id FROM cp_run_contracts WHERE workspace_root = ?3
               )))
           )",
        params![to_agent, project_id, project_path],
    )?;
    tx.execute(
        "UPDATE cp_agent_runs
         SET status = 'cancelled', ended_at = ?1, exit_reason = 'superseded'
         WHERE provider = 'handoff' AND status = 'awaiting_approval'
           AND task_id IN (
               SELECT id FROM cp_tasks
               WHERE assignee_id = ?2 AND (project_id = ?3 OR (?3 IS NULL AND id IN (
                   SELECT task_id FROM cp_run_contracts WHERE workspace_root = ?4
               )))
           )",
        params![now, to_agent, project_id, project_path],
    )?;
    tx.execute(
        "UPDATE cp_tasks
         SET status = 'cancelled', updated_at = ?1
         WHERE assignee_id = ?2 AND status = 'awaiting_approval'
           AND EXISTS (SELECT 1 FROM cp_agent_runs r
                       WHERE r.task_id = cp_tasks.id AND r.provider = 'handoff')
           AND (project_id = ?3 OR (?3 IS NULL AND id IN (
                   SELECT task_id FROM cp_run_contracts WHERE workspace_root = ?4
               )))",
        params![now, to_agent, project_id, project_path],
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
        tx,
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

    Ok(raios_core::control_plane::HandoffWorkflowIds {
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
) -> Result<Option<raios_core::control_plane::HandoffContext>> {
    select_pending_handoff(conn, project_id, None, to_agent)
}

/// Scope delivery to a directory even when it has not been registered yet.
pub fn cp_take_pending_handoff_for_path(
    conn: &Connection,
    project_path: &str,
    to_agent: &str,
) -> Result<Option<raios_core::control_plane::HandoffContext>> {
    select_pending_handoff(
        conn,
        project_id_for_file_path(conn, project_path),
        Some(project_path),
        to_agent,
    )
}

fn select_pending_handoff(
    conn: &Connection,
    project_id: Option<i64>,
    project_path: Option<&str>,
    to_agent: &str,
) -> Result<Option<raios_core::control_plane::HandoffContext>> {
    let row = conn
        .query_row(
            "SELECT ap.id, ap.artifact_id, ap.agent_run_id, ap.task_id, art.metadata_json
             FROM cp_approvals ap
             JOIN cp_tasks t ON t.id = ap.task_id
             JOIN cp_artifacts art ON art.id = ap.artifact_id
             JOIN cp_agent_runs run ON run.id = ap.agent_run_id
             JOIN cp_run_contracts contract ON contract.id = run.run_contract_id
             WHERE ap.approval_type = 'handover' AND ap.status = 'pending'
               AND t.status = 'awaiting_approval' AND run.status = 'awaiting_approval'
               AND run.provider = 'handoff' AND art.kind = 'handover_note'
               AND art.status = 'submitted'
               AND t.assignee_id = ?1
               AND ((?2 IS NULL AND ?3 IS NULL) OR t.project_id = ?2
                    OR (?2 IS NULL AND contract.workspace_root = ?3))
             ORDER BY ap.requested_at DESC
             LIMIT 1",
            params![to_agent, project_id, project_path],
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
    let report: Option<HandoffReport> = metadata
        .get("report")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());

    Ok(Some(raios_core::control_plane::HandoffContext {
        approval_id,
        artifact_id,
        agent_run_id,
        task_id,
        from_agent,
        to_agent: to_agent.to_string(),
        status,
        context_summary,
        report,
        diff_stat,
    }))
}

/// Current optimistic-concurrency revision of an approval, or `None` when no such row exists.
///
/// Every transition of an approval bumps this value, so a surface can send back what it saw
/// (`expected_revision` on the handoff commands) and be refused with `CONFLICT` instead of
/// acting on state that changed since it was read.
pub fn cp_approval_revision(conn: &Connection, approval_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT revision FROM cp_approvals WHERE id = ?1",
        params![approval_id],
        |row| row.get(0),
    )
    .optional()
}

/// Resolve every linked delivery record inside the caller's transaction.
/// Returns false for a stale or foreign-owned approval, without reviving it.
pub fn cp_resolve_handoff(
    tx: &Connection,
    approval_id: &str,
    owner_subject: &str,
    resolved_by: &str,
    rejection_reason: Option<&str>,
) -> Result<bool> {
    let linked: Option<(String, String, String)> = tx
        .query_row(
            "SELECT ap.task_id, ap.agent_run_id, ap.artifact_id
         FROM cp_approvals ap
         JOIN cp_tasks t ON t.id = ap.task_id
         JOIN cp_agent_runs r ON r.id = ap.agent_run_id AND r.task_id = t.id
         JOIN cp_artifacts a ON a.id = ap.artifact_id AND a.task_id = t.id
                              AND a.agent_run_id = r.id
         WHERE ap.id = ?1 AND ap.owner_subject = ?2
           AND ap.approval_type = 'handover' AND ap.status = 'pending'
           AND t.status = 'awaiting_approval' AND r.status = 'awaiting_approval'
           AND r.provider = 'handoff' AND a.kind = 'handover_note' AND a.status = 'submitted'",
            params![approval_id, owner_subject],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((task_id, run_id, artifact_id)) = linked else {
        return Ok(false);
    };
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let (approval_status, artifact_status, run_status, task_status, exit_reason) =
        if rejection_reason.is_some() {
            (
                "rejected",
                "rejected",
                "cancelled",
                "cancelled",
                "handover_rejected",
            )
        } else {
            (
                "approved",
                "applied",
                "succeeded",
                "completed",
                "handover_delivered",
            )
        };
    tx.execute(
        "UPDATE cp_approvals SET status = ?1, resolved_at = ?2, resolved_by = ?3,
         reason = CASE WHEN ?4 IS NULL THEN reason ELSE reason || char(10) || 'Rejected: ' || ?4 END,
         revision = revision + 1
         WHERE id = ?5",
        params![
            approval_status,
            now,
            resolved_by,
            rejection_reason,
            approval_id
        ],
    )?;
    tx.execute(
        "UPDATE cp_artifacts SET status = ?1 WHERE id = ?2",
        params![artifact_status, artifact_id],
    )?;
    tx.execute(
        "UPDATE cp_agent_runs SET status = ?1, ended_at = ?2, exit_reason = ?3 WHERE id = ?4",
        params![run_status, now, exit_reason, run_id],
    )?;
    tx.execute(
        "UPDATE cp_tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![task_status, now, task_id],
    )?;
    Ok(true)
}

/// Atomically acknowledge delivery, including audit. Superseded/rejected
/// handoffs remain terminal even if a wrapper holds an old context.
pub fn cp_consume_handoff(
    conn: &Connection,
    ctx: &raios_core::control_plane::HandoffContext,
    resolved_by: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // Bind the acknowledgement to the receiver and the canonical IDs, rather
    // than trusting a caller's cached context after a rejection/supersession.
    let owner: Option<String> = tx
        .query_row(
            "SELECT ap.owner_subject FROM cp_approvals ap JOIN cp_tasks t ON t.id = ap.task_id
         WHERE ap.id = ?1 AND ap.task_id = ?2 AND ap.agent_run_id = ?3
           AND ap.artifact_id = ?4 AND t.assignee_id = ?5",
            params![
                ctx.approval_id,
                ctx.task_id,
                ctx.agent_run_id,
                ctx.artifact_id,
                resolved_by
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(owner) = owner {
        if cp_resolve_handoff(&tx, &ctx.approval_id, &owner, resolved_by, None)? {
            audit_handoff_delivery(&tx, &ctx.approval_id, resolved_by)?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn audit_handoff_delivery(tx: &Connection, approval_id: &str, resolved_by: &str) -> Result<()> {
    crate::security::record_audit_event(
        tx,
        "handover_delivered",
        resolved_by,
        &serde_json::json!({"approval_id": approval_id, "resolved_by": resolved_by}).to_string(),
    )
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
}
