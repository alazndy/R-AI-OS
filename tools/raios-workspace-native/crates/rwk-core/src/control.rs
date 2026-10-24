//! The boundary to the `aiosd` control plane.
//!
//! Rule: reads may bypass the daemon; every mutation goes through [`ControlPlane`] so the
//! daemon's policy and hash-chained audit ledger apply in one place.

pub mod daemon;
pub mod fake;
pub mod stream;

use std::path::{Path, PathBuf};
use std::time::Duration;

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, NowSnapshot, WorkSnapshot};

use crate::error::{AppError, ErrorCode, Result};
use crate::sessions::SessionKind;

/// Client-chosen key that makes a mutation safe to retry. A retry of the same user action
/// reuses the same key; a new user action generates a new one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Wraps a caller-generated unique string.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// A new key for a new user action.
    pub fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Policy action for one command, as reported by the daemon's `GetCommandPolicy` query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    /// Run without asking.
    Allow,
    /// Ask the user first (the UI shows its own consent dialog).
    Confirm,
    /// The daemon will refuse; do not send.
    Deny,
}

impl PolicyAction {
    /// Parses the daemon's lowercase label. Unknown labels are rejected; they are never
    /// treated as [`PolicyAction::Allow`].
    pub fn parse(label: &str) -> Result<Self> {
        match label {
            "allow" => Ok(Self::Allow),
            "confirm" => Ok(Self::Confirm),
            "deny" => Ok(Self::Deny),
            other => Err(AppError::new(
                ErrorCode::Daemon,
                format!("unknown policy action '{other}'"),
            )),
        }
    }
}

/// Asks the daemon to validate, authorize and audit an agent launch. The UI then spawns
/// `raios run` in its own PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgentRequest {
    /// Registered project root.
    pub project_root: PathBuf,
    /// Agent to start; must not be [`SessionKind::Shell`].
    pub agent: SessionKind,
    /// Optional first prompt, at most 600 characters, without credentials.
    pub prompt: Option<String>,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// Queues a `BLOCKER` handoff between two agent identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateHandoffRequest {
    /// Project root the handoff is scoped to.
    pub project_root: PathBuf,
    /// Handing-off agent; must not be [`SessionKind::Shell`].
    pub from: SessionKind,
    /// Receiving agent; must differ from `from` and must not be [`SessionKind::Shell`].
    pub to: SessionKind,
    /// Context for the receiver, at most 16,000 bytes, without credentials.
    pub summary: String,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// A decision on a pending handoff approval. The revision the user saw travels with the
/// decision, so a decision based on a stale read is refused by the daemon (`CONFLICT`) instead
/// of being applied to state that changed since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffDecisionRequest {
    /// The open project the decision is made from. Used for the consent context; the daemon
    /// addresses the approval by its id alone.
    pub project_root: PathBuf,
    /// Target approval identifier.
    pub approval_id: String,
    /// Revision of the approval as it was displayed to the user.
    pub expected_revision: i64,
    /// Rejection reason; `None` when approving.
    pub reason: Option<String>,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// Creates an operator task in a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTaskRequest {
    /// Project root the task belongs to.
    pub project_root: PathBuf,
    /// Validated title.
    pub title: String,
    /// Priority, 0 to 255.
    pub priority: u8,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// Sets the status of an operator task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTaskStatusRequest {
    /// Project root the task belongs to (the policy and the audit entry are scoped by it).
    pub project_root: PathBuf,
    /// Task to change.
    pub task_id: String,
    /// The new status.
    pub status: crate::tasks::TaskStatus,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// Assigns an operator task to an agent, or clears its assignee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignTaskRequest {
    /// Project root the task belongs to.
    pub project_root: PathBuf,
    /// Task to change.
    pub task_id: String,
    /// The agent that gets the task; `None` clears it. A shell is not an agent.
    pub assignee: Option<SessionKind>,
    /// The `updated_at` the task had when the user saw it; a newer change is not overwritten.
    pub expected_updated_at: Option<String>,
    /// Retry key.
    pub idempotency_key: IdempotencyKey,
}

/// A handoff the daemon accepted into its queue. The receiver has not started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedHandoff {
    /// Identifier to poll with [`ControlPlane::handoff_receipt`].
    pub approval_id: String,
    /// Project root the handoff is scoped to.
    pub project_path: PathBuf,
}

/// Result of registering a project directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationOutcome {
    /// Canonical registered path.
    pub project_path: PathBuf,
    /// True when the path was already registered and left untouched.
    pub already_registered: bool,
}

/// Reachability of the daemon, shown as a banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Requests succeed.
    Connected,
    /// Some requests fail; reads may be stale.
    Degraded {
        /// Short reason for the banner.
        reason: String,
    },
    /// The daemon cannot be reached; mutations fail closed.
    Unreachable {
        /// Short reason for the banner.
        reason: String,
    },
}

