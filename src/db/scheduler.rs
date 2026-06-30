use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
pub fn cp_scheduled_job_create(
    conn: &Connection,
    title: &str,
    agent: &str,
    task_description: &str,
    interval_secs: i64,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO cp_scheduled_jobs (
            id, title, agent, task_description, project_id, interval_secs, status,
            last_run_at, next_run_at, created_at, run_count
         ) VALUES (
            ?1, ?2, ?3, ?4, NULL, ?5, 'active',
            NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?5 || ' seconds'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 0
         )",
        params![id, title, agent, task_description, interval_secs],
    )?;
    Ok(id)
}

pub fn cp_scheduled_jobs_list(conn: &Connection) -> Result<Vec<ScheduledJob>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, agent, task_description, project_id, interval_secs, status, last_run_at, next_run_at, created_at, run_count
         FROM cp_scheduled_jobs
         WHERE status != 'deleted'
         ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ScheduledJob {
            id: row.get(0)?,
            title: row.get(1)?,
            agent: row.get(2)?,
            task_description: row.get(3)?,
            project_id: row.get(4)?,
            interval_secs: row.get(5)?,
            status: row.get(6)?,
            last_run_at: row.get(7)?,
            next_run_at: row.get(8)?,
            created_at: row.get(9)?,
            run_count: row.get(10)?,
        })
    })?;
    let mut jobs = Vec::new();
    for r in rows {
        jobs.push(r?);
    }
    Ok(jobs)
}

pub fn cp_scheduled_jobs_claim_due(conn: &Connection) -> Result<Vec<ScheduledJob>> {
    let mut stmt = conn.prepare(
        "UPDATE cp_scheduled_jobs
         SET status = 'firing'
         WHERE status = 'active' AND next_run_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         RETURNING id, title, agent, task_description, project_id, interval_secs, status, last_run_at, next_run_at, created_at, run_count"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ScheduledJob {
            id: row.get(0)?,
            title: row.get(1)?,
            agent: row.get(2)?,
            task_description: row.get(3)?,
            project_id: row.get(4)?,
            interval_secs: row.get(5)?,
            status: row.get(6)?,
            last_run_at: row.get(7)?,
            next_run_at: row.get(8)?,
            created_at: row.get(9)?,
            run_count: row.get(10)?,
        })
    })?;
    let mut jobs = Vec::new();
    for r in rows {
        jobs.push(r?);
    }
    Ok(jobs)
}

pub fn cp_scheduled_job_mark_fired(conn: &Connection, id: &str, interval_secs: i64) -> Result<()> {
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET status = 'active',
             last_run_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
             next_run_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?2 || ' seconds'),
             run_count = run_count + 1
         WHERE id = ?1",
        params![id, interval_secs],
    )?;
    Ok(())
}

pub fn cp_scheduled_job_revert_firing(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET status = 'active'
         WHERE id = ?1 AND status = 'firing'",
        params![id],
    )?;
    Ok(())
}

pub fn cp_scheduled_jobs_reset_firing(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET status = 'active'
         WHERE status = 'firing'",
        [],
    )?;
    Ok(())
}

pub fn cp_scheduled_job_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET status = 'deleted'
         WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn cp_scheduled_job_set_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    if status != "active" && status != "paused" {
        return Err(rusqlite::Error::ToSqlConversionFailure(
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Status must be active or paused",
            )),
        ));
    }
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET status = ?2
         WHERE id = ?1 AND status != 'deleted'",
        params![id, status],
    )?;
    Ok(())
}

pub fn cp_scheduled_job_trigger_now(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE cp_scheduled_jobs
         SET next_run_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?1 AND status != 'deleted'",
        params![id],
    )?;
    Ok(())
}

