use crate::daemon::state::DaemonState;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

const ACTIVITY_EVENT_RETENTION_DAYS: i64 = 30;

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
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        match raios_core::db::open_db() {
            Ok(mut conn) => {
                run_lifecycle_pass(
                    &state,
                    &tx,
                    &mut conn,
                    now_secs,
                    standby_secs,
                    archive_secs,
                    &last_commit_timestamp,
                )
                .await;
            }
            Err(e) => eprintln!("[Lifecycle] DB open failed: {e}"),
        }

        sleep(interval).await;
    }
}

/// One pass of the lifecycle worker: prune old activity rows, move each automatic-status project
/// to the status its last commit age calls for, and broadcast fresh state when anything moved.
/// `last_commit` returns a repo's last commit time (Unix seconds); the worker passes the real
/// `git log`, tests pass a fake. Returns whether any project changed status.
async fn run_lifecycle_pass(
    state: &RwLock<DaemonState>,
    tx: &broadcast::Sender<String>,
    conn: &mut rusqlite::Connection,
    now_secs: u64,
    standby_secs: u64,
    archive_secs: u64,
    last_commit: &(dyn Fn(&std::path::Path) -> Option<u64> + Sync),
) -> bool {
    let projects = {
        let s = state.read().await;
        s.projects.clone()
    };

    prune_stale_activity_events(conn);

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
        let last_commit_secs = last_commit(&proj.local_path);
        let Some(last_ts) = last_commit_secs else {
            continue;
        };

        let age_secs = now_secs.saturating_sub(last_ts);
        let path_str = proj.local_path.to_string_lossy().to_string();

        let new_status = next_lifecycle_status(current, age_secs, standby_secs, archive_secs);

        if let Some(status) = new_status {
            if let Err(e) = raios_core::db::update_project_status(conn, &path_str, status) {
                eprintln!("[Lifecycle] Failed to update {}: {e}", proj.name);
            } else {
                log_transition_as_activity(conn, &proj.name, current, status);
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
        if let Ok(fresh) = raios_core::db::load_all_projects(conn) {
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

    updated
}

fn next_lifecycle_status(
    current: &str,
    age_secs: u64,
    standby_secs: u64,
    archive_secs: u64,
) -> Option<&'static str> {
    if age_secs < standby_secs {
        if matches!(current, "beklemede" | "archived") {
            Some("active")
        } else {
            None
        }
    } else if age_secs < archive_secs {
        if current != "beklemede" {
            Some("beklemede")
        } else {
            None
        }
    } else if current != "archived" {
        Some("archived")
    } else {
        None
    }
}

fn log_transition_as_activity(
    conn: &rusqlite::Connection,
    project_name: &str,
    old_status: &str,
    new_status: &str,
) {
    let summary = format!("{project_name}: {old_status} → {new_status}");
    if let Err(e) = raios_core::db::log_activity_event(
        conn,
        "lifecycle",
        Some(project_name),
        "important",
        &summary,
        None,
    ) {
        eprintln!("[Lifecycle] Failed to log activity event for {project_name}: {e}");
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

fn prune_stale_activity_events(conn: &rusqlite::Connection) {
    match raios_core::db::prune_activity_events_older_than(conn, ACTIVITY_EVENT_RETENTION_DAYS) {
        Ok(0) => {}
        Ok(n) => println!("[Lifecycle] Pruned {n} stale activity_events row(s)."),
        Err(e) => eprintln!("[Lifecycle] activity_events prune failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn recent_activity_reactivates_only_inactive_automatic_statuses() {
        assert_eq!(
            next_lifecycle_status("active", DAY, 14 * DAY, 90 * DAY),
            None
        );
        assert_eq!(
            next_lifecycle_status("beklemede", DAY, 14 * DAY, 90 * DAY),
            Some("active")
        );
        assert_eq!(
            next_lifecycle_status("archived", DAY, 14 * DAY, 90 * DAY),
            Some("active")
        );
    }

    #[test]
    fn stale_activity_moves_to_beklemede_once() {
        assert_eq!(
            next_lifecycle_status("active", 20 * DAY, 14 * DAY, 90 * DAY),
            Some("beklemede")
        );
        assert_eq!(
            next_lifecycle_status("beklemede", 20 * DAY, 14 * DAY, 90 * DAY),
            None
        );
    }

    #[test]
    fn archive_threshold_moves_to_archived_once() {
        assert_eq!(
            next_lifecycle_status("active", 100 * DAY, 14 * DAY, 90 * DAY),
            Some("archived")
        );
        assert_eq!(
            next_lifecycle_status("archived", 100 * DAY, 14 * DAY, 90 * DAY),
            None
        );
    }

    #[test]
    fn exact_thresholds_use_the_stale_branch() {
        assert_eq!(
            next_lifecycle_status("active", 14 * DAY, 14 * DAY, 90 * DAY),
            Some("beklemede")
        );
        assert_eq!(
            next_lifecycle_status("active", 90 * DAY, 14 * DAY, 90 * DAY),
            Some("archived")
        );
    }

    #[test]
    fn log_transition_as_activity_writes_important_row() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        log_transition_as_activity(&conn, "demo-project", "active", "beklemede");

        let (tier, project, summary): (String, Option<String>, String) = conn
            .query_row(
                "SELECT tier, project, summary FROM activity_events WHERE source = 'lifecycle'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(tier, "important");
        assert_eq!(project.as_deref(), Some("demo-project"));
        assert!(summary.contains("active"));
        assert!(summary.contains("beklemede"));
    }

    #[test]
    fn lifecycle_tick_prunes_activity_events_older_than_30_days() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // Insert a stale row from 2020
        conn.execute(
            "INSERT INTO activity_events (ts, source, project, tier, summary)
             VALUES ('2020-01-01T00:00:00Z', 'git', 'old', 'routine', 'stale')",
            [],
        )
        .unwrap();

        prune_stale_activity_events(&conn);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ─── run_lifecycle_pass ──────────────────────────────────────────────

    use raios_core::entities::EntityProject;
    use std::path::{Path, PathBuf};

    const NOW: u64 = 1_800_000_000;

    struct Fixture {
        _dir: tempfile::TempDir,
        conn: rusqlite::Connection,
        state: Arc<RwLock<DaemonState>>,
        tx: broadcast::Sender<String>,
        rx: broadcast::Receiver<String>,
        root: PathBuf,
    }

    impl Fixture {
        /// `projects`: (name, status, has_git).
        async fn new(projects: &[(&str, &str, bool)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            raios_core::db::migrate_existing(&conn).unwrap();
            let mut entities = Vec::new();
            for (name, status, has_git) in projects {
                let path = root.join(name);
                std::fs::create_dir_all(&path).unwrap();
                if *has_git {
                    std::fs::create_dir_all(path.join(".git")).unwrap();
                }
                raios_core::db::upsert_project(
                    &conn,
                    name,
                    "test",
                    &path.to_string_lossy(),
                    None,
                    status,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                // upsert keeps some statuses; set the one under test explicitly.
                raios_core::db::update_project_status(&conn, &path.to_string_lossy(), status)
                    .unwrap();
                entities.push(EntityProject {
                    name: name.to_string(),
                    category: "test".into(),
                    local_path: path,
                    github: None,
                    status: status.to_string(),
                    stars: None,
                    last_commit: None,
                    version: None,
                    version_nickname: None,
                });
            }
            let state = DaemonState::new();
            state.write().await.projects = entities;
            let (tx, rx) = broadcast::channel(8);
            Fixture {
                _dir: dir,
                conn,
                state,
                tx,
                rx,
                root,
            }
        }

        async fn pass(&mut self, last_commit: &(dyn Fn(&Path) -> Option<u64> + Sync)) -> bool {
            run_lifecycle_pass(
                &self.state,
                &self.tx,
                &mut self.conn,
                NOW,
                14 * DAY,
                90 * DAY,
                last_commit,
            )
            .await
        }

        fn db_status(&self, name: &str) -> String {
            let path = self.root.join(name).to_string_lossy().to_string();
            self.conn
                .query_row("SELECT status FROM projects WHERE path = ?1", [path], |r| {
                    r.get(0)
                })
                .unwrap()
        }
    }

    fn committed_days_ago(days: u64) -> impl Fn(&Path) -> Option<u64> + Sync {
        move |_| Some(NOW - days * DAY)
    }

    #[tokio::test]
    async fn a_pass_moves_a_quiet_project_to_standby_and_broadcasts_fresh_state() {
        let mut fx = Fixture::new(&[("quiet", "active", true)]).await;

        assert!(fx.pass(&committed_days_ago(20)).await);

        assert_eq!(fx.db_status("quiet"), "beklemede");
        assert_eq!(fx.state.read().await.projects[0].status, "beklemede");
        let msg = fx.rx.try_recv().expect("state was not broadcast");
        assert!(msg.contains("StateSync") && msg.contains("beklemede"));
        let logged: i64 = fx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE source = 'lifecycle'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1, "the transition must be recorded as activity");
    }

    #[tokio::test]
    async fn a_pass_archives_a_long_dormant_project_and_reactivates_one_with_new_commits() {
        let mut fx = Fixture::new(&[("old", "beklemede", true), ("back", "archived", true)]).await;
        let by_name = |p: &Path| {
            Some(if p.ends_with("old") {
                NOW - 120 * DAY
            } else {
                NOW - DAY
            })
        };

        assert!(fx.pass(&by_name).await);

        assert_eq!(fx.db_status("old"), "archived");
        assert_eq!(fx.db_status("back"), "active");
    }

    #[tokio::test]
    async fn a_pass_never_touches_pinned_statuses_however_old_the_commit() {
        let mut fx = Fixture::new(&[
            ("prod", "production", true),
            ("early", "early", true),
            ("legacy", "legacy", true),
            ("waiting", "waiting", true),
        ])
        .await;

        assert!(!fx.pass(&committed_days_ago(400)).await);

        for name in ["prod", "early", "legacy", "waiting"] {
            assert_ne!(fx.db_status(name), "archived", "{name} was moved");
        }
        assert_eq!(fx.db_status("prod"), "production");
        assert!(
            fx.rx.try_recv().is_err(),
            "nothing changed, nothing to send"
        );
    }

    #[tokio::test]
    async fn a_pass_skips_projects_without_git_or_without_a_readable_commit() {
        let mut fx =
            Fixture::new(&[("nogit", "active", false), ("nocommit", "active", true)]).await;
        let no_commit = |_: &Path| None;

        // `nogit` is skipped by the .git check even though its commit looks ancient; `nocommit`
        // is not skipped by it, so with an ancient commit it *would* move, hence the order below.
        let nogit_only = |p: &Path| p.ends_with("nogit").then_some(NOW - 400 * DAY);
        assert!(!fx.pass(&nogit_only).await);
        assert_eq!(fx.db_status("nogit"), "active");

        assert!(!fx.pass(&no_commit).await);
        assert_eq!(fx.db_status("nocommit"), "active");
    }

    #[tokio::test]
    async fn a_pass_prunes_activity_rows_past_retention() {
        let mut fx = Fixture::new(&[]).await;
        fx.conn
            .execute(
                "INSERT INTO activity_events (ts, source, project, tier, summary)
                 VALUES ('2020-01-01T00:00:00Z', 'test', NULL, 'routine', 'ancient')",
                [],
            )
            .unwrap();

        fx.pass(&committed_days_ago(0)).await;

        let left: i64 = fx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE summary = 'ancient'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }
}
