extern crate self as raios_runtime;
pub mod agent_runner;
pub mod agent_wrapper;
pub mod anka;
pub mod bootstrap;
pub mod compliance;
pub mod compressor;
pub mod constitution;
pub mod control_plane;
pub mod cortex;
pub mod daemon;
pub mod daemon_client;
pub mod discovery;
pub mod factory;
pub mod filebrowser;
pub mod git_utils;
pub mod health;
pub mod intelligence;
pub mod job_executor;
pub mod kernel;
pub mod new_project;
pub mod obsidian;
pub mod product_factory;
pub mod proxy_store;
pub mod radar;
pub mod refactor_scan;
pub mod reflect_scoring;
pub mod search;
pub mod sentinel;
pub mod server;
pub mod session;
pub mod session_canvas;
pub mod session_memory;
pub mod session_review;
pub mod swarm;
pub mod sync;
pub mod system_scan;
pub mod tasks;
pub mod trace_recall;
pub mod workers;
pub mod cli {
    pub mod preflight;
}
// `RAIOS_DB_PATH` is process-global. Every test in this crate that reads or
// writes it (agent_runner::tests::wrapper_note_ipc,
// health::security_persistence_tests) must serialize on this SAME lock — two
// separate per-module Mutexes don't actually synchronize with each other and
// let parallel `cargo test` threads clobber each other's env var (same class
// of bug already fixed in raios-surface-cli/src/cli/mod.rs::DB_ENV_LOCK and
// raios-surface-mcp/src/mcp/mod.rs, see this repo's memory.md PR #26 entry).
#[cfg(test)]
pub(crate) static DB_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// `HOME`/`XDG_CONFIG_HOME` are process-global, and `raios_core::config::
// Config::load()` resolves through them. Every test that redirects them
// (server::http::auth IsolatedEnv, daemon::handlers dispatch_loop_tests
// IsolatedHome, control_plane::service snapshot_generation_on_in_memory_db)
// must serialize on this SAME lock — separate per-module Mutexes don't
// synchronize with each other (same bug class as DB_ENV_LOCK above).
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub use intelligence::edge;
pub use intelligence::evolution;
pub use intelligence::instinct;
pub use intelligence::router;
pub use search::hybrid as hybrid_search;
pub use search::indexer;
