use std::sync::Arc;
use tokio::sync::RwLock;
use crate::indexer::ProjectIndex;
use crate::entities::EntityProject;
use crate::health::ProjectHealth;

use crate::daemon::proxy::AgentProcess;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FileChangeApproval {
    pub id: uuid::Uuid,
    pub path: String,
    pub original_content: String,
    pub new_content: String,
    pub agent_name: String,
}

/// Represents the shared state managed by the daemon.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DaemonState {
    #[serde(skip)]
    pub index: Option<ProjectIndex>,
    pub projects: Vec<EntityProject>,
    pub health_reports: Vec<ProjectHealth>,
    pub active_agents: Vec<AgentProcess>,
    pub pending_file_changes: Vec<FileChangeApproval>,
    pub handover_count: u32,
    pub needs_human_approval: bool,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            index: None,
            projects: Vec::new(),
            health_reports: Vec::new(),
            active_agents: Vec::new(),
            pending_file_changes: Vec::new(),
            handover_count: 0,
            needs_human_approval: false,
        }
    }
}

impl DaemonState {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
    }
}
