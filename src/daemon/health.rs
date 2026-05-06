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
        
        let mut reports = Vec::new();
        for proj in projects {
            // Perform check
            let report = check_project(&proj);
            reports.push(report);
        }

        // 2. Update state with new reports
        {
            let mut s = state.write().await;
            s.health_reports = reports.clone();
            println!("[Daemon] Health scan complete. Reports updated.");
            
            // 3. Broadcast new state
            let msg = serde_json::json!({
                "event": "StateSync",
                "projects": s.projects,
                "health_reports": s.health_reports,
                "active_agents": s.active_agents,
                "index_ready": s.index.is_some(),
                "handover_count": s.handover_count
            });
            let _ = tx.send(msg.to_string());
        }

        // Wait before next scan (e.g., 5 minutes)
        sleep(Duration::from_secs(300)).await;
    }
}
