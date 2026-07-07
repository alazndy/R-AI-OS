use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use crate::daemon::state::DaemonState;

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
            let prompt = format!(
                "[SCHEDULED TASK]\nTitle: {}\n\n{}",
                job.title, job.task_description
            );
            let agent = job.agent.clone();
            let job_id = job.id.clone();
            let interval = job.interval_secs;
            let tx_clone = tx.clone();

            // Spawn task to prevent blocking the scheduler loop
            tokio::spawn(async move {
                let spawn_result = tokio::task::spawn_blocking(move || {
                    crate::agent_runner::spawn_agent_detached(&agent, &prompt, None)
                }).await;

                let conn = match raios_core::db::open_db() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Scheduler] spawn context open_db failed: {e}");
                        return;
                    }
                };

                match spawn_result {
                    Ok(Ok(pid)) => {
                        let _ = raios_core::db::cp_scheduled_job_mark_fired(&conn, &job_id, interval);
                        let evt = serde_json::json!({
                            "event": "ScheduledJobFired",
                            "id": job_id,
                            "agent": job.agent,
                            "title": job.title,
                            "pid": pid
                        });
                        let _ = tx_clone.send(evt.to_string());
                        println!("[Scheduler] Fired '{}' → {} (pid {pid})", job.title, job.agent);
                    }
                    Ok(Err(e)) => {
                        let _ = raios_core::db::cp_scheduled_job_revert_firing(&conn, &job_id);
                        eprintln!(
                            "[Scheduler] Spawn failed for '{}' (agent '{}'): {e}",
                            job.title, job.agent
                        );
                    }
                    Err(join_err) => {
                        let _ = raios_core::db::cp_scheduled_job_revert_firing(&conn, &job_id);
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
