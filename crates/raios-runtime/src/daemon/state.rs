use raios_core::entities::EntityProject;
use crate::health::ProjectHealth;
use crate::indexer::ProjectIndex;
use crate::sentinel::SentinelState;
use serde_json::json;
use std::collections::VecDeque;
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
    /// cp_approvals.id — canonical, stable identifier shared between daemon and TUI.
    pub id: String,
    pub path: String,
    pub original_content: String,
    pub new_content: String,
    pub agent_name: String,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub artifact_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ValidationError {
    pub file: String,
    pub message: String,
    pub line: Option<usize>,
    pub source: String, // "cargo check", "compliance", "security"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingDiff {
    pub id: String,
    pub project: String,
    pub file_path: String,
    pub original: String,
    pub proposed: String,
    pub agent: String,
    pub description: String,
    pub created_at: String,
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
    pub pending_diffs: VecDeque<PendingDiff>,
}

impl DaemonState {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
    }

    /// Reload pending file-change approvals from canonical DB state.
    /// Safe to call repeatedly; replaces the in-memory list atomically.
    pub fn refresh_pending_from_db(&mut self) {
        let Ok(conn) = raios_core::db::open_db() else { return };
        let Ok(rows) = raios_core::db::cp_load_pending_file_change_approvals(&conn) else { return };
        self.pending_file_changes = rows
            .into_iter()
            .map(|d| FileChangeApproval {
                id: d.approval_id,
                path: d.path,
                original_content: d.original_content,
                new_content: d.new_content,
                agent_name: d.agent_name,
                task_id: d.task_id,
                agent_run_id: d.agent_run_id,
                artifact_id: d.artifact_id,
            })
            .collect();
    }

    pub fn sync_payload(&self) -> serde_json::Value {
        json!({
            "event": "StateSync",
            "projects": self.projects,
            "health_reports": self.health_reports,
            "active_agents": self.active_agents,
            "index_ready": self.index.is_some(),
            "handover_count": self.handover_count,
            "pending_file_changes": self.pending_file_changes,
            "latest_errors": self.latest_errors,
            "sentinel_files": self.sentinel_files
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_payload_includes_latest_errors() {
        let state = DaemonState {
            latest_errors: vec![ValidationError {
                file: "src/main.rs".into(),
                message: "compile error".into(),
                line: Some(12),
                source: "cargo check".into(),
            }],
            ..Default::default()
        };

        let payload = state.sync_payload();
        assert_eq!(payload["event"], "StateSync");
        assert!(payload["latest_errors"].is_array());
        assert_eq!(payload["latest_errors"][0]["file"], "src/main.rs");
    }
}
