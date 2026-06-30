pub mod capability;
pub mod evolution;
pub mod file;
pub mod jobs;
pub mod swarm;
pub mod task_graph;

/// Signal returned by command handlers that affect the connection loop.
pub enum CmdResult {
    /// Normal completion — caller continues the main loop.
    Ok,
    /// Handler issued an early `continue` on the outer loop (skips line.clear()).
    Continue,
    /// Fatal condition — terminate the connection immediately.
    Disconnect,
}
