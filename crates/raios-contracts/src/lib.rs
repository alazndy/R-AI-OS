pub mod command;
pub mod dto;
pub mod event;
pub mod problem;
pub mod query;
pub mod snapshot;

pub use command::Command;
pub use dto::*;
pub use event::Event;
pub use problem::Problem;
pub use query::Query;
pub use snapshot::{
    ExploreSnapshot, GovernSnapshot, NowSnapshot, SnapshotEnvelope, WorkSnapshot,
};

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
            idempotency_key: "key-456".into(),
        };
        assert_eq!(cmd.idempotency_key(), "key-456");
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn problem_construction() {
        let p = Problem::unauthorized("Access denied");
        assert_eq!(p.code, "UNAUTHORIZED");
        assert!(!p.retryable);
    }
}