/// A change the UI should react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    /// A newer snapshot is available.
    SnapshotChanged {
        /// Snapshot sequence number reported by the daemon.
        sequence: u64,
    },
    /// The daemon's reachability changed.
    ConnectionChanged(ConnectionState),
    /// An agent run's execution state changed (typed `AgentRunStateChanged` frame).
    RunChanged {
        /// Target run identifier.
        run_id: String,
        /// Name of the agent harness.
        agent_name: String,
        /// New execution status string.
        status: String,
    },
    /// An approval was requested or resolved (typed `ApprovalRequested`/`ApprovalResolved`).
    ApprovalChanged {
        /// Target approval identifier.
        approval_id: String,
    },
}

/// Everything the workspace asks of the daemon. Blocking; call from a background executor.
pub trait ControlPlane: Send + Sync {
    /// Tasks, runs and projects, unfiltered; the inspector filters by exact project path.
    fn work_snapshot(&self) -> Result<WorkSnapshot>;

    /// Policy, audit and health projection.
    fn govern_snapshot(&self) -> Result<GovernSnapshot>;

    /// Pending approvals and active runs (the `now` projection), unfiltered; the inspector
    /// filters by exact project path. Approvals are not part of the govern projection.
    fn now_snapshot(&self) -> Result<NowSnapshot>;

    /// Policy action the daemon applies to `command_name` (snake_case, e.g. `launch_agent`).
    fn command_policy(&self, command_name: &str) -> Result<PolicyAction>;

    /// Registers an existing directory; an existing registration is left untouched.
    fn register_project(
        &self,
        project_root: &Path,
        key: &IdempotencyKey,
    ) -> Result<RegistrationOutcome>;

    /// Asks the daemon to validate, authorize and audit launching an agent in
    /// `request.project_root` **without starting any process**: the workspace runs the agent in
    /// its own PTY. `DaemonClient` sends `ValidateAgentLaunch`; the daemon's `LaunchAgent`
    /// opens an external terminal window and is never sent.
    fn launch_agent(&self, request: &LaunchAgentRequest) -> Result<()>;

    /// Queues a handoff.
    fn create_handoff(&self, request: &CreateHandoffRequest) -> Result<QueuedHandoff>;

    /// Approves a pending handoff approval: the control-plane owner's consent. This is not
    /// delivery — the addressed agent's own run still has to consume the handoff.
    fn approve_handoff(&self, request: &HandoffDecisionRequest) -> Result<()>;

    /// Rejects a pending handoff approval; `request.reason` must carry the reason.
    fn reject_handoff(&self, request: &HandoffDecisionRequest) -> Result<()>;

    /// Reads the delivery receipt of one handoff, scoped to `project_root`.
    fn handoff_receipt(&self, approval_id: &str, project_root: &Path) -> Result<HandoffReceiptDto>;

    /// Creates an operator task and returns its identifier.
    fn create_task(&self, request: &CreateTaskRequest) -> Result<String>;

    /// Sets the status of an operator task.
    fn update_task_status(&self, request: &UpdateTaskStatusRequest) -> Result<()>;

    /// Assigns an operator task and returns its new `updated_at`.
    fn assign_task(&self, request: &AssignTaskRequest) -> Result<String>;
}

/// Source of [`ControlEvent`]s. [`stream::WebSocketEvents`] subscribes to the daemon's typed
/// event stream; callers keep their own snapshot poll as the fallback.
pub trait EventSource: Send {
    /// Waits up to `timeout` for the next event; `Ok(None)` means nothing happened.
    fn next_event(&mut self, timeout: Duration) -> Result<Option<ControlEvent>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_policy_labels_parse() {
        assert_eq!(PolicyAction::parse("allow").unwrap(), PolicyAction::Allow);
        assert_eq!(
            PolicyAction::parse("confirm").unwrap(),
            PolicyAction::Confirm
        );
        assert_eq!(PolicyAction::parse("deny").unwrap(), PolicyAction::Deny);
    }

    #[test]
    fn unknown_policy_labels_fail_closed_and_never_allow() {
        for label in ["", "ALLOW", "Allow", "permit", "allow ", "yes", "none"] {
            let error = PolicyAction::parse(label).unwrap_err();
            assert_eq!(error.code, ErrorCode::Daemon, "{label:?}");
        }
    }

    #[test]
    fn idempotency_keys_compare_by_value() {
        assert_eq!(IdempotencyKey::new("k"), IdempotencyKey::new("k"));
        assert_ne!(IdempotencyKey::new("k"), IdempotencyKey::new("j"));
        assert_eq!(IdempotencyKey::new("k").as_str(), "k");
    }
}
