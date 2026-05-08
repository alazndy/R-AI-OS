/// Embedded background workers for standalone raios (no aiosd required).
/// These activate automatically when aiosd is not reachable on port 42069.
/// Workers send BgMsg directly via std::sync::mpsc — no TCP, no JSON.
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::path::PathBuf;
use notify::{Watcher, RecursiveMode, Config as NotifyConfig};

use crate::app::state::BgMsg;

const HEALTH_INTERVAL:     Duration = Duration::from_secs(120);
const GIT_INTERVAL:        Duration = Duration::from_secs(180);
const DISCOVERY_INTERVAL:  Duration = Duration::from_secs(300);
const SENTINEL_DEBOUNCE:   Duration = Duration::from_millis(600);

// ─── Entry point called from App::new() ──────────────────────────────────────

pub fn spawn_embedded_workers(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    // Only spawn if aiosd is NOT already running
    if std::net::TcpStream::connect_timeout(
        &"127.0.0.1:42069".parse().unwrap(),
        Duration::from_millis(200),
    ).is_ok() {
        return; // aiosd is up, it will handle workers
    }

    spawn_discovery_worker(tx.clone(), dev_ops.clone());
    spawn_health_worker(tx.clone(), dev_ops.clone());
    spawn_git_worker(tx.clone(), dev_ops.clone());
    spawn_file_watcher(tx.clone(), dev_ops.clone());
    spawn_sentinel_worker(tx, dev_ops);
}

// ─── Discovery ───────────────────────────────────────────────────────────────

fn spawn_discovery_worker(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    thread::spawn(move || {
        loop {
            let projects = crate::entities::discover_entities(&dev_ops);
            tx.send(BgMsg::Projects(projects)).ok();
            thread::sleep(DISCOVERY_INTERVAL);
        }
    });
}

// ─── Health ──────────────────────────────────────────────────────────────────

fn spawn_health_worker(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    thread::spawn(move || {
        // Initial delay — let discovery run first
        thread::sleep(Duration::from_secs(5));
        loop {
            let projects = crate::entities::load_entities(&dev_ops);
            if !projects.is_empty() {
                let reports: Vec<_> = projects.iter()
                    .map(|p| crate::health::check_project(p))
                    .collect();
                tx.send(BgMsg::HealthReport(reports)).ok();
            }
            thread::sleep(HEALTH_INTERVAL);
        }
    });
}

// ─── Git status ──────────────────────────────────────────────────────────────

fn spawn_git_worker(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(10));
        loop {
            let mut projects = crate::entities::load_entities(&dev_ops);
            let mut changed = false;

            for proj in &mut projects {
                if !proj.local_path.join(".git").exists() { continue; }

                let dirty = crate::filebrowser::git_is_dirty(&proj.local_path);
                let new_status = match dirty {
                    Some(true)  => "dirty".to_string(),
                    Some(false) => "clean".to_string(),
                    None        => proj.status.clone(),
                };
                if proj.status != new_status {
                    proj.status = new_status;
                    changed = true;
                }
            }

            if changed {
                let _ = crate::entities::save_entities(&dev_ops, projects.clone());
                tx.send(BgMsg::Projects(projects)).ok();
            }

            thread::sleep(GIT_INTERVAL);
        }
    });
}

// ─── File watcher ────────────────────────────────────────────────────────────

fn spawn_file_watcher(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    thread::spawn(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();

        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = notify_tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };

        if watcher.watch(&dev_ops, RecursiveMode::Recursive).is_err() {
            return;
        }

        // Keep watcher alive
        let _watcher = watcher;

        for event in notify_rx {
            for path in event.paths {
                tx.send(BgMsg::FileChanged(path)).ok();
            }
        }
    });
}

// ─── Sentinel (event-driven, Faz 3A) ─────────────────────────────────────────

fn spawn_sentinel_worker(tx: Sender<BgMsg>, dev_ops: PathBuf) {
    use std::collections::HashMap;
    use std::time::Instant;

    thread::spawn(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel::<PathBuf>();

        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                use notify::EventKind::*;
                match event.kind {
                    Modify(_) | Create(_) => {
                        for path in event.paths {
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if matches!(ext, "rs" | "ts" | "tsx" | "py" | "js" | "jsx") {
                                let _ = notify_tx.send(path);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };

        if watcher.watch(&dev_ops, RecursiveMode::Recursive).is_err() {
            return;
        }

        let _watcher = watcher;

        // Debounce: path → last event time
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

        loop {
            // Drain incoming events into debounce map
            loop {
                match notify_rx.try_recv() {
                    Ok(path) => { pending.insert(path, Instant::now()); }
                    Err(_) => break,
                }
            }

            // Find paths whose debounce window has elapsed
            let now = Instant::now();
            let ready: Vec<PathBuf> = pending
                .iter()
                .filter(|(_, t)| now.duration_since(**t) >= SENTINEL_DEBOUNCE)
                .map(|(p, _)| p.clone())
                .collect();

            for path in &ready {
                pending.remove(path);

                // Find which project this file belongs to
                let project_path = find_project_root(path, &dev_ops);
                if let Some(proj_path) = project_path {
                    run_sentinel_check(&proj_path, &tx);
                }
            }

            thread::sleep(Duration::from_millis(100));
        }
    });
}

fn find_project_root(file_path: &std::path::Path, dev_ops: &std::path::Path) -> Option<PathBuf> {
    let mut current = file_path.parent()?;
    loop {
        if current.join(".raios.yaml").exists()
            || current.join("Cargo.toml").exists()
            || current.join("package.json").exists()
            || current.join(".git").exists()
        {
            return Some(current.to_path_buf());
        }
        if current == dev_ops || current.parent().is_none() {
            return None;
        }
        current = current.parent()?;
    }
}

fn run_sentinel_check(proj_path: &std::path::Path, tx: &Sender<BgMsg>) {
    use crate::sentinel::SentinelState;
    use crate::daemon::state::SentinelFileStatus;

    if proj_path.join("Cargo.toml").exists() {
        match crate::sentinel::compiler::run_cargo_check(proj_path) {
            Ok(errors) => {
                let proj_str = proj_path.to_string_lossy().to_string();
                let proj_name = proj_path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let status = if errors.is_empty() {
                    SentinelFileStatus { path: proj_str, state: SentinelState::Compiled, errors: vec![] }
                } else {
                    let first_err = errors.into_iter().next().unwrap();
                    SentinelFileStatus { path: proj_str, state: SentinelState::Failed, errors: vec![first_err] }
                };

                tx.send(BgMsg::SentinelUpdate {
                    project: proj_name,
                    status: format!("{:?}", status.state),
                    error_count: status.errors.len(),
                }).ok();
            }
            Err(_) => {}
        }
    }
}
