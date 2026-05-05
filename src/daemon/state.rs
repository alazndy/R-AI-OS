use std::sync::Arc;
use tokio::sync::RwLock;
use r_ai_os::indexer::ProjectIndex;
use r_ai_os::entities::EntityProject;

/// Represents the shared state managed by the daemon.
pub struct DaemonState {
    pub index: Option<ProjectIndex>,
    pub projects: Vec<EntityProject>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            index: None,
            projects: Vec::new(),
        }
    }
}

impl DaemonState {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
    }
}
