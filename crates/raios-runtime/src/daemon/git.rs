use crate::daemon::state::DaemonState;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{sleep, Duration};

/// Background worker that periodically updates Git status for all projects.
pub async fn start_git_worker(
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    interval: Duration,
) {
    println!("[Daemon] Git Worker started.");

    // Stagger startup to avoid CPU spike with health worker
    sleep(Duration::from_secs(60)).await;

    loop {
        let projects = {
            let s = state.read().await;
            s.projects.clone()
        };

        if projects.is_empty() {
            sleep(Duration::from_secs(5)).await;
            continue;
        }

        println!(
            "[Daemon] Scanning Git status for {} projects...",
            projects.len()
        );

        let conn = raios_core::db::open_db().ok();
        let mut updated = false;
        {
            let mut s = state.write().await;
            for proj in s.projects.iter_mut() {
                if proj.local_path.join(".git").exists() {
                    // 1. Get branch name
                    let branch = Command::new("git")
                        .current_dir(&proj.local_path)
                        .args(["rev-parse", "--abbrev-ref", "HEAD"])
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    // 2. Get dirty status
                    let dirty = Command::new("git")
                        .current_dir(&proj.local_path)
                        .args(["status", "--porcelain"])
                        .output()
                        .ok()
                        .map(|o| !o.stdout.is_empty())
                        .unwrap_or(false);

                    // Update status string for now
                    let new_status = if dirty {
                        format!("{} (dirty)", branch)
                    } else {
                        branch
                    };

                    if proj.status != new_status {
                        if let Some(ref conn) = conn {
                            log_status_change_as_activity(
                                conn,
                                &proj.name,
                                &proj.status,
                                &new_status,
                            );
                        }
                        proj.status = new_status;
                        updated = true;
                    }

                    // 3. GitHub Sync (if github URL exists)
                    if let Some(ref gh_url) = proj.github {
                        if gh_url.contains("github.com") {
                            // Try to get stars and last update via gh api
                            let repo = gh_url
                                .trim_end_matches(".git")
                                .split("github.com/")
                                .last()
                                .unwrap_or("");

                            if !repo.is_empty() {
                                let output = Command::new("gh")
                                    .args([
                                        "api",
                                        &format!("repos/{}", repo),
                                        "--template",
                                        "{{.stargazers_count}}|{{.updated_at}}",
                                    ])
                                    .output();

                                if let Ok(o) = output {
                                    let res = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                    let parts: Vec<&str> = res.split('|').collect();
                                    if parts.len() == 2 {
                                        let stars = parts[0].parse::<u32>().ok();
                                        let last = parts[1].to_string();

                                        if proj.stars != stars
                                            || proj.last_commit.as_ref() != Some(&last)
                                        {
                                            proj.stars = stars;
                                            proj.last_commit = Some(last);
                                            updated = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if updated {
                println!("[Daemon] Git statuses updated. Broadcasting StateSync.");
                let msg = s.sync_payload();
                let _ = tx.send(msg.to_string());
            }
        }

        // Wait before next scan (e.g., 2 minutes)
        sleep(interval).await;
    }
}

/// Log a routine `activity_events` row for a project's git status change.
/// Best-effort: a failed write is logged to stderr but never propagated —
/// this must never break the git-status update itself.
fn log_status_change_as_activity(
    conn: &rusqlite::Connection,
    project_name: &str,
    old_status: &str,
    new_status: &str,
) {
    let summary = format!("{project_name}: {old_status} → {new_status}");
    if let Err(e) = raios_core::db::log_activity_event(
        conn,
        "git",
        Some(project_name),
        "routine",
        &summary,
        None,
    ) {
        eprintln!("[Daemon] Failed to log git activity event for {project_name}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_status_change_as_activity_writes_routine_row_with_old_and_new_status() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap(); // see Task 5 note on locating the real helper name

        log_status_change_as_activity(&conn, "demo-project", "active", "beklemede");

        let (tier, summary): (String, String) = conn
            .query_row(
                "SELECT tier, summary FROM activity_events WHERE source = 'git'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(tier, "routine");
        // Exact-structure assertion: "active" and "beklemede" share no substring
        // relationship, so this can only pass if the *distinct* old and new
        // status values both landed in their correct position around the
        // arrow — it cannot pass under a log-after-mutate bug where both
        // arguments accidentally carry the new status.
        assert_eq!(summary, "demo-project: active → beklemede");
    }
}
