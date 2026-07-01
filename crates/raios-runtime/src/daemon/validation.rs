use crate::daemon::state::DaemonState;
use crate::health::validate_file;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

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

                    let matching_proj = projects.iter().find(|p| path.starts_with(&p.local_path));

                    if let Some(proj) = matching_proj {
                        println!(
                            "[Validation Worker] Validating {} in project {}...",
                            path.display(),
                            proj.name
                        );

                        let proj_clone = proj.clone();
                        let path_clone = path.to_path_buf();

                        // Run validation in a blocking thread
                        let errors = tokio::task::spawn_blocking(move || {
                            validate_file(&path_clone, &proj_clone)
                        })
                        .await
                        .unwrap_or_default();

                        // 2. Update state
                        {
                            let mut s = state.write().await;
                            // Update latest errors
                            // For now, let's keep only errors for the current project or a limit
                            s.latest_errors = errors.clone();

                            // 3. Emit Radar whispers for compliance violations
                            let radar = crate::radar::RadarChannel::new(tx_broadcast.clone());
                            emit_compliance_whispers(&proj.name, &errors, &radar);

                            // 4. Broadcast Feedback
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

/// Emit Radar whispers for compliance violations found during validation.
///
/// Each validation error is converted to an architectural violation whisper
/// and emitted to connected agents via the Radar channel.
pub(crate) fn emit_compliance_whispers(
    project_name: &str,
    errors: &[crate::daemon::state::ValidationError],
    radar: &crate::radar::RadarChannel,
) {
    let whispers: Vec<crate::radar::Whisper> = errors
        .iter()
        .map(|e| {
            crate::radar::Whisper::arch_violation(
                project_name,
                std::path::PathBuf::from(&e.file),
                &e.message,
                Some("fix compliance violation before merging"),
            )
        })
        .collect();
    radar.emit_many(whispers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::state::ValidationError;
    use crate::radar::RadarChannel;

    #[test]
    fn compliance_violations_produce_arch_whispers() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);

        let errors = vec![ValidationError {
            file: "src/app.ts".to_string(),
            message: "console.log found in production code".to_string(),
            line: Some(42),
            source: "compliance".to_string(),
        }];

        emit_compliance_whispers("my-proj", &errors, &radar);

        let msg = rx.try_recv().unwrap();
        let val: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(val["event"], "RadarWhisper");
        assert_eq!(val["kind"], "arch_violation");
    }

    #[test]
    fn empty_errors_emit_no_whispers() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);
        emit_compliance_whispers("my-proj", &[], &radar);
        assert!(rx.try_recv().is_err());
    }
}
