use r_ai_os::daemon;
use r_ai_os::kernel::Kernel;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting R-AI-OS Daemon (aiosd) — Tri-Protocol Kernel...");

    let state = daemon::state::DaemonState::new();

    // Background project discovery and indexing
    let state_for_index = state.clone();
    tokio::spawn(async move {
        use r_ai_os::config::Config;
        use r_ai_os::indexer::ProjectIndex;

        let config = Config::load().unwrap();

        println!("[Kernel] Discovering projects...");
        let projects = r_ai_os::entities::discover_entities(&config.dev_ops_path);
        let _ = r_ai_os::entities::save_entities(&config.dev_ops_path, projects.clone());

        if config.daemon.startup_bm25_indexing {
            println!("[Kernel] Building Neural Index...");
            let bm25_db = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("raios")
                .join("workspace.db");

            if let Ok(idx) = ProjectIndex::load_or_build(&config.dev_ops_path, &bm25_db) {
                let mut s = state_for_index.write().await;
                s.index = Some(idx);
                s.projects = projects;
                println!("[Kernel] Index & Projects ready.");
            } else {
                let mut s = state_for_index.write().await;
                s.projects = projects;
                println!("[Kernel] Projects ready (BM25 index build failed).");
            }
        } else {
            let mut s = state_for_index.write().await;
            s.projects = projects;
            println!("[Kernel] Projects ready (startup BM25 indexing disabled).");
        }
    });

    // Start lock manager sweeper
    let lock_mgr = r_ai_os::lock_manager::LockManager::new();
    r_ai_os::lock_manager::spawn_sweeper(lock_mgr, std::time::Duration::from_secs(15));

    // Start tri-protocol kernel
    let kernel = Kernel::new(state);
    kernel.run().await?;

    Ok(())
}
