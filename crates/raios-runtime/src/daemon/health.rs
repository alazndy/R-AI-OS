use crate::daemon::state::DaemonState;
use crate::health::check_project_fast;
use crate::radar::{RadarChannel, Whisper};
use raios_core::security::{scan_project_fast, SecurityReport, Severity as SecSev};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio::time::{sleep, Duration};

/// Max concurrent spawn_blocking health scans.
/// Prevents parallel `cargo audit` / `pnpm audit` processes from spiking RAM.
const MAX_CONCURRENT_SCANS: usize = 3;

/// SQLite permits only one writer at a time. Keep expensive project scans
/// parallel, but serialize their short activity-state transactions.
const MAX_CONCURRENT_DB_SYNCS: usize = 1;

/// Emit Radar whispers for Critical/High security issues found in a report.
pub(crate) fn emit_security_whispers(
    project_name: &str,
    report: &SecurityReport,
    radar: &RadarChannel,
) {
    let whispers: Vec<Whisper> = report
        .issues
        .iter()
        .filter(|i| matches!(i.severity, SecSev::Critical | SecSev::High))
        .map(|issue| {
            let msg = format!(
                "[{}] {} (OWASP {})",
                issue.severity.label(),
                issue.title,
                issue.owasp
            );
            Whisper::security_vuln(project_name, issue.file.clone(), &msg, None)
        })
        .collect();
    radar.emit_many(whispers);
}

/// Synchronize one project's currently active Critical/High findings and
/// persist important activity only for findings that are new or reappeared
/// after being resolved. This is independent of the ephemeral Radar-whisper
/// broadcast `emit_security_whispers` already provides to connected agents.
pub(crate) fn sync_security_findings_as_activity(
    conn: &mut rusqlite::Connection,
    project_name: &str,
    report: &SecurityReport,
) {
    let findings: Vec<(String, String)> = report
        .issues
        .iter()
        .filter(|i| matches!(i.severity, SecSev::Critical | SecSev::High))
        .map(|issue| {
            let file = issue
                .file
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned());
            let fingerprint = serde_json::to_string(&(
                issue.owasp,
                issue.title,
                issue.severity.label(),
                file,
                issue.line,
            ))
            .expect("security finding identity contains only serializable values");
            let summary = format!(
                "{} [{}] {}",
                project_name,
                issue.severity.label(),
                issue.title
            );
            (fingerprint, summary)
        })
        .collect();

    if let Err(e) = raios_core::db::sync_security_activity_findings(conn, project_name, &findings) {
        eprintln!("[Daemon] Failed to sync security activity for {project_name}: {e}");
    }
}

/// Background worker that periodically updates project health reports.
pub async fn start_health_worker(
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    interval: Duration,
) {
    println!("[Daemon] Health Worker started.");

    // Delay first scan so other workers can settle without CPU contention
    sleep(Duration::from_secs(30)).await;

    let sec_scan: Arc<SecurityScanFn> = Arc::new(scan_project_fast);
    let db_opener: Arc<DbOpener> = Arc::new(production_db_opener);

    loop {
        if !run_health_pass(&state, &tx, &sec_scan, &db_opener).await {
            // Wait for projects to be discovered
            sleep(Duration::from_secs(1)).await;
            continue;
        }

        // Wait before next scan (e.g., 5 minutes)
        sleep(interval).await;
    }
}

type SecurityScanFn = dyn Fn(&std::path::Path) -> SecurityReport + Send + Sync;
type DbOpener = dyn Fn() -> Result<rusqlite::Connection, String> + Send + Sync;

fn production_db_opener() -> Result<rusqlite::Connection, String> {
    raios_core::db::open_db().map_err(|e| e.to_string())
}

