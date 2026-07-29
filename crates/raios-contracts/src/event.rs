//! Control-plane asynchronous event contract definitions.

use serde::{Deserialize, Serialize};

use crate::dto::{LogEntryDto, ProjectHealthDto};
use crate::problem::Problem;
use crate::snapshot::SnapshotEnvelope;

/// Asynchronous control-plane events emitted by the resident daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type", content = "payload")]
pub enum Event {
    /// A new system-wide snapshot envelope was published.
    SnapshotUpdated(Box<SnapshotEnvelope>),
    /// Execution state of an active agent run changed.
    AgentRunStateChanged {
        /// Target run identifier.
        run_id: String,
        /// Name of the agent harness.
        agent_name: String,
        /// New execution status string.
        status: String,
    },
    /// A new approval request was registered.
    ApprovalRequested {
        /// Target approval identifier.
        approval_id: String,
        /// Approval request kind (e.g. `"handoff"`, `"file_change"`).
        kind: String,
        /// Short title describing the approval request.
        title: String,
        /// Target agent or resource identifier.
        target: String,
    },
    /// An approval request was resolved (approved or rejected).
    ApprovalResolved {
        /// Target approval identifier.
        approval_id: String,
        /// Resolution status string (e.g., `"approved"`, `"rejected"`).
        status: String,
    },
    /// Project health reports were updated.
    HealthDeltaUpdated {
        /// Updated list of project health reports.
        reports: Vec<ProjectHealthDto>,
    },
    /// A new log record entry was appended to the log stream.
    LogAppended {
        /// Appended log record payload.
        log: LogEntryDto,
    },
    /// A submitted command completed successfully.
    CommandSucceeded {
        /// Idempotency key of the executed command.
        idempotency_key: String,
        /// Optional command result payload.
        result: Option<serde_json::Value>,
    },
    /// A submitted command failed during execution.
    CommandFailed {
        /// Idempotency key of the failed command.
        idempotency_key: String,
        /// Problem detail payload describing the failure.
        problem: Problem,
    },
}
