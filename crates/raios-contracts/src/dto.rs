use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScoredApprovalDto {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub title: String,
    pub origin_agent: String,
    pub target_agent: String,
    pub project_path: Option<String>,
    pub created_at: String,
    pub score: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActiveRunDto {
    pub run_id: String,
    pub task_id: String,
    pub agent_name: String,
    pub project_name: String,
    pub status: String,
    pub duration_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BlockedTaskDto {
    pub task_id: String,
    pub title: String,
    pub project_name: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SystemAlertDto {
    pub id: String,
    pub level: String, // "INFO", "WARN", "ERROR"
    pub title: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectDto {
    pub path: String,
    pub name: String,
    pub status: String,
    pub git_branch: Option<String>,
    pub dirty_files: usize,
    pub last_active: Option<String>,
    /// Whether the tracked project has a `memory.md` file.
    #[serde(default)]
    pub has_memory: bool,
    /// Bounded, local-only summary of `memory.md` for the control-surface preview.
    #[serde(default)]
    pub memory_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UnifiedTaskDto {
    pub id: String,
    pub title: String,
    pub project_path: Option<String>,
    pub assignee: Option<String>,
    pub status: String,
    pub priority: u8,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ArtifactDto {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub title: String,
    pub file_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SearchResultDto {
    pub file_path: String,
    pub line_number: usize,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolTraceDto {
    pub id: String,
    pub tool_name: String,
    pub project_path: Option<String>,
    pub status: String,
    pub duration_ms: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LogEntryDto {
    pub timestamp: String,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PolicySummaryDto {
    pub enforce_sandbox: bool,
    pub egress_enabled: bool,
    pub default_action: String,
    pub total_rules: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AuditSummaryDto {
    pub total_records: usize,
    pub allowed_records: usize,
    pub denied_records: usize,
    pub confirmed_records: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectHealthDto {
    pub name: String,
    pub path: String,
    pub is_clean: bool,
    pub cve_count: usize,
    pub missing_docs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScheduledJobDto {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub status: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
}
