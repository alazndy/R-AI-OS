use raios_core::config::Config;
use crate::cortex::Cortex;
use crate::cortex::store::VectorResult;
use std::path::PathBuf;

pub enum CortexRequest {
    Search {
        query: String,
        top_k: usize,
        scope: Option<PathBuf>,
        reply: tokio::sync::oneshot::Sender<Vec<VectorResult>>,
    },
    IndexFile {
        path: PathBuf,
    },
    Reindex {
        scope: PathBuf,
        reply: tokio::sync::oneshot::Sender<usize>,
    },
}

#[derive(Debug, Default)]
struct DirtyTracker {
    dirty: bool,
    rebuilds: usize,
}

impl DirtyTracker {
    fn on_index_file(&mut self, is_modified: bool) {
        if is_modified {
            self.dirty = true;
        }
    }

    fn on_search(&mut self) -> bool {
        if self.dirty {
            self.dirty = false;
            self.rebuilds += 1;
            true
        } else {
            false
        }
    }

    fn on_reindex(&mut self) {
        self.dirty = false;
    }
}

struct CortexWorkerState {
    cortex: Cortex,
    tracker: DirtyTracker,
}

impl CortexWorkerState {
    fn handle(&mut self, req: CortexRequest) {
        match req {
            CortexRequest::Search { query, top_k, scope, reply } => {
                if self.tracker.on_search() {
                    self.cortex.rebuild_index();
                }
                let hits = match scope {
                    Some(dir) => self.cortex.search_scoped(&query, top_k, &dir).unwrap_or_default(),
                    None => self.cortex.search(&query, top_k).unwrap_or_default(),
                };
                let _ = reply.send(hits);
            }
            CortexRequest::IndexFile { path } => {
                if let Ok(is_modified) = self.cortex.index_file(&path) {
                    self.tracker.on_index_file(is_modified);
                }
            }
            CortexRequest::Reindex { scope, reply } => {
                let n = self.cortex.index_project(&scope).unwrap_or(0);
                self.tracker.on_reindex();
                let _ = reply.send(n);
            }
        }
    }
}

pub fn spawn_cortex_worker(eager_indexing: bool) -> tokio::sync::mpsc::Sender<CortexRequest> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<CortexRequest>(64);

    std::thread::spawn(move || {
        println!("[Cortex Worker] Initializing resident thread...");
        let cortex = match Cortex::init() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Cortex Worker] Failed to initialize Cortex: {e:?}");
                return;
            }
        };

        let mut state = CortexWorkerState {
            cortex,
            tracker: DirtyTracker::default(),
        };

        // Initial full index on startup
        if eager_indexing {
            let config = Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));
            println!("[Cortex Worker] Starting initial indexing...");
            let count = state.cortex.index_workspace(&config.dev_ops_path).unwrap_or(0);
            println!(
                "[Cortex Worker] Initial indexing complete. {} files indexed.",
                count
            );
        }

        while let Some(req) = rx.blocking_recv() {
            state.handle(req);
        }
    });

    tx
}

pub fn is_indexable(path: &str) -> bool {
    let p = path.to_lowercase();
    p.ends_with(".rs")
        || p.ends_with(".ts")
        || p.ends_with(".js")
        || p.ends_with(".py")
        || p.ends_with(".md")
        || p.ends_with(".txt")
        || p.ends_with(".json")
        || p.ends_with(".toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_then_search_rebuilds_exactly_once() {
        let mut tracker = DirtyTracker::default();
        tracker.on_index_file(true);
        assert!(tracker.dirty);
        
        let rebuilt1 = tracker.on_search();
        assert!(rebuilt1);
        assert_eq!(tracker.rebuilds, 1);
        assert!(!tracker.dirty);
        
        let rebuilt2 = tracker.on_search();
        assert!(!rebuilt2);
        assert_eq!(tracker.rebuilds, 1);
    }

    #[test]
    fn search_without_changes_never_rebuilds() {
        let mut tracker = DirtyTracker::default();
        let rebuilt = tracker.on_search();
        assert!(!rebuilt);
        assert_eq!(tracker.rebuilds, 0);
    }

    #[test]
    fn reindex_clears_dirty() {
        let mut tracker = DirtyTracker::default();
        tracker.on_index_file(true);
        assert!(tracker.dirty);
        
        tracker.on_reindex();
        assert!(!tracker.dirty);
        
        let rebuilt = tracker.on_search();
        assert!(!rebuilt);
        assert_eq!(tracker.rebuilds, 0);
    }
}