/// One health pass: scan every active project (at most `MAX_CONCURRENT_SCANS` at once, DB writes
/// one at a time), store the reports in the daemon state and broadcast a `HealthDelta`.
/// Inactive projects (`beklemede`, `archived`) are skipped. Returns `false`, doing nothing, when
/// no projects are known yet.
async fn run_health_pass(
    state: &Arc<RwLock<DaemonState>>,
    tx: &broadcast::Sender<String>,
    sec_scan: &Arc<SecurityScanFn>,
    open_db: &Arc<DbOpener>,
) -> bool {
    // 1. Get projects from state
    let projects = {
        let s = state.read().await;
        s.projects.clone()
    };

    if projects.is_empty() {
        return false;
    }

    // Skip inactive projects from the expensive dependency audit.
    let (active, inactive): (Vec<_>, Vec<_>) = projects
        .iter()
        .partition(|p| !matches!(p.status.as_str(), "beklemede" | "archived"));

    println!(
        "[Daemon] Scanning health for {} active projects ({} inactive skipped)...",
        active.len(),
        inactive.len(),
    );

    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_SCANS));
    let db_sem = Arc::new(Semaphore::new(MAX_CONCURRENT_DB_SYNCS));

    let mut handles = vec![];
    for proj in active {
        let proj = proj.clone();
        let tx_log = tx.clone();
        let radar_clone = RadarChannel::new(tx.clone());
        let proj_name_clone = proj.name.clone();
        let proj_path_clone = proj.local_path.clone();
        let sem_clone = sem.clone();
        let db_sem_clone = db_sem.clone();
        let sec_scan = Arc::clone(sec_scan);
        let open_db = Arc::clone(open_db);
        handles.push(tokio::spawn(async move {
            // Acquire permit — blocks until a slot is free
            let scan_permit = sem_clone.acquire().await.ok()?;
            let scan_result = tokio::task::spawn_blocking(move || {
                let report = check_project_fast(&proj);
                let sec_report = sec_scan(&proj_path_clone);
                emit_security_whispers(&proj_name_clone, &sec_report, &radar_clone);
                let log_msg = serde_json::json!({
                    "event": "NewLog",
                    "log": {
                        "timestamp": chrono::Local::now().format("%H:%M:%S").to_string(),
                        "sender": "HealthWorker",
                        "content": format!("Checked: {}", proj_name_clone)
                    }
                });
                let _ = tx_log.send(log_msg.to_string());
                (report, proj_name_clone, sec_report)
            })
            .await
            .ok()?;
            drop(scan_permit);

            let db_permit = db_sem_clone.acquire().await.ok()?;
            let report = tokio::task::spawn_blocking(move || {
                let (report, project_name, sec_report) = scan_result;
                match open_db() {
                    Ok(mut conn) => {
                        sync_security_findings_as_activity(&mut conn, &project_name, &sec_report)
                    }
                    Err(e) => eprintln!(
                        "[Daemon] DB open failed for security activity logging ({project_name}): {e}"
                    ),
                }
                report
            })
            .await
            .ok()?;
            drop(db_permit);
            Some(report)
        }));
    }

    let mut reports = Vec::new();
    for handle in handles {
        if let Ok(Some(report)) = handle.await {
            reports.push(report);
        }
    }

    // 2. Update state
    {
        let mut s = state.write().await;
        s.health_reports = reports.clone();
        println!("[Daemon] Health scan complete. Reports updated.");
    }

    // 3. Broadcast delta — not full StateSync
    let delta = serde_json::json!({
        "event": "HealthDelta",
        "report": reports,
    });
    let _ = tx.send(delta.to_string());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::RadarChannel;
    use raios_core::security::{ProjectType, SecurityIssue, SecurityReport, Severity as SecSev};

    #[test]
    fn critical_issues_emit_security_whispers() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);

        let report = SecurityReport {
            score: 40,
            grade: "F",
            issues: vec![SecurityIssue {
                owasp: "A02",
                title: "Hardcoded password",
                severity: SecSev::Critical,
                file: Some(std::path::PathBuf::from("src/config.rs")),
                line: Some(12),
                snippet: None,
            }],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        };

        emit_security_whispers("my-proj", &report, &radar);

        let msg = rx.try_recv().unwrap();
        let val: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(val["event"], "RadarWhisper");
        assert_eq!(val["kind"], "security_vuln");
        assert_eq!(val["project"], "my-proj");
    }

    #[test]
    fn low_severity_issues_are_not_emitted() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);

        let report = SecurityReport {
            score: 90,
            grade: "A",
            issues: vec![SecurityIssue {
                owasp: "A09",
                title: "Missing rate limit header",
                severity: SecSev::Low,
                file: None,
                line: None,
                snippet: None,
            }],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        };

        emit_security_whispers("my-proj", &report, &radar);

        // No whisper for Low severity
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sync_security_findings_as_activity_writes_one_event_for_repeated_critical() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let report = SecurityReport {
            score: 40,
            grade: "F",
            issues: vec![SecurityIssue {
                owasp: "A02",
                title: "hardcoded secret",
                severity: SecSev::Critical,
                file: Some(std::path::PathBuf::from("src/main.rs")),
                line: None,
                snippet: None,
            }],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        };

        sync_security_findings_as_activity(&mut conn, "demo-project", &report);
        sync_security_findings_as_activity(&mut conn, "demo-project", &report);

        let (count, tier, project): (i64, String, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MIN(tier), MIN(project)
                 FROM activity_events WHERE source = 'audit'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(count, 1, "an unchanged finding must not notify twice");
        assert_eq!(tier, "important");
        assert_eq!(project.as_deref(), Some("demo-project"));
    }

    #[test]
    fn sync_security_findings_as_activity_ignores_low_and_info_severity() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let report = SecurityReport {
            score: 90,
            grade: "A",
            issues: vec![SecurityIssue {
                owasp: "A09",
                title: "Missing rate limit header",
                severity: SecSev::Low,
                file: None,
                line: None,
                snippet: None,
            }],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        };

        sync_security_findings_as_activity(&mut conn, "demo-project", &report);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ─── run_health_pass ─────────────────────────────────────────────────

    use raios_core::entities::EntityProject;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn project(root: &std::path::Path, name: &str, status: &str) -> EntityProject {
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        EntityProject {
            name: name.into(),
            category: "test".into(),
            local_path: path,
            github: None,
            status: status.into(),
            stars: None,
            last_commit: None,
            version: None,
            version_nickname: None,
        }
    }

    fn report_with(severity: SecSev) -> SecurityReport {
        SecurityReport {
            score: 50,
            grade: "C",
            issues: vec![SecurityIssue {
                owasp: "A02",
                title: "Hardcoded password",
                severity,
                file: Some(std::path::PathBuf::from("src/config.rs")),
                line: Some(1),
                snippet: None,
            }],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        }
    }

    struct Fx {
        dir: tempfile::TempDir,
        db: std::path::PathBuf,
        state: Arc<RwLock<DaemonState>>,
        tx: broadcast::Sender<String>,
        rx: broadcast::Receiver<String>,
    }

    impl Fx {
        async fn new(projects: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("test.db");
            raios_core::db::migrate_existing(&rusqlite::Connection::open(&db).unwrap()).unwrap();
            let root = dir.path().join("projects");
            let state = DaemonState::new();
            state.write().await.projects = projects
                .iter()
                .map(|(name, status)| project(&root, name, status))
                .collect();
            let (tx, rx) = broadcast::channel(64);
            Fx {
                dir,
                db,
                state,
                tx,
                rx,
            }
        }

        fn opener(&self) -> Arc<DbOpener> {
            let db = self.db.clone();
            Arc::new(move || rusqlite::Connection::open(&db).map_err(|e| e.to_string()))
        }

        async fn pass(&self, scan: Arc<SecurityScanFn>) -> bool {
            run_health_pass(&self.state, &self.tx, &scan, &self.opener()).await
        }

        fn events(&mut self) -> Vec<serde_json::Value> {
            let mut out = Vec::new();
            while let Ok(m) = self.rx.try_recv() {
                out.push(serde_json::from_str(&m).unwrap());
            }
            out
        }
    }

    #[tokio::test]
    async fn a_pass_with_no_known_projects_does_nothing_and_says_so() {
        let mut fx = Fx::new(&[]).await;
        let scans = Arc::new(AtomicUsize::new(0));
        let counter = scans.clone();

        let ran = fx
            .pass(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                report_with(SecSev::Low)
            }))
            .await;

        assert!(!ran);
        assert_eq!(scans.load(Ordering::SeqCst), 0);
        assert!(fx.events().is_empty());
    }

    #[tokio::test]
    async fn a_pass_scans_only_active_projects_and_stores_and_broadcasts_their_reports() {
        let mut fx = Fx::new(&[
            ("live", "active"),
            ("prod", "production"),
            ("sleeping", "beklemede"),
            ("old", "archived"),
        ])
        .await;
        let scanned = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = scanned.clone();

        assert!(
            fx.pass(Arc::new(move |p| {
                seen.lock()
                    .unwrap()
                    .push(p.file_name().unwrap().to_string_lossy().into_owned());
                report_with(SecSev::Low)
            }))
            .await
        );

        let mut scanned = scanned.lock().unwrap().clone();
        scanned.sort();
        assert_eq!(
            scanned,
            vec!["live", "prod"],
            "inactive projects were scanned"
        );
        let stored = fx.state.read().await.health_reports.len();
        assert_eq!(stored, 2);
        let events = fx.events();
        let delta = events
            .iter()
            .find(|e| e["event"] == "HealthDelta")
            .expect("no HealthDelta broadcast");
        assert_eq!(delta["report"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_critical_finding_reaches_radar_and_the_activity_log() {
        let mut fx = Fx::new(&[("live", "active")]).await;

        fx.pass(Arc::new(|_| report_with(SecSev::Critical))).await;

        assert!(
            fx.events().iter().any(|e| e["event"] == "RadarWhisper"),
            "no radar whisper for a critical finding"
        );
        let conn = rusqlite::Connection::open(&fx.db).unwrap();
        let logged: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE project = 'live' AND tier = 'important'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1);
        // A second pass with the same finding must not log it again.
        fx.pass(Arc::new(|_| report_with(SecSev::Critical))).await;
        let again: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activity_events WHERE project = 'live'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(again, 1, "an unchanged finding was logged twice");
        drop(fx.dir);
    }

    #[tokio::test]
    async fn a_database_that_cannot_be_opened_does_not_lose_the_report() {
        let mut fx = Fx::new(&[("live", "active")]).await;
        let failing: Arc<DbOpener> = Arc::new(|| Err("disk gone".into()));

        let scan: Arc<SecurityScanFn> = Arc::new(|_| report_with(SecSev::Critical));
        assert!(run_health_pass(&fx.state, &fx.tx, &scan, &failing).await);

        assert_eq!(fx.state.read().await.health_reports.len(), 1);
        assert!(fx.events().iter().any(|e| e["event"] == "HealthDelta"));
    }
}
