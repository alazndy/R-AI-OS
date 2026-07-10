use super::state::DaemonState;
use raios_core::config::Config;
use crate::cortex::Cortex;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub async fn start_cortex_worker(
    _state: Arc<RwLock<DaemonState>>,
    eager_indexing: bool,
    mut tx_rx: broadcast::Receiver<String>,
) {
    println!("[Cortex Worker] Initializing...");

    // Initial full index on startup
    if eager_indexing {
        let config =
            Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));
        if let Ok(mut cortex) = Cortex::init() {
            println!("[Cortex Worker] Starting initial indexing...");
            let count = cortex.index_workspace(&config.dev_ops_path).unwrap_or(0);
            println!(
                "[Cortex Worker] Initial indexing complete. {} files indexed.",
                count
            );
        }
    }

    // Listen for file change events from the watcher
    while let Ok(msg) = tx_rx.recv().await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
            if v["event"] == "FileChanged" {
                if let Some(path) = v["path"].as_str() {
                    // Skip if not a code/doc file
                    if !is_indexable(path) {
                        continue;
                    }

                    println!("[Cortex Worker] File changed: {}. Re-indexing...", path);
                    if let Ok(mut cortex) = Cortex::init() {
                        let _ = cortex.index_file(std::path::Path::new(path));
                        // No need for full workspace index, index_file handles individual files
                    }
                }
            }
        }
    }
}

fn is_indexable(path: &str) -> bool {
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
mod thread_probe {
    #[test]
    fn cortex_constructible_inside_a_thread() {
        let h = std::thread::spawn(|| {
            let _ = crate::cortex::Cortex::init().map(|c| c.chunk_count());
        });
        h.join().unwrap();
    }
}
