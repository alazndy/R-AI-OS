use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Ready,
    Running,
    Blocked,
    AwaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Pending,
    Starting,
    Running,
    AwaitingInput,
    AwaitingApproval,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Patch,
    Diff,
    FileChange,
    BuildLog,
    TestReport,
    SecurityReport,
    ResearchNote,
    HandoverNote,
    SessionNote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Draft,
    Submitted,
    Approved,
    Rejected,
    Applied,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalType {
    FileWrite,
    Merge,
    Handover,
    NetworkException,
    ToolQuarantine,
    BudgetOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetConfidence {
    Exact,
    Estimated,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlTask {
    pub id: String,
    pub project_id: Option<i64>,
    pub plan_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub description: String,
    pub priority: i64,
    pub status: TaskStatus,
    pub assignee_kind: Option<String>,
    pub assignee_id: Option<String>,
    pub acceptance_criteria: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub task_id: String,
    pub project_id: Option<i64>,
    pub provider: String,
    pub agent_name: String,
    pub run_contract_id: String,
    pub attempt: i64,
    pub status: AgentRunStatus,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub exit_reason: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub task_id: String,
    pub agent_run_id: String,
    pub kind: ArtifactKind,
    pub status: ArtifactStatus,
    pub path: Option<String>,
    pub content_ref: Option<String>,
    pub metadata_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub project_id: Option<i64>,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub artifact_id: Option<String>,
    pub approval_type: ApprovalType,
    pub reason: String,
    pub status: ApprovalStatus,
    pub requested_at: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetLedgerEntry {
    pub id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub provider: Option<String>,
    pub metric: String,
    pub limit_value: Option<f64>,
    pub used_value: Option<f64>,
    pub remaining_value: Option<f64>,
    pub reset_at: Option<String>,
    pub confidence: BudgetConfidence,
    pub source: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangeWorkflowIds {
    pub task_id: String,
    pub agent_run_id: String,
    pub artifact_id: String,
    pub approval_id: String,
    pub project_id: Option<i64>,
}
