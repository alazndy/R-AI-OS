use crate::agent_runner::ext_command_from_task_description;
use crate::daemon::state::DaemonState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

/// Log a routine `activity_events` row when a scheduled job fires successfully.
fn log_fire_success_as_activity(conn: &rusqlite::Connection, job: &raios_core::db::ScheduledJob) {
    let summary = format!("'{}' fired ({})", job.title, job.agent);
    if let Err(e) =
        raios_core::db::log_activity_event(conn, "scheduler", None, "routine", &summary, None)
    {
        eprintln!("[Scheduler] Failed to log fire-success activity event: {e}");
    }
}

/// Log an important `activity_events` row when a claimed job is more than 2x
/// its own interval overdue (comparing `next_run_at` against now). A no-op
/// (and silently skipped) if `next_run_at` fails to parse.
fn log_overdue_activity_if_needed(conn: &rusqlite::Connection, job: &raios_core::db::ScheduledJob) {
    let Ok(due) =
        chrono::NaiveDateTime::parse_from_str(&job.next_run_at, "%Y-%m-%dT%H:%M:%SZ")
    else {
        return;
    };
    let due = due.and_utc();
    let overdue_secs = (chrono::Utc::now() - due).num_seconds();

    if overdue_secs > 2 * job.interval_secs {
        let summary = format!(
            "'{}' is overdue by {}m (interval {}m)",
            job.title,
            overdue_secs / 60,
            job.interval_secs / 60
        );
        if let Err(e) = raios_core::db::log_activity_event(
            conn, "scheduler", None, "important", &summary, None,
        ) {
            eprintln!("[Scheduler] Failed to log overdue activity event: {e}");
        }
    }
}

