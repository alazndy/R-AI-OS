//! Serialization-only transport contracts for the R-AI-OS control plane.
//!
//! Provides typed command, event, query, DTO, and domain snapshot definitions
//! shared across CLI, TUI, and daemon IPC boundaries.

#![warn(missing_docs)]

pub mod anka;
pub mod command;
pub mod dto;
pub mod event;
pub mod factory;
pub mod handoff;
pub mod problem;
pub mod query;
pub mod snapshot;

pub use anka::{AnkaHitDto, AnkaIndexStatusDto, AnkaSearchRequestDto};
pub use command::Command;
pub use dto::*;
pub use event::Event;
pub use factory::{
    FactoryCommand, FactoryEvent, FactoryMode, FactoryOverviewSnapshot, FactoryProductSummaryDto,
    FactoryQuery,
};
pub use handoff::{CommandPolicyDto, HandoffReceiptDto};
pub use problem::Problem;
pub use query::Query;
pub use snapshot::{ExploreSnapshot, GovernSnapshot, NowSnapshot, SnapshotEnvelope, WorkSnapshot};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_serialization_roundtrip() {
        let q = Query::GetExploreSnapshot {
            search_query: Some("test".into()),
            log_filter: None,
        };
        let json = serde_json::to_string(&q).unwrap();
        let deserialized: Query = serde_json::from_str(&json).unwrap();
        assert_eq!(q, deserialized);
    }

    #[test]
    fn command_serialization_roundtrip() {
        let cmd = Command::ApproveHandoff {
            approval_id: "app-123".into(),
            expected_revision: None,
            idempotency_key: "key-456".into(),
        };
        assert_eq!(cmd.idempotency_key(), "key-456");
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn handoff_decision_payloads_stay_backward_compatible() {
        // A payload written before `expected_revision` existed still deserializes; the
        // revision is then simply unknown and never blocks the decision.
        let legacy = r#"{
            "command_type": "ApproveHandoff",
            "payload": {"approval_id": "app-1", "idempotency_key": "key-1"}
        }"#;
        let parsed: Command = serde_json::from_str(legacy).unwrap();
        match parsed {
            Command::ApproveHandoff {
                approval_id,
                expected_revision,
                idempotency_key,
            } => {
                assert_eq!(approval_id, "app-1");
                assert_eq!(expected_revision, None);
                assert_eq!(idempotency_key, "key-1");
            }
            other => panic!("wrong variant: {other:?}"),
        }

        // A current payload names the revision it saw and round-trips unchanged.
        let current = Command::RejectHandoff {
            approval_id: "app-2".into(),
            expected_revision: Some(7),
            reason: "stale work".into(),
            idempotency_key: "key-2".into(),
        };
        let json = serde_json::to_string(&current).unwrap();
        assert!(json.contains("\"expected_revision\":7"));
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), current);
    }

    #[test]
    fn create_task_accepts_legacy_project_id_payloads() {
        let json = r#"{
            "command_type":"CreateTask",
            "payload":{
                "title":"Compatibility task",
                "project_id":"/workspace/example",
                "priority":50,
                "idempotency_key":"legacy-project-id"
            }
        }"#;

        let command: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(
            command,
            Command::CreateTask {
                project_path: Some(ref path),
                ..
            } if path == "/workspace/example"
        ));
    }

    #[test]
    fn problem_construction() {
        let p = Problem::unauthorized("Access denied");
        assert_eq!(p.code, "UNAUTHORIZED");
        assert!(!p.retryable);
    }

    #[test]
    fn project_dto_accepts_snapshots_from_pre_memory_preview_daemons() {
        let json = r#"{
            "path":"/workspace/example",
            "name":"example",
            "status":"active",
            "git_branch":null,
            "dirty_files":0,
            "last_active":null
        }"#;

        let project: ProjectDto = serde_json::from_str(json).unwrap();
        assert!(!project.has_memory);
        assert_eq!(project.memory_preview, None);
    }
}
