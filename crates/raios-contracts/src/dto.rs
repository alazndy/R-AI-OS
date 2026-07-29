//! Data Transfer Objects (DTOs) for the R-AI-OS control plane.

use serde::{Deserialize, Serialize};

/// Pending handoff or file approval request with an urgency score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScoredApprovalDto {
    /// Unique approval request identifier.
    pub id: String,
    /// ID of the associated task item.
    pub task_id: String,
    /// Approval type (e.g. `"handoff"`, `"file_change"`).
    pub kind: String,
    /// Title describing the approval request.
    pub title: String,
    /// Agent that requested the approval.
    pub origin_agent: String,
    /// Target agent recipient.
    pub target_agent: String,
    /// Optional file system path to the target project directory.
    pub project_path: Option<String>,
    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,
    /// Urgency risk score assigned to this approval request.
    pub score: i32,
    /// Explanation of why approval is required.
    pub reason: String,
}

/// Active agent run state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActiveRunDto {
    /// Unique run session identifier.
    pub run_id: String,
    /// Associated task identifier.
    pub task_id: String,
    /// Name of the agent harness running the task.
    pub agent_name: String,
    /// Display name of the active project.
    pub project_name: String,
    /// Current execution status string (e.g., `"running"`, `"paused"`).
    pub status: String,
    /// Elapsed execution time in seconds.
    pub duration_secs: u64,
}

/// Task item blocked on external action or dependency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BlockedTaskDto {
    /// Unique task identifier.
    pub task_id: String,
    /// Short task title.
    pub title: String,
    /// Display name of the associated project.
    pub project_name: String,
    /// Explanation of why the task is blocked.
    pub reason: String,
    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,
}

/// High-level system alert message displayed on the Now route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SystemAlertDto {
    /// Unique alert identifier.
    pub id: String,
    /// Alert severity level (`"INFO"`, `"WARN"`, or `"ERROR"`).
    pub level: String,
    /// Short summary title of the alert.
    pub title: String,
    /// Detailed alert description message.
    pub message: String,
    /// ISO-8601 UTC timestamp of the alert.
    pub timestamp: String,
}

/// Tracked project metadata and memory posture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectDto {
    /// Absolute file system path to the project root directory.
    pub path: String,
    /// Display name of the project.
    pub name: String,
    /// Project lifecycle status string (e.g., `"active"`, `"standby"`).
    pub status: String,
    /// Currently checked-out Git branch name, if any.
    pub git_branch: Option<String>,
    /// Number of modified or untracked files in the working directory.
    pub dirty_files: usize,
    /// ISO-8601 UTC timestamp when the project was last active.
    pub last_active: Option<String>,
    /// Whether the tracked project has a `memory.md` file.
    #[serde(default)]
    pub has_memory: bool,
    /// Bounded, local-only summary of `memory.md` for the control-surface preview.
    #[serde(default)]
    pub memory_preview: Option<String>,
}

/// System-wide task item representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UnifiedTaskDto {
    /// Unique task identifier.
    pub id: String,
    /// Short task title.
    pub title: String,
    /// Optional project root directory path.
    pub project_path: Option<String>,
    /// Assigned agent or human user identifier.
    pub assignee: Option<String>,
    /// Current task status string.
    pub status: String,
    /// Numerical task priority rating.
    pub priority: u8,
    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,
}

/// Generated artifact metadata entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ArtifactDto {
    /// Unique artifact identifier.
    pub id: String,
    /// Associated task identifier.
    pub task_id: String,
    /// Artifact kind classifier (e.g., `"report"`, `"diff"`, `"log"`).
    pub kind: String,
    /// Human-readable artifact title.
    pub title: String,
    /// Optional file path on disk where the artifact is saved.
    pub file_path: Option<String>,
    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,
}

/// Search result match entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SearchResultDto {
    /// File path where the match occurred.
    pub file_path: String,
    /// 1-based line number of the match.
    pub line_number: usize,
    /// Text snippet surrounding the match.
    pub snippet: String,
    /// Search relevance score.
    pub score: f32,
}

/// Tool execution trace log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolTraceDto {
    /// Unique trace identifier.
    pub id: String,
    /// Name of the executed tool.
    pub tool_name: String,
    /// Optional project path context.
    pub project_path: Option<String>,
    /// Execution status string (e.g., `"success"`, `"failed"`).
    pub status: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// ISO-8601 UTC execution timestamp.
    pub timestamp: String,
}

/// Append-only log record entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LogEntryDto {
    /// ISO-8601 UTC log timestamp.
    pub timestamp: String,
    /// Log category or source tag.
    pub category: String,
    /// Log message text.
    pub message: String,
}

/// Security and execution policy posture summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PolicySummaryDto {
    /// Whether sandbox isolation is strictly enforced.
    pub enforce_sandbox: bool,
    /// Whether network egress monitoring is enabled.
    pub egress_enabled: bool,
    /// Default policy evaluation action (e.g., `"deny"`, `"allow"`).
    pub default_action: String,
    /// Total number of active policy rules.
    pub total_rules: usize,
}

/// Audit trail summary counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AuditSummaryDto {
    /// Total audit records logged.
    pub total_records: usize,
    /// Records for allowed actions.
    pub allowed_records: usize,
    /// Records for denied actions.
    pub denied_records: usize,
    /// Records for human-confirmed actions.
    pub confirmed_records: usize,
}

/// Health check report for a tracked project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectHealthDto {
    /// Project display name.
    pub name: String,
    /// File system path to the project root directory.
    pub path: String,
    /// Whether the Git working tree is clean.
    pub is_clean: bool,
    /// Count of known CVE vulnerabilities detected.
    pub cve_count: usize,
    /// List of expected documentation files missing from the project.
    pub missing_docs: Vec<String>,
}

/// Status of a scheduled background job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScheduledJobDto {
    /// Unique job identifier.
    pub id: String,
    /// Display name of the job.
    pub name: String,
    /// Cron schedule expression.
    pub schedule: String,
    /// Command line executed by the job.
    pub command: String,
    /// Current job status (e.g., `"active"`, `"paused"`).
    pub status: String,
    /// Optional ISO-8601 UTC timestamp of the last execution.
    pub last_run: Option<String>,
    /// Optional ISO-8601 UTC timestamp of the next planned execution.
    pub next_run: Option<String>,
}
