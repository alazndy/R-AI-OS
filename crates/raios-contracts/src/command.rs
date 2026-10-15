//! Control-plane command definitions.

use serde::{Deserialize, Serialize};

/// Typed control-plane commands issued to the resident daemon.
///
/// Every command variant carries a unique `idempotency_key` string used for
/// audit logging and request deduplication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command_type", content = "payload")]
pub enum Command {
    /// Approve a pending handoff approval request.
    ///
    /// This is the owner's consent, not the delivery of the handoff: the run ends as
    /// `handover_approved_by_owner` and the task stays open until the addressed agent's own
    /// wrapper consumes the handoff.
    ApproveHandoff {
        /// Target approval identifier.
        approval_id: String,
        /// Revision the caller observed for this approval. When present, the daemon refuses
        /// with `CONFLICT` if the approval changed since it was read. Optional for callers
        /// that have no revision to name.
        #[serde(default)]
        expected_revision: Option<i64>,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Reject a pending handoff approval request with a reason.
    RejectHandoff {
        /// Target approval identifier.
        approval_id: String,
        /// Revision the caller observed for this approval; see [`Command::ApproveHandoff`].
        #[serde(default)]
        expected_revision: Option<i64>,
        /// Human or automated reason for rejection.
        reason: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Approve a pending file mutation change.
    ApproveFileChange {
        /// Target approval identifier.
        approval_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Reject a pending file mutation change with a reason.
    RejectFileChange {
        /// Target approval identifier.
        approval_id: String,
        /// Reason explaining the rejection.
        reason: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Launch an agent invocation run.
    LaunchAgent {
        /// Target agent harness name (e.g. "claude", "codex", "opencode", "agy").
        agent_name: String,
        /// File system path to the target project directory.
        project_path: String,
        /// Optional initial prompt text to pass to the agent.
        prompt: Option<String>,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Validate, authorize and audit an agent launch **without starting any process**.
    ///
    /// Same payload and checks as [`Command::LaunchAgent`] (known agent, registered project,
    /// bounded prompt without credentials) and the same `launch_agent` security policy, but the
    /// daemon never opens a terminal. A client that runs the agent itself, such as the native
    /// workspace with its own PTY, sends this instead of `LaunchAgent`.
    ValidateAgentLaunch {
        /// Target agent harness name (e.g. "claude", "codex", "opencode", "agy").
        agent_name: String,
        /// Absolute path of a registered project directory.
        project_path: String,
        /// Optional initial prompt text the client will pass to the agent.
        prompt: Option<String>,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Terminate an active agent run.
    CancelAgentRun {
        /// Unique run identifier to terminate.
        run_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a new task item in the control plane.
    CreateTask {
        /// Short title describing the task.
        title: String,
        /// Optional absolute project-root path associated with the task.
        #[serde(alias = "project_id")]
        project_path: Option<String>,
        /// Task priority rating (0-255).
        priority: u8,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Update the status of an existing task item.
    UpdateTaskStatus {
        /// Target task identifier.
        task_id: String,
        /// New status string (e.g., "pending", "in_progress", "completed").
        status: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Immediately trigger execution of a scheduled cron job.
    TriggerCronJob {
        /// Identifier of the target scheduled job.
        job_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Toggle pause state for a scheduled cron job.
    ToggleCronJob {
        /// Identifier of the target scheduled job.
        job_id: String,
        /// `true` to pause scheduling, `false` to resume.
        paused: bool,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Perform a search across workspace files or index databases.
    ExecuteSearch {
        /// Search pattern or query text.
        query: String,
        /// Search engine mode: `"trigram"`, `"semantic"`, or `"all"`.
        mode: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Update an egress or execution policy rule.
    UpdatePolicyRule {
        /// Target policy rule identifier.
        rule_id: String,
        /// Action to set (e.g., "allow", "deny", "confirm").
        action: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Register an existing directory as a project so agents can be launched in it.
    RegisterProject {
        /// Absolute path to an existing project directory.
        project_path: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Queue a handoff (recorded as a `BLOCKER` continuation) from one agent identity to another.
    CreateHandoff {
        /// Absolute project root the handoff is scoped to.
        project_path: String,
        /// Agent identity that is handing off (for example `"claude_kaira"`).
        from_agent: String,
        /// Agent identity that should receive the context.
        to_agent: String,
        /// Context for the receiving agent, at most 16,000 bytes, without credentials.
        summary: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
}

impl Command {
    /// Returns the idempotency key string carried by this command variant.
    pub fn idempotency_key(&self) -> &str {
        match self {
            Command::ApproveHandoff {
                idempotency_key, ..
            } => idempotency_key,
            Command::RejectHandoff {
                idempotency_key, ..
            } => idempotency_key,
            Command::ApproveFileChange {
                idempotency_key, ..
            } => idempotency_key,
            Command::RejectFileChange {
                idempotency_key, ..
            } => idempotency_key,
            Command::LaunchAgent {
                idempotency_key, ..
            } => idempotency_key,
            Command::ValidateAgentLaunch {
                idempotency_key, ..
            } => idempotency_key,
            Command::CancelAgentRun {
                idempotency_key, ..
            } => idempotency_key,
            Command::CreateTask {
                idempotency_key, ..
            } => idempotency_key,
            Command::UpdateTaskStatus {
                idempotency_key, ..
            } => idempotency_key,
            Command::TriggerCronJob {
                idempotency_key, ..
            } => idempotency_key,
            Command::ToggleCronJob {
                idempotency_key, ..
            } => idempotency_key,
            Command::ExecuteSearch {
                idempotency_key, ..
            } => idempotency_key,
            Command::UpdatePolicyRule {
                idempotency_key, ..
            } => idempotency_key,
            Command::RegisterProject {
                idempotency_key, ..
            } => idempotency_key,
            Command::CreateHandoff {
                idempotency_key, ..
            } => idempotency_key,
        }
    }
}
