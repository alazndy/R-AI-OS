use crate::daemon::state::DaemonState;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

/// Auto-lifecycle worker: transitions project statuses based on git activity.
///
/// Transitions:
///   active      → beklemede  if no commit for `standby_days`
///   beklemede   → archived   if no commit for `archive_days`
///   beklemede / archived → active  if a new commit is detected
///
/// "Manually pinned" statuses (production, early, legacy) are never touched.
pub async fn start_lifecycle_worker(
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    interval: Duration,
    standby_days: u64,
    archive_days: u64,
) {
    println!(
        "[Lifecycle] Worker started — standby={}d archive={}d interval={}s",
        standby_days,
        archive_days,
        interval.as_secs()
    );

    // Initial delay — let other workers settle first
    sleep(Duration::from_secs(90)).await;

    let standby_secs = standby_days * 86_400;
    let archive_secs = archive_days * 86_400;

    loop {
        let projects = {
            let s = state.read().await;
            s.projects.clone()
        };

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let conn = match raios_core::db::open_db() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Lifecycle] DB open failed: {e}");
                sleep(interval).await;
                continue;
            }
        };

        let mut updated = false;

        for proj in &projects {
            // Skip projects without git
            if !proj.local_path.join(".git").exists() {
                continue;
            }

            // Skip manually pinned statuses
            let current = proj.status.as_str();
            if matches!(current, "production" | "early" | "legacy" | "waiting") {
                continue;
            }

            // Get last commit timestamp (Unix epoch seconds)
            let last_commit_secs = last_commit_timestamp(&proj.local_path);
            let Some(last_ts) = last_commit_secs else {
                continue;
            };

            let age_secs = now_secs.saturating_sub(last_ts);
            let path_str = proj.local_path.to_string_lossy().to_string();

            let new_status = if age_secs < standby_secs {
                // Recent activity → active
                if matches!(current, "beklemede" | "archived") {
                    Some("active")
                } else {
                    None // already active, no change
                }
            } else if age_secs < archive_secs {
                // Stale but not dead → beklemede
                if current != "beklemede" {
                    Some("beklemede")
                } else {
                    None
                }
            } else {
                // Long inactive → archived
                if current != "archived" {
                    Some("archived")
                } else {
                    None
                }
            };

            if let Some(status) = new_status {
                if let Err(e) = raios_core::db::update_project_status(&conn, &path_str, status) {
                    eprintln!("[Lifecycle] Failed to update {}: {e}", proj.name);
                } else {
                    println!(
                        "[Lifecycle] {} → {} (age: {}d)",
                        proj.name,
                        status,
                        age_secs / 86_400
                    );
                    updated = true;
                }
            }
        }

        if updated {
            // Reload state from DB and broadcast
            let mut s = state.write().await;
            if let Ok(fresh) = raios_core::db::load_all_projects(&conn) {
                s.projects = fresh
                    .into_iter()
                    .filter(|r| std::path::Path::new(&r.path).exists() && r.status != "waiting")
                    .map(|r| raios_core::entities::EntityProject {
                        name: r.name,
                        category: r.category,
                        local_path: std::path::PathBuf::from(r.path),
                        github: r.github,
                        status: r.status,
                        stars: r.stars.map(|s| s as u32),
                        last_commit: r.last_commit,
                        version: r.version,
                        version_nickname: r.nickname,
                    })
                    .collect();
                let msg = s.sync_payload();
                let _ = tx.send(msg.to_string());
            }
        }

        sleep(interval).await;
    }
}

fn last_commit_timestamp(repo: &std::path::Path) -> Option<u64> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(["log", "-1", "--format=%ct"])
        .output()
        .ok()?;

    let ts_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
    ts_str.parse::<u64>().ok()
}
