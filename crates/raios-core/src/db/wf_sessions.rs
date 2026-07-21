use super::*;
use rusqlite::{params, Connection, Result};
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
pub fn cp_session_end(conn: &Connection, task_id: &str, run_id: &str, success: bool) -> Result<()> {
    cp_session_end_with_summary(conn, task_id, run_id, success, None)
}

pub fn cp_session_end_with_summary(
    conn: &Connection,
    task_id: &str,
    run_id: &str,
    success: bool,
    summary: Option<&str>,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let run_status = if success { "succeeded" } else { "failed" };
    let task_status = if success { "completed" } else { "failed" };
    let exit_reason = if success {
        "clean_exit"
    } else {
        "nonzero_exit"
    };

    conn.execute(
        "UPDATE cp_agent_runs
         SET status=?1, ended_at=?2, exit_reason=?3, summary=COALESCE(?4, summary)
         WHERE id=?5",
        params![run_status, now, exit_reason, summary, run_id],
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
    pub summary: Option<String>,
}

pub fn cp_sessions_list(conn: &Connection, limit: usize) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_name, status, started_at, ended_at, exit_reason, summary
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
            summary: r.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>>>()
}
