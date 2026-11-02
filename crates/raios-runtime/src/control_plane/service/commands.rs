//! One function per control-plane command.
//!
//! Each runs inside the dispatcher's transaction (`tx`): returning `Err` rolls
//! everything back, so a refused command leaves no trace and its idempotency
//! key stays usable. Side effects that must only happen once the transaction
//! has committed (typed events, starting an agent) are queued in [`Effects`]
//! instead of being performed here.

use super::*;

/// Work queued by a command for after the transaction commits.
#[derive(Default)]
pub(super) struct Effects {
    /// Typed events, published only once the data they describe is durable.
    pub(super) events: Vec<Event>,
    /// An agent to start in a terminal.
    pub(super) launch: Option<AgentLaunchRequest>,
}

type CommandResult = Result<Option<serde_json::Value>, Problem>;

pub(super) fn approve_handoff(
    tx: &Connection,
    actor: &ControlActor,
    approval_id: &str,
    expected_revision: Option<i64>,
    effects: &mut Effects,
) -> CommandResult {
    require_expected_revision(tx, approval_id, expected_revision)?;
    let run_id = handoff_run_id(tx, approval_id)?;
    // The owner's consent: the approval resolves as approved and the run leaves the
    // queue, but the task stays open. Only the addressed agent's own wrapper marks
    // delivery (`handover_delivered`).
    if !raios_core::db::cp_approve_handoff_by_owner(
        tx,
        approval_id,
        actor.subject(),
        actor.subject(),
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::not_found("Pending handoff approval not found"));
    }
    push_handoff_resolution_events(
        tx,
        approval_id,
        run_id.as_deref(),
        "approved",
        &mut effects.events,
    )?;
    Ok(Some(
        serde_json::json!({"approval_id": approval_id, "status": "approved"}),
    ))
}

pub(super) fn reject_handoff(
    tx: &Connection,
    actor: &ControlActor,
    approval_id: &str,
    expected_revision: Option<i64>,
    reason: &str,
    effects: &mut Effects,
) -> CommandResult {
    require_expected_revision(tx, approval_id, expected_revision)?;
    if reason.trim().is_empty() || reason.len() > 2_000 {
        return Err(Problem::invalid_input(
            "Rejection reason must contain at most 2000 characters",
        ));
    }
    let run_id = handoff_run_id(tx, approval_id)?;
    if !raios_core::db::cp_resolve_handoff(
        tx,
        approval_id,
        actor.subject(),
        actor.subject(),
        Some(reason),
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::not_found("Pending handoff approval not found"));
    }
    push_handoff_resolution_events(
        tx,
        approval_id,
        run_id.as_deref(),
        "rejected",
        &mut effects.events,
    )?;
    Ok(Some(
        serde_json::json!({"approval_id": approval_id, "status": "rejected"}),
    ))
}

pub(super) fn create_handoff(
    tx: &Connection,
    project_path: &str,
    from_agent: &str,
    to_agent: &str,
    summary: &str,
    effects: &mut Effects,
) -> CommandResult {
    let result = crate::control_plane::handoff::create_handoff(
        tx,
        project_path,
        from_agent,
        to_agent,
        summary,
    )?;
    push_handoff_created_events(tx, &result, to_agent, &mut effects.events)?;
    Ok(Some(result))
}

/// Both cron commands act on a job that exists and has not been deleted.
fn require_scheduled_job(tx: &Connection, job_id: &str) -> Result<(), Problem> {
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM cp_scheduled_jobs WHERE id = ?1 AND status != 'deleted'",
            params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| Problem::internal(e.to_string()))?;
    if exists.is_none() {
        return Err(Problem::not_found("Scheduled job not found"));
    }
    Ok(())
}

pub(super) fn trigger_cron_job(tx: &Connection, job_id: &str) -> CommandResult {
    require_scheduled_job(tx, job_id)?;
    raios_core::db::cp_scheduled_job_trigger_now(tx, job_id)
        .map_err(|e| Problem::internal(e.to_string()))?;
    Ok(Some(
        serde_json::json!({"job_id": job_id, "status": "scheduled_now"}),
    ))
}

pub(super) fn toggle_cron_job(tx: &Connection, job_id: &str, paused: bool) -> CommandResult {
    require_scheduled_job(tx, job_id)?;
    let status_str = if paused { "paused" } else { "active" };
    raios_core::db::cp_scheduled_job_set_status(tx, job_id, status_str)
        .map_err(|e| Problem::internal(e.to_string()))?;
    Ok(Some(
        serde_json::json!({"job_id": job_id, "status": status_str}),
    ))
}

pub(super) fn launch_agent(
    tx: &Connection,
    agent_name: &str,
    project_path: &str,
    prompt: Option<&str>,
    effects: &mut Effects,
) -> CommandResult {
    let request =
        crate::control_plane::agent_launch::validate_launch(tx, agent_name, project_path, prompt)?;
    let result = crate::control_plane::agent_launch::launch_result(&request, "launching");
    effects.launch = Some(request);
    Ok(Some(result))
}

