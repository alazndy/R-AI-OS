//! The boundary to the `aiosd` control plane.
//!
//! Rule: reads may bypass the daemon; every mutation goes through [`ControlPlane`] so the
//! daemon's policy and hash-chained audit ledger apply in one place.

pub mod daemon;
pub mod fake;

use std::path::{Path, PathBuf};
use std::time::Duration;

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, WorkSnapshot};

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

    /// The wire value.
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

/// Asks the daemon to validate and record an agent launch. The UI then spawns `raios run`.
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
}

/// Everything the workspace asks of the daemon. Blocking; call from a background executor.
pub trait ControlPlane: Send + Sync {
    /// Tasks, runs and projects, unfiltered; the inspector filters by exact project path.
    fn work_snapshot(&self) -> Result<WorkSnapshot>;

    /// Pending approvals, unfiltered; the inspector filters by exact project path.
    fn govern_snapshot(&self) -> Result<GovernSnapshot>;

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
    /// its own PTY. `DaemonClient` fails closed until the daemon has a command that does not
    /// spawn (the existing `LaunchAgent` opens an external terminal window).
    fn launch_agent(&self, request: &LaunchAgentRequest) -> Result<()>;

    /// Queues a handoff.
    fn create_handoff(&self, request: &CreateHandoffRequest) -> Result<QueuedHandoff>;

    /// Reads the delivery receipt of one handoff, scoped to `project_root`.
    fn handoff_receipt(&self, approval_id: &str, project_root: &Path) -> Result<HandoffReceiptDto>;
}

/// Source of [`ControlEvent`]s. The first implementation polls snapshots; a WebSocket source
/// can replace it without touching callers.
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
