use crate::daemon::state::{DaemonState, SentinelFileStatus};
use crate::sentinel::compiler::run_cargo_check;
use crate::sentinel::SentinelState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

pub async fn start_sentinel_worker(state: Arc<RwLock<DaemonState>>, tx: broadcast::Sender<String>) {
    println!("[Sentinel] Worker started.");

    // Simple loop to check for dirty files and run compiler
    loop {
        let dirty_projects = {
            let s = state.read().await;
            // For now, let's assume we monitor the current active projects
            s.projects
                .iter()
                .filter(|p| p.status == "active")
                .map(|p| (p.name.clone(), p.local_path.clone()))
                .collect::<Vec<_>>()
        };

        for (name, proj_path) in dirty_projects {
            if !proj_path.exists() {
                continue;
            }
            // Only scan Rust projects (Cargo.toml present)
            if !proj_path.join("Cargo.toml").exists() {
                continue;
            }

            let proj_str = proj_path.to_string_lossy().to_string();

            match run_cargo_check(&proj_path) {
                Ok(errors) => {
                    let mut s = state.write().await;

                    s.sentinel_files.retain(|f| !f.path.starts_with(&proj_str));

                    if errors.is_empty() {
                        s.sentinel_files.push(SentinelFileStatus {
                            path: proj_str.clone(),
                            state: SentinelState::Compiled,
                            errors: Vec::new(),
                        });
                    } else {
                        for err in &errors {
                            s.sentinel_files.push(SentinelFileStatus {
                                path: format!("{}/{}", proj_str, err.file),
                                state: SentinelState::Failed,
                                errors: vec![err.clone()],
                            });
                        }
                    }

                    let sync_msg = serde_json::json!({
                        "event": "SentinelUpdate",
                        "project": name,
                        "status": if errors.is_empty() { "Compiled" } else { "Failed" },
                        "error_count": errors.len()
                    });
                    let _ = tx.send(sync_msg.to_string());
                }
                Err(e) => {
                    eprintln!("[Sentinel] cargo check failed for {}: {}", name, e);
                }
            }
        }

        sleep(Duration::from_secs(30)).await; // Periodik check (MVP)
    }
}