pub(super) fn validate_agent_launch(
    tx: &Connection,
    agent_name: &str,
    project_path: &str,
    prompt: Option<&str>,
) -> CommandResult {
    let request =
        crate::control_plane::agent_launch::validate_launch(tx, agent_name, project_path, prompt)?;
    Ok(Some(crate::control_plane::agent_launch::launch_result(
        &request,
        "validated",
    )))
}

pub(super) fn create_task(
    tx: &Connection,
    title: &str,
    project_path: Option<&str>,
    priority: u8,
) -> CommandResult {
    let title = validate_task_title(title)?;
    let project_path = validate_project_path(project_path)?;
    let project_id = project_path
        .as_deref()
        .map(|path| {
            tx.query_row(
                "SELECT id FROM projects WHERE path = ?1",
                params![path],
                |row| row.get::<_, i64>(0),
            )
            .optional()
        })
        .transpose()
        .map_err(|error| Problem::internal(error.to_string()))?
        .flatten();
    let task_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let display_order: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(display_order), -1) + 1 FROM cp_task_list_items",
            [],
            |row| row.get(0),
        )
        .map_err(|error| Problem::internal(error.to_string()))?;

    tx.execute(
        "INSERT INTO cp_tasks
         (id, project_id, plan_id, parent_task_id, title, description, priority, status,
          assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
         VALUES (?1, ?2, NULL, NULL, ?3, '', ?4, 'queued',
                 NULL, NULL, '', ?5, ?5)",
        params![task_id, project_id, title, i64::from(priority), now],
    )
    .map_err(|error| Problem::internal(error.to_string()))?;
    tx.execute(
        "INSERT INTO cp_task_list_items
         (task_id, source_kind, source_path, display_order, project_name, created_at)
         VALUES (?1, 'tui', 'control-plane', ?2, ?3, ?4)",
        params![task_id, display_order, project_path, now],
    )
    .map_err(|error| Problem::internal(error.to_string()))?;

    Ok(Some(serde_json::json!({
        "task_id": task_id,
        "status": "queued",
    })))
}

pub(super) fn update_task_status(tx: &Connection, task_id: &str, status: &str) -> CommandResult {
    let status = status.trim();
    if task_id.trim().is_empty() || !OPERATOR_TASK_STATUSES.contains(&status) {
        return Err(Problem::invalid_input(
            "Task id and an allowed operator task status are required",
        ));
    }
    let now = Utc::now().to_rfc3339();
    let changed = tx
        .execute(
            &format!(
                "UPDATE cp_tasks
                 SET status = ?1, updated_at = ?2
                 WHERE id = ?3 AND {OPERATOR_TASK_CONDITIONS}"
            ),
            params![status, now, task_id],
        )
        .map_err(|error| Problem::internal(error.to_string()))?;
    if changed != 1 {
        return Err(Problem::not_found(
            "Operator-managed personal task was not found or cannot be updated",
        ));
    }
    Ok(Some(
        serde_json::json!({"task_id": task_id, "status": status}),
    ))
}

pub(super) fn assign_task(
    tx: &Connection,
    task_id: &str,
    assignee: Option<&str>,
    expected_updated_at: Option<&str>,
) -> CommandResult {
    let assignee = assignee.map(str::trim);
    if task_id.trim().is_empty() || assignee.is_some_and(|name| !TASK_ASSIGNEES.contains(&name)) {
        return Err(Problem::invalid_input(
            "A task id and, when given, one of the known agents are required",
        ));
    }
    let current: Option<String> = tx
        .query_row(
            &format!(
                "SELECT updated_at FROM cp_tasks WHERE id = ?1 AND {OPERATOR_TASK_CONDITIONS}"
            ),
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| Problem::internal(error.to_string()))?;
    let Some(current) = current else {
        return Err(Problem::not_found(
            "Operator-managed personal task was not found or cannot be updated",
        ));
    };
    if expected_updated_at.is_some_and(|expected| expected != current) {
        return Err(Problem::conflict(
            "The task changed since it was read; reload it and try again",
        ));
    }
    let now = Utc::now().to_rfc3339();
    let changed = tx
        .execute(
            &format!(
                "UPDATE cp_tasks
                 SET assignee_kind = ?1, assignee_id = ?2, updated_at = ?3
                 WHERE id = ?4 AND updated_at = ?5 AND {OPERATOR_TASK_CONDITIONS}"
            ),
            params![assignee.map(|_| "agent"), assignee, now, task_id, current],
        )
        .map_err(|error| Problem::internal(error.to_string()))?;
    if changed != 1 {
        return Err(Problem::conflict(
            "The task changed while it was being assigned; try again",
        ));
    }
    Ok(Some(serde_json::json!({
        "task_id": task_id,
        "assignee": assignee,
        "updated_at": now,
    })))
}
