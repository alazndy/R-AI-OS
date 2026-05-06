use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{sleep, Duration};
use std::process::Command;
use crate::daemon::state::DaemonState;

/// Background worker that periodically updates Git status for all projects.
pub async fn start_git_worker(state: Arc<RwLock<DaemonState>>, tx: broadcast::Sender<String>) {
    println!("[Daemon] Git Worker started.");
    
    loop {
        let projects = {
            let s = state.read().await;
            s.projects.clone()
        };

        if projects.is_empty() {
            sleep(Duration::from_secs(5)).await;
            continue;
        }

        println!("[Daemon] Scanning Git status for {} projects...", projects.len());
        
        let mut updated = false;
        {
            let mut s = state.write().await;
            for proj in s.projects.iter_mut() {
                if proj.local_path.join(".git").exists() {
                    // 1. Get branch name
                    let branch = Command::new("git")
                        .current_dir(&proj.local_path)
                        .args(&["rev-parse", "--abbrev-ref", "HEAD"])
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    // 2. Get dirty status
                    let dirty = Command::new("git")
                        .current_dir(&proj.local_path)
                        .args(&["status", "--porcelain"])
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
                        proj.status = new_status;
                        updated = true;
                    }

                    // 3. GitHub Sync (if github URL exists)
                    if let Some(ref gh_url) = proj.github {
                        if gh_url.contains("github.com") {
                            // Try to get stars and last update via gh api
                            let repo = gh_url.trim_end_matches(".git")
                                .split("github.com/").last()
                                .unwrap_or("");
                            
                            if !repo.is_empty() {
                                let output = Command::new("gh")
                                    .args(&["api", &format!("repos/{}", repo), "--template", "{{.stargazers_count}}|{{.updated_at}}"])
                                    .output();
                                
                                if let Ok(o) = output {
                                    let res = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                    let parts: Vec<&str> = res.split('|').collect();
                                    if parts.len() == 2 {
                                        let stars = parts[0].parse::<u32>().ok();
                                        let last = parts[1].to_string();
                                        
                                        if proj.stars != stars || proj.last_commit.as_ref() != Some(&last) {
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
        }

        // Wait before next scan (e.g., 2 minutes)
        sleep(Duration::from_secs(120)).await;
    }
}
