use crate::daemon::state::{DaemonState, SentinelFileStatus};
use crate::radar::{RadarChannel, Whisper};
use crate::sentinel::compiler::run_cargo_check;
use crate::sentinel::SentinelState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

/// At most this many projects are compiled per cycle, newest edits first.
/// Bounds the load when many projects changed at once (or on the first cycle,
/// when none has been checked yet); the rest follow on later cycles.
const MAX_CHECKS_PER_CYCLE: usize = 3;

/// Picks the projects that need a (re)check: Rust projects whose sources
/// changed since their last completed check, most recently edited first,
/// capped at `max`.
fn select_projects_to_check(
    candidates: Vec<(String, PathBuf)>,
    max: usize,
) -> Vec<(String, PathBuf)> {
    let mut stale: Vec<_> = candidates
        .into_iter()
        .filter(|(_, path)| path.join("Cargo.toml").exists())
        .filter(|(_, path)| !crate::sentinel::compiler::is_up_to_date(path))
        .map(|(name, path)| {
            let newest = crate::sentinel::compiler::source_fingerprint(&path).map(|f| f.newest);
            (newest, name, path)
        })
        .collect();
    stale.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    stale
        .into_iter()
        .take(max)
        .map(|(_, name, path)| (name, path))
        .collect()
}

pub async fn start_sentinel_worker(
    state: Arc<RwLock<DaemonState>>,
    tx: broadcast::Sender<String>,
    interval: Duration,
) {
    println!("[Sentinel] Worker started.");
    let radar = RadarChannel::new(tx.clone());

    // Delay first cargo check to avoid startup CPU spike
    sleep(Duration::from_secs(120)).await;

    loop {
        let active_projects = {
            let s = state.read().await;
            s.projects
                .iter()
                .filter(|p| p.status == "active")
                .map(|p| (p.name.clone(), p.local_path.clone()))
                .collect::<Vec<_>>()
        };

        // Fingerprinting walks the source trees: keep it off the async threads.
        let to_check = tokio::task::spawn_blocking(move || {
            select_projects_to_check(active_projects, MAX_CHECKS_PER_CYCLE)
        })
        .await
        .unwrap_or_default();

        for (name, proj_path) in to_check {
            let proj_str = proj_path.to_string_lossy().to_string();

            // `cargo check` blocks for seconds to minutes: never on a runtime thread.
            let check_path = proj_path.clone();
            let result = tokio::task::spawn_blocking(move || run_cargo_check(&check_path)).await;

            match result {
                Ok(Ok(errors)) => {
                    let mut s = state.write().await;
                    s.sentinel_files.retain(|f| !f.path.starts_with(&proj_str));

                    if errors.is_empty() {
                        s.sentinel_files.push(SentinelFileStatus {
                            path: proj_str.clone(),
                            state: SentinelState::Compiled,
                            errors: Vec::new(),
                        });
                    } else {
                        // Emit a structured whisper per compile error
                        let whispers: Vec<Whisper> = errors
                            .iter()
                            .map(|e| {
                                Whisper::compile_error(
                                    &name,
                                    PathBuf::from(&e.file),
                                    e.line.unwrap_or(0) as u32,
                                    &e.message,
                                )
                            })
                            .collect();
                        drop(s); // release write lock before emitting
                        radar.emit_many(whispers);

                        let mut s = state.write().await;
                        for err in &errors {
                            s.sentinel_files.push(SentinelFileStatus {
                                path: format!("{}/{}", proj_str, err.file),
                                state: SentinelState::Failed,
                                errors: vec![err.clone()],
                            });
                        }

                        let sync_msg = serde_json::json!({
                            "event": "SentinelUpdate",
                            "project": name,
                            "status": "Failed",
                            "error_count": errors.len()
                        });
                        let _ = tx.send(sync_msg.to_string());
                        continue;
                    }

                    let sync_msg = serde_json::json!({
                        "event": "SentinelUpdate",
                        "project": name,
                        "status": "Compiled",
                        "error_count": 0
                    });
                    let _ = tx.send(sync_msg.to_string());
                }
                // Could not be checked at all (stale lock file, dependency not
                // available offline, timeout, ...). The result is memoized per
                // source state, so this is logged once per change, not per cycle.
                Ok(Err(e)) => {
                    eprintln!("[Sentinel] cargo check unavailable for {}: {}", name, e);
                }
                Err(e) => {
                    eprintln!("[Sentinel] check task for {} failed: {}", name, e);
                }
            }
        }

        sleep(interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn project(root: &std::path::Path, name: &str, age_secs: u64) -> (String, PathBuf) {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let main = dir.join("src/main.rs");
        fs::write(&main, "fn main() {}").unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        fs::File::options()
            .write(true)
            .open(&main)
            .unwrap()
            .set_modified(when)
            .unwrap();
        // Manifest older than the source so `src/main.rs` decides recency.
        fs::File::options()
            .write(true)
            .open(dir.join("Cargo.toml"))
            .unwrap()
            .set_modified(when - Duration::from_secs(10))
            .unwrap();
        (name.to_string(), dir)
    }

    #[test]
    fn selects_the_most_recently_edited_projects_up_to_the_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let candidates = vec![
            project(tmp.path(), "oldest", 5000),
            project(tmp.path(), "newest", 10),
            project(tmp.path(), "middle", 500),
            project(tmp.path(), "old", 2000),
            project(tmp.path(), "recent", 100),
        ];
        let picked: Vec<String> = select_projects_to_check(candidates, 3)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(picked, ["newest", "recent", "middle"]);
    }

    #[test]
    fn skips_directories_that_are_not_rust_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, rust) = project(tmp.path(), "rusty", 10);
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let picked =
            select_projects_to_check(vec![("plain".into(), plain), ("rusty".into(), rust)], 10);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].0, "rusty");
    }
}
