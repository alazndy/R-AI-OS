pub mod merge;
pub mod store;
pub mod worktree;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: Uuid,
    pub project_name: String,
    pub project_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub task_description: String,
    pub agent: String,
    pub status: SwarmStatus,
    pub created_at: String,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub artifact_id: Option<String>,
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SwarmStatus {
    Initializing,
    Running,
    AwaitingReview,
    Merged,
    Rejected,
    Failed(String),
}