pub async fn start_scheduler_worker(
    _state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    check_interval: Duration,
) {
    println!("[Scheduler] Worker starting up...");

    // Startup crash recovery (first tick)
    if let Ok(conn) = raios_core::db::open_db() {
        if let Err(e) = raios_core::db::cp_scheduled_jobs_reset_firing(&conn) {
            eprintln!("[Scheduler] Crash recovery reset failed: {e}");
        } else {
            println!("[Scheduler] Crash recovery complete (reset 'firing' -> 'active').");
        }
    }

    loop {
        tokio::time::sleep(check_interval).await;

        let conn = match raios_core::db::open_db() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Scheduler] open_db failed: {e}");
                continue;
            }
        };

        // Atomic claim — no two instances can claim the same job
        let jobs = match raios_core::db::cp_scheduled_jobs_claim_due(&conn) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("[Scheduler] claim failed: {e}");
                vec![]
            }
        };

        for job in jobs {
            log_overdue_activity_if_needed(&conn, &job);

            let prompt = format!(
                "[SCHEDULED TASK]\nTitle: {}\n\n{}",
                job.title, job.task_description
            );
            let agent = job.agent.clone();
            let task_description = job.task_description.clone();
            let job_id = job.id.clone();
            let interval = job.interval_secs;
            let tx_clone = tx.clone();

            // Spawn task to prevent blocking the scheduler loop
            tokio::spawn(async move {
                let spawn_result = tokio::task::spawn_blocking(move || {
                    match ext_command_from_task_description(&task_description) {
                        Some((ext_name, command)) => {
                            crate::agent_runner::spawn_ext_command_detached(ext_name, command)
                        }
                        None => crate::agent_runner::spawn_agent_detached(&agent, &prompt, None),
                    }
                })
                .await;

                let conn = match raios_core::db::open_db() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Scheduler] spawn context open_db failed: {e}");
                        return;
                    }
                };

                match spawn_result {
                    Ok(Ok(pid)) => {
                        let _ =
                            raios_core::db::cp_scheduled_job_mark_fired(&conn, &job_id, interval);
                        log_fire_success_as_activity(&conn, &job);
                        let evt = serde_json::json!({
                            "event": "ScheduledJobFired",
                            "id": job_id,
                            "agent": job.agent,
                            "title": job.title,
                            "pid": pid
                        });
                        let _ = tx_clone.send(evt.to_string());
                        println!(
                            "[Scheduler] Fired '{}' → {} (pid {pid})",
                            job.title, job.agent
                        );
                    }
                    Ok(Err(e)) => {
                        let _ = raios_core::db::cp_scheduled_job_revert_firing(
                            &conn, &job_id, interval,
                        );
                        eprintln!(
                            "[Scheduler] Spawn failed for '{}' (agent '{}'): {e}",
                            job.title, job.agent
                        );
                    }
                    Err(join_err) => {
                        let _ = raios_core::db::cp_scheduled_job_revert_firing(
                            &conn, &job_id, interval,
                        );
                        eprintln!(
                            "[Scheduler] Spawn task panicked for '{}' (agent '{}'): {join_err}",
                            job.title, job.agent
                        );
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use raios_core::db::ScheduledJob;
    use rusqlite::Connection;

    fn job_due_at(next_run_at: &str, interval_secs: i64) -> ScheduledJob {
        ScheduledJob {
            id: "job-1".into(),
            title: "JSON Backup".into(),
            agent: "claude".into(),
            task_description: "backup".into(),
            project_id: None,
            interval_secs,
            status: "active".into(),
            last_run_at: None,
            next_run_at: next_run_at.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            run_count: 0,
        }
    }

    #[test]
    fn log_fire_success_as_activity_writes_routine_row() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        log_fire_success_as_activity(&conn, &job_due_at("2026-08-17T12:00:00Z", 3600));

        let tier: String = conn
            .query_row(
                "SELECT tier FROM activity_events WHERE source = 'scheduler'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tier, "routine");
    }

    #[test]
    fn overdue_important_event_fires_when_job_is_more_than_2x_interval_late() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s (1h), next_run_at was due 3 hours ago -> overdue by 3x interval
        let three_hours_ago = (chrono::Utc::now() - chrono::Duration::hours(3))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&three_hours_ago, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let tier: String = conn
            .query_row(
                "SELECT tier FROM activity_events WHERE source = 'scheduler'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tier, "important");
    }

    #[test]
    fn overdue_important_event_does_not_fire_when_job_is_only_slightly_late() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s, due 5 minutes ago -> not overdue by the 2x threshold
        let five_min_ago = (chrono::Utc::now() - chrono::Duration::minutes(5))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&five_min_ago, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn overdue_important_event_does_not_fire_at_exactly_2x_interval() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s, threshold is overdue_secs > 7200. Due exactly 7200s ago sits
        // right ON the boundary and must NOT fire, since the requirement is strictly `>`.
        //
        // The reference instant is truncated to a whole second (`with_nanosecond(0)`)
        // before subtracting, so the fixture's `due` timestamp (formatted with
        // second-granularity, no fractional part) has zero sub-second error relative to
        // it. Without this, the function's own `Utc::now()` call — made a few
        // microseconds later — could occasionally round the computed `overdue_secs` up
        // by one across the exact boundary, flaking this test at the sub-millisecond
        // level. Truncating first makes the elapsed whole-second count deterministic.
        let now_aligned = chrono::Utc::now().with_nanosecond(0).unwrap();
        let exactly_at_threshold = (now_aligned - chrono::Duration::seconds(7200))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&exactly_at_threshold, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "exactly 2x interval overdue must NOT fire (threshold is strictly '>', not '>=')"
        );
    }

    #[test]
    fn overdue_important_event_fires_one_second_past_2x_interval() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s, threshold is overdue_secs > 7200. Due 7201s ago is one second
        // past the boundary and must fire. Same whole-second alignment as the sibling
        // boundary test above, for the same determinism reason.
        let now_aligned = chrono::Utc::now().with_nanosecond(0).unwrap();
        let one_second_past_threshold = (now_aligned - chrono::Duration::seconds(7201))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&one_second_past_threshold, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let tier: String = conn
            .query_row(
                "SELECT tier FROM activity_events WHERE source = 'scheduler'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tier, "important");
    }
}
