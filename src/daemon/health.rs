use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{sleep, Duration};
use crate::daemon::state::DaemonState;
use crate::health::check_project;

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

        println!("[Daemon] Scanning health for {} projects...", projects.len());
        
        let tx_clone = tx.clone();
        let state_clone = state.clone();
        
        // Use a task set or similar to run in parallel
        let mut handles = vec![];
        for proj in projects.clone() {
            let tx_log = tx.clone();
            handles.push(tokio::task::spawn_blocking(move || {
                let report = check_project(&proj);
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
