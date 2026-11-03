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
    let Ok(due) = chrono::NaiveDateTime::parse_from_str(&job.next_run_at, "%Y-%m-%dT%H:%M:%SZ")
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
        if let Err(e) =
            raios_core::db::log_activity_event(conn, "scheduler", None, "important", &summary, None)
        {
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

    let spawner: Arc<JobSpawner> = Arc::new(production_spawner);
    let db_opener: Arc<DbOpener> = Arc::new(production_db_opener);

    loop {
        tokio::time::sleep(check_interval).await;

        let conn = match raios_core::db::open_db() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Scheduler] open_db failed: {e}");
                continue;
            }
        };

        // Only wrappers carrying stable process witnesses are auto-recovered.
        // Legacy records require an owner-reviewed maintenance operation.
        if let Err(error) =
            crate::wrapper_liveness::reconcile_sessions(&raios_core::db::db_path(), true, false)
        {
            eprintln!("[Scheduler] Wrapper recovery failed: {error}");
        }

        run_scheduler_pass(&conn, &tx, &spawner, &db_opener);
    }
}

type JobSpawner = dyn Fn(&raios_core::db::ScheduledJob) -> Result<u32, String> + Send + Sync;
type DbOpener = dyn Fn() -> Result<rusqlite::Connection, String> + Send + Sync;

/// Starts the job for real: an extension command or a detached agent run.
fn production_spawner(job: &raios_core::db::ScheduledJob) -> Result<u32, String> {
    match ext_command_from_task_description(&job.task_description) {
        Some((ext_name, command)) => {
            crate::agent_runner::spawn_ext_command_detached(ext_name, command)
        }
        None => {
            let prompt = format!(
                "[SCHEDULED TASK]\nTitle: {}\n\n{}",
                job.title, job.task_description
            );
            crate::agent_runner::spawn_agent_detached(&job.agent, &prompt, None)
        }
    }
}

fn production_db_opener() -> Result<rusqlite::Connection, String> {
    raios_core::db::open_db().map_err(|e| e.to_string())
}

/// One scheduler pass: claim every due job atomically, note the ones that are badly overdue, and
/// start each on its own task so a slow spawn never blocks the loop. The returned handles let a
/// caller (a test) wait for the spawned work; the worker drops them.
fn run_scheduler_pass(
    conn: &rusqlite::Connection,
    tx: &broadcast::Sender<String>,
    spawner: &Arc<JobSpawner>,
    open_db: &Arc<DbOpener>,
) -> Vec<tokio::task::JoinHandle<()>> {
    // Atomic claim — no two instances can claim the same job
    let jobs = match raios_core::db::cp_scheduled_jobs_claim_due(conn) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("[Scheduler] claim failed: {e}");
            vec![]
        }
    };

    let mut handles = Vec::new();
    for job in jobs {
        log_overdue_activity_if_needed(conn, &job);
        let tx = tx.clone();
        let spawner = Arc::clone(spawner);
        let open_db = Arc::clone(open_db);

        handles.push(tokio::spawn(async move {
            let spawn_job = job.clone();
            let outcome = tokio::task::spawn_blocking(move || spawner(&spawn_job)).await;

            let conn = match open_db() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[Scheduler] spawn context open_db failed: {e}");
                    return;
                }
            };
            record_spawn_outcome(&conn, &tx, &job, outcome);
        }));
    }
    handles
}

