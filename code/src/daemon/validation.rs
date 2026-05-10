use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use crate::daemon::state::DaemonState;
use crate::health::validate_file;
use std::path::Path;

pub async fn start_validation_worker(
    state: Arc<RwLock<DaemonState>>,
    mut tx_rx: broadcast::Receiver<String>,
    tx_broadcast: broadcast::Sender<String>,
) {
    println!("[Validation Worker] Started.");

    while let Ok(msg) = tx_rx.recv().await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
            if v["event"] == "FileChanged" {
                if let Some(path_str) = v["path"].as_str() {
                    let path = Path::new(path_str);
                    
                    // 1. Find which project this file belongs to
                    let projects = {
                        let s = state.read().await;
                        s.projects.clone()
                    };

                    let matching_proj = projects.iter().find(|p| {
                        path.starts_with(&p.local_path)
                    });

                    if let Some(proj) = matching_proj {
                        println!("[Validation Worker] Validating {} in project {}...", path.display(), proj.name);
                        
                        let proj_clone = proj.clone();
                        let path_clone = path.to_path_buf();
                        
                        // Run validation in a blocking thread
                        let errors = tokio::task::spawn_blocking(move || {
                            validate_file(&path_clone, &proj_clone)
                        }).await.unwrap_or_default();

                        // 2. Update state
                        {
                            let mut s = state.write().await;
                            // Update latest errors
                            // For now, let's keep only errors for the current project or a limit
                            s.latest_errors = errors.clone();
                            
                            // 3. Broadcast Feedback
                            let feedback_msg = serde_json::json!({
                                "event": "ValidationError",
                                "project": proj.name,
                                "file": path.display().to_string(),
                                "errors": errors
                            });
                            let _ = tx_broadcast.send(feedback_msg.to_string());
                        }
                    }
                }
            }
        }
    }
}
