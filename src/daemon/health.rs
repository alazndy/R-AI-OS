use crate::daemon::state::DaemonState;
use crate::health::check_project_fast;
use crate::radar::{RadarChannel, Whisper};
use crate::security::{scan_project_fast, SecurityReport, Severity as SecSev};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio::time::{sleep, Duration};

/// Max concurrent spawn_blocking health scans.
/// Prevents parallel `cargo audit` / `pnpm audit` processes from spiking RAM.
const MAX_CONCURRENT_SCANS: usize = 3;

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

/// Background worker that periodically updates project health reports.
pub async fn start_health_worker(
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    interval: Duration,
) {
    println!("[Daemon] Health Worker started.");

    // Delay first scan so other workers can settle without CPU contention
    sleep(Duration::from_secs(30)).await;

    loop {
        // 1. Get projects from state
        let projects = {
            let s = state.read().await;
            s.projects.clone()
        };

        if projects.is_empty() {
            // Wait for projects to be discovered
            sleep(Duration::from_secs(1)).await;
            continue;
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

        let tx_clone = tx.clone();
        let state_clone = state.clone();
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_SCANS));

        let mut handles = vec![];
        for proj in active {
            let proj = proj.clone();
            let tx_log = tx.clone();
            let radar_clone = RadarChannel::new(tx.clone());
            let proj_name_clone = proj.name.clone();
            let proj_path_clone = proj.local_path.clone();
            let sem_clone = sem.clone();
            handles.push(tokio::spawn(async move {
                // Acquire permit — blocks until a slot is free
                let _permit = sem_clone.acquire().await;
                tokio::task::spawn_blocking(move || {
                    let report = check_project_fast(&proj);
                    let sec_report = scan_project_fast(&proj_path_clone);
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
                    report
                })
                .await
                .ok()
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
            let mut s = state_clone.write().await;
            s.health_reports = reports.clone();
            println!("[Daemon] Health scan complete. Reports updated.");
        }

        // 3. Broadcast delta — not full StateSync
        let delta = serde_json::json!({
            "event": "HealthDelta",
            "report": reports,
        });
        let _ = tx_clone.send(delta.to_string());

        // Wait before next scan (e.g., 5 minutes)
        sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::RadarChannel;
    use crate::security::{ProjectType, SecurityIssue, SecurityReport, Severity as SecSev};

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
}