/// What happens after a job's spawn attempt: success marks it fired, logs it and broadcasts;
/// a spawn error or a panic puts the job back to `active` so it is tried again next interval.
fn record_spawn_outcome(
    conn: &rusqlite::Connection,
    tx: &broadcast::Sender<String>,
    job: &raios_core::db::ScheduledJob,
    outcome: Result<Result<u32, String>, tokio::task::JoinError>,
) {
    let job_id = &job.id;
    let interval = job.interval_secs;
    match outcome {
        Ok(Ok(pid)) => {
            let _ = raios_core::db::cp_scheduled_job_mark_fired(conn, job_id, interval);
            log_fire_success_as_activity(conn, job);
            let evt = serde_json::json!({
                "event": "ScheduledJobFired",
                "id": job_id,
                "agent": job.agent,
                "title": job.title,
                "pid": pid
            });
            let _ = tx.send(evt.to_string());
            println!(
                "[Scheduler] Fired '{}' → {} (pid {pid})",
                job.title, job.agent
            );
        }
        Ok(Err(e)) => {
            let _ = raios_core::db::cp_scheduled_job_revert_firing(conn, job_id, interval);
            eprintln!(
                "[Scheduler] Spawn failed for '{}' (agent '{}'): {e}",
                job.title, job.agent
            );
        }
        Err(join_err) => {
            let _ = raios_core::db::cp_scheduled_job_revert_firing(conn, job_id, interval);
            eprintln!(
                "[Scheduler] Spawn task panicked for '{}' (agent '{}'): {join_err}",
                job.title, job.agent
            );
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

    // ─── run_scheduler_pass ──────────────────────────────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fx {
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        conn: Connection,
        tx: broadcast::Sender<String>,
        rx: broadcast::Receiver<String>,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test.db");
            let conn = Connection::open(&path).unwrap();
            raios_core::db::migrate_existing(&conn).unwrap();
            let (tx, rx) = broadcast::channel(8);
            Fx {
                _dir: dir,
                path,
                conn,
                tx,
                rx,
            }
        }

        /// A job whose `next_run_at` is `due_in_secs` from now (negative = already due).
        fn job(&self, title: &str, due_in_secs: i64) -> String {
            let id =
                raios_core::db::cp_scheduled_job_create(&self.conn, title, "claude", "do it", 3600)
                    .unwrap();
            self.conn
                .execute(
                    "UPDATE cp_scheduled_jobs SET next_run_at =
                     strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?2 || ' seconds') WHERE id = ?1",
                    rusqlite::params![id, due_in_secs],
                )
                .unwrap();
            id
        }

        fn opener(&self) -> Arc<DbOpener> {
            let path = self.path.clone();
            Arc::new(move || Connection::open(&path).map_err(|e| e.to_string()))
        }

        async fn run(&self, spawner: Arc<JobSpawner>) {
            let handles = run_scheduler_pass(&self.conn, &self.tx, &spawner, &self.opener());
            for h in handles {
                h.await.unwrap();
            }
        }

        fn row(&self, id: &str) -> (String, i64, String) {
            self.conn
                .query_row(
                    "SELECT status, run_count, next_run_at FROM cp_scheduled_jobs WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        }
    }

    fn counting(calls: Arc<AtomicUsize>, result: Result<u32, String>) -> Arc<JobSpawner> {
        Arc::new(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            result.clone()
        })
    }

    #[tokio::test]
    async fn a_due_job_is_started_once_marked_fired_logged_and_broadcast() {
        let mut fx = Fx::new();
        let id = fx.job("nightly", -60);
        let calls = Arc::new(AtomicUsize::new(0));

        fx.run(counting(calls.clone(), Ok(4242))).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (status, runs, next) = fx.row(&id);
        assert_eq!((status.as_str(), runs), ("active", 1));
        assert!(next > chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
        let evt = fx.rx.try_recv().expect("no ScheduledJobFired broadcast");
        assert!(evt.contains("ScheduledJobFired") && evt.contains("4242"));
        let logged: i64 = fx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE source='scheduler' AND tier='routine'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1);
    }

    #[tokio::test]
    async fn a_job_that_is_not_due_is_left_alone() {
        let fx = Fx::new();
        let id = fx.job("later", 3600);
        let calls = Arc::new(AtomicUsize::new(0));

        fx.run(counting(calls.clone(), Ok(1))).await;

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fx.row(&id).0, "active");
    }

    #[tokio::test]
    async fn a_failed_spawn_puts_the_job_back_without_counting_a_run() {
        let mut fx = Fx::new();
        let id = fx.job("broken", -60);

        fx.run(counting(
            Arc::new(AtomicUsize::new(0)),
            Err("no such agent".into()),
        ))
        .await;

        let (status, runs, next) = fx.row(&id);
        assert_eq!((status.as_str(), runs), ("active", 0));
        assert!(next > chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
        assert!(
            fx.rx.try_recv().is_err(),
            "a failure must not announce a fire"
        );
    }

    #[tokio::test]
    async fn a_panicking_spawn_is_contained_and_the_job_is_retried_later() {
        let fx = Fx::new();
        let id = fx.job("panics", -60);
        let other = fx.job("fine", -60);
        let spawner: Arc<JobSpawner> = Arc::new(|job| {
            if job.title == "panics" {
                panic!("boom");
            }
            Ok(7)
        });

        fx.run(spawner).await;

        assert_eq!(fx.row(&id).0, "active");
        assert_eq!(fx.row(&id).1, 0);
        assert_eq!(
            fx.row(&other).1,
            1,
            "one job's panic must not stop the others"
        );
    }

    #[tokio::test]
    async fn a_claimed_job_is_not_claimed_again_by_the_next_pass() {
        let fx = Fx::new();
        fx.job("once", -60);
        let calls = Arc::new(AtomicUsize::new(0));
        let spawner = counting(calls.clone(), Ok(1));
        let opener = fx.opener();

        let mut handles = run_scheduler_pass(&fx.conn, &fx.tx, &spawner, &opener);
        handles.extend(run_scheduler_pass(&fx.conn, &fx.tx, &spawner, &opener));
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
