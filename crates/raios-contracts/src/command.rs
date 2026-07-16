use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command_type", content = "payload")]
pub enum Command {
    ApproveHandoff {
        approval_id: String,
        idempotency_key: String,
    },
    RejectHandoff {
        approval_id: String,
        reason: String,
        idempotency_key: String,
    },
    ApproveFileChange {
        approval_id: String,
        idempotency_key: String,
    },
    RejectFileChange {
        approval_id: String,
        reason: String,
        idempotency_key: String,
    },
    LaunchAgent {
        agent_name: String,
        project_path: String,
        prompt: Option<String>,
        idempotency_key: String,
    },
    CancelAgentRun {
        run_id: String,
        idempotency_key: String,
    },
    CreateTask {
        title: String,
        project_id: Option<String>,
        priority: u8,
        idempotency_key: String,
    },
    UpdateTaskStatus {
        task_id: String,
        status: String,
        idempotency_key: String,
    },
    TriggerCronJob {
        job_id: String,
        idempotency_key: String,
    },
    ToggleCronJob {
        job_id: String,
        paused: bool,
        idempotency_key: String,
    },
    ExecuteSearch {
        query: String,
        mode: String, // "trigram", "semantic", "all"
        idempotency_key: String,
    },
    UpdatePolicyRule {
        rule_id: String,
        action: String,
        idempotency_key: String,
    },
}

impl Command {
    pub fn idempotency_key(&self) -> &str {
        match self {
            Command::ApproveHandoff { idempotency_key, .. } => idempotency_key,
            Command::RejectHandoff { idempotency_key, .. } => idempotency_key,
            Command::ApproveFileChange { idempotency_key, .. } => idempotency_key,
            Command::RejectFileChange { idempotency_key, .. } => idempotency_key,
            Command::LaunchAgent { idempotency_key, .. } => idempotency_key,
            Command::CancelAgentRun { idempotency_key, .. } => idempotency_key,
            Command::CreateTask { idempotency_key, .. } => idempotency_key,
            Command::UpdateTaskStatus { idempotency_key, .. } => idempotency_key,
            Command::TriggerCronJob { idempotency_key, .. } => idempotency_key,
            Command::ToggleCronJob { idempotency_key, .. } => idempotency_key,
            Command::ExecuteSearch { idempotency_key, .. } => idempotency_key,
            Command::UpdatePolicyRule { idempotency_key, .. } => idempotency_key,
        }
    }
}
