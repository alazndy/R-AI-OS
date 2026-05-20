use crate::daemon::state::DaemonState;
use crate::health::check_project;
use crate::radar::{RadarChannel, Whisper};
use crate::security::{scan_project, SecurityReport, Severity as SecSev};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{sleep, Duration};

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
pub async fn start_health_worker(state: Arc<RwLock<DaemonState>>, tx: broadcast::Sender<String>) {
    println!("[Daemon] Health Worker started.");

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

        println!(
            "[Daemon] Scanning health for {} projects...",
            projects.len()
        );

        let tx_clone = tx.clone();
        let state_clone = state.clone();

        // Use a task set or similar to run in parallel
        let mut handles = vec![];
        for proj in projects.clone() {
            let tx_log = tx.clone();
            let radar_clone = RadarChannel::new(tx.clone());
            let proj_name_clone = proj.name.clone();
            let proj_path_clone = proj.local_path.clone();
            handles.push(tokio::task::spawn_blocking(move || {
                let report = check_project(&proj);
                let sec_report = scan_project(&proj_path_clone);
                emit_security_whispers(&proj_name_clone, &sec_report, &radar_clone);
                let log_msg = serde_json::json!({
                    "event": "NewLog",
                    "log": {
                        "timestamp": chrono::Local::now().format("%H:%M:%S").to_string(),
                        "sender": "HealthWorker",
                        "content": format!("Checked: {}", proj.name)
                    }
                });
                let _ = tx_log.send(log_msg.to_string());
                report
            }));
        }

        let mut reports = Vec::new();
        for handle in handles {
            if let Ok(report) = handle.await {
                reports.push(report);
            }
        }

        // 2. Update state with new reports
        {
            let mut s = state_clone.write().await;
            s.health_reports = reports.clone();
            println!("[Daemon] Health scan complete. Reports updated.");

            // 3. Broadcast new state
            let msg = serde_json::json!({
                "event": "StateSync",
                "projects": s.projects,
                "health_reports": s.health_reports,
                "active_agents": s.active_agents,
                "index_ready": s.index.is_some(),
                "handover_count": s.handover_count,
                "pending_file_changes": s.pending_file_changes
            });
            let _ = tx_clone.send(msg.to_string());
        }

        // Wait before next scan (e.g., 5 minutes)
        sleep(Duration::from_secs(300)).await;
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
