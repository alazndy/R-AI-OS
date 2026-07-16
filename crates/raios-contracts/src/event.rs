use serde::{Deserialize, Serialize};

use crate::dto::{LogEntryDto, ProjectHealthDto};
use crate::problem::Problem;
use crate::snapshot::SnapshotEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type", content = "payload")]
pub enum Event {
    SnapshotUpdated(Box<SnapshotEnvelope>),
    AgentRunStateChanged {
        run_id: String,
        agent_name: String,
        status: String,
    },
    ApprovalRequested {
        approval_id: String,
        kind: String,
        title: String,
        target: String,
    },
    ApprovalResolved {
        approval_id: String,
        status: String,
    },
    HealthDeltaUpdated {
        reports: Vec<ProjectHealthDto>,
    },
    LogAppended {
        log: LogEntryDto,
    },
    CommandSucceeded {
        idempotency_key: String,
        result: Option<serde_json::Value>,
    },
    CommandFailed {
        idempotency_key: String,
        problem: Problem,
    },
}
