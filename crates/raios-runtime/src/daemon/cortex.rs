use crate::cortex::store::VectorResult;
use crate::cortex::Cortex;
use raios_core::config::Config;
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
            CortexRequest::Search {
                query,
                top_k,
                scope,
                reply,
            } => {
                // Scanning checks persisted mtimes; only changed files run inference.
                let indexed = match scope.as_deref() {
                    Some(dir) => self.cortex.index_project(dir),
                    None => self.cortex.index_workspace(&workspace_root()),
                };
                if let Err(e) = indexed {
                    eprintln!("[Cortex Worker] On-demand indexing failed: {e:?}");
                }
                if self.tracker.on_search() {
                    self.cortex.rebuild_index();
                }
                let hits = match scope {
                    Some(dir) => self
                        .cortex
                        .search_scoped(&query, top_k, &dir)
                        .unwrap_or_default(),
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

fn workspace_root() -> PathBuf {
    Config::load()
        .unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()))
        .dev_ops_path
}

fn run_cortex_worker(
    eager_indexing: bool,
    mut rx: tokio::sync::mpsc::Receiver<CortexRequest>,
    initialize: impl Fn() -> anyhow::Result<CortexWorkerState>,
) {
    let mut state = None;
    if eager_indexing {
        match initialize() {
            Ok(mut worker) => {
                println!("[Cortex Worker] Starting initial indexing...");
                match worker.cortex.index_workspace(&workspace_root()) {
                    Ok(n) => {
                        println!("[Cortex Worker] Initial indexing complete. {n} files indexed.")
                    }
                    Err(e) => eprintln!("[Cortex Worker] Initial indexing failed: {e:?}"),
                }
                state = Some(worker);
            }
            Err(e) => eprintln!("[Cortex Worker] Failed to initialize Cortex: {e:?}"),
        }
    } else {
        println!("[Cortex Worker] Ready (model and indexing deferred until first request).");
    }

    while let Some(req) = rx.blocking_recv() {
        // Before the first search/reindex, filesystem events must not load the
        // model. The first on-demand scan catches these changes from disk.
        if state.is_none() && matches!(req, CortexRequest::IndexFile { .. }) {
            continue;
        }
        if state.is_none() {
            match initialize() {
                Ok(worker) => state = Some(worker),
                Err(e) => {
                    eprintln!("[Cortex Worker] Failed to initialize Cortex: {e:?}");
                    continue;
                }
            }
        }
        if let Some(worker) = state.as_mut() {
            worker.handle(req);
        }
    }
}

pub fn spawn_cortex_worker(eager_indexing: bool) -> tokio::sync::mpsc::Sender<CortexRequest> {
    let (tx, rx) = tokio::sync::mpsc::channel::<CortexRequest>(64);
    std::thread::spawn(move || {
        // Bound Rayon work too (tokenization and HNSW graph construction).
        let pool = match rayon::ThreadPoolBuilder::new().num_threads(2).build() {
            Ok(pool) => pool,
            Err(e) => {
                eprintln!("[Cortex Worker] Failed to create bounded thread pool: {e}");
                return;
            }
        };
        pool.install(|| {
            run_cortex_worker(eager_indexing, rx, || {
                println!("[Cortex Worker] Loading model on demand...");
                Ok(CortexWorkerState {
                    cortex: Cortex::init()?,
                    tracker: DirtyTracker::default(),
                })
            });
        });
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
    fn startup_and_file_events_do_not_load_model() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tx.try_send(super::CortexRequest::IndexFile {
            path: "changed.rs".into(),
        })
        .unwrap();
        drop(tx);
        super::run_cortex_worker(false, rx, || panic!("idle worker loaded model"));
    }

    #[test]
    fn first_search_loads_model_and_init_failure_can_retry() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        for _ in 0..2 {
            let (reply, _result) = tokio::sync::oneshot::channel();
            tx.try_send(super::CortexRequest::Search {
                query: "test".into(),
                top_k: 1,
                scope: None,
                reply,
            })
            .unwrap();
        }
        drop(tx);
        let attempts = std::cell::Cell::new(0);
        super::run_cortex_worker(false, rx, || {
            attempts.set(attempts.get() + 1);
            anyhow::bail!("simulated model unavailable")
        });
        assert_eq!(attempts.get(), 2);
    }

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
