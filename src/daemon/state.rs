use crate::entities::EntityProject;
use crate::health::ProjectHealth;
use crate::indexer::ProjectIndex;
use crate::sentinel::SentinelState;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::daemon::proxy::AgentProcess;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SentinelFileStatus {
    pub path: String,
    pub state: SentinelState,
    pub errors: Vec<ValidationError>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FileChangeApproval {
    pub id: uuid::Uuid,
    pub path: String,
    pub original_content: String,
    pub new_content: String,
    pub agent_name: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ValidationError {
    pub file: String,
    pub message: String,
    pub line: Option<usize>,
    pub source: String, // "cargo check", "compliance", "security"
}

/// Represents the shared state managed by the daemon.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct DaemonState {
    #[serde(skip)]
    pub index: Option<ProjectIndex>,
    pub projects: Vec<EntityProject>,
    pub health_reports: Vec<ProjectHealth>,
    pub active_agents: Vec<AgentProcess>,
    pub pending_file_changes: Vec<FileChangeApproval>,
    pub handover_count: u32,
    pub needs_human_approval: bool,
    pub latest_errors: Vec<ValidationError>,
    pub sentinel_files: Vec<SentinelFileStatus>,
}

impl DaemonState {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
    }
}
