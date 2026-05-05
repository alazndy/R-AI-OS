#[path = "../daemon/mod.rs"]
mod daemon;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Starting R-AI-OS Daemon (aiosd)...");
    
    // Initialize daemon state
    let state = daemon::state::DaemonState::new();
    
    // Start background tasks
    let state_clone = state.clone();
    tokio::spawn(async move {
        use r_ai_os::config::Config;
        use r_ai_os::indexer::ProjectIndex;
        
        let config = Config::load().unwrap();
        
        println!("Background: Discovering projects...");
        let projects = r_ai_os::entities::discover_entities(&config.dev_ops_path);
        
        println!("Background: Building Neural Index...");
        if let Ok(idx) = ProjectIndex::build(&config.dev_ops_path) {
            let mut s = state_clone.write().await;
            s.index = Some(idx);
            s.projects = projects;
            println!("Background: Index & Projects ready.");
        } else {
            let mut s = state_clone.write().await;
            s.projects = projects;
            println!("Background: Projects ready (Index failed).");
        }
    });

    // Start IPC/TCP Server
    let server = daemon::server::Server::new(state.clone());
    server.run().await?;
    
    Ok(())
}
