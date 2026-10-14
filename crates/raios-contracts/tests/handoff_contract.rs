use raios_contracts::{Command, CommandPolicyDto, HandoffReceiptDto, Query};

#[test]
fn create_handoff_round_trips_and_exposes_its_idempotency_key() {
    let command = Command::CreateHandoff {
        project_path: "/work/app".into(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        summary: "Continue the migration".into(),
        idempotency_key: "handoff-1".into(),
    };
    assert_eq!(command.idempotency_key(), "handoff-1");
    let json = serde_json::to_string(&command).unwrap();
    assert!(json.contains(r#""command_type":"CreateHandoff""#));
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command);
}

#[test]
fn register_project_round_trips_and_exposes_its_idempotency_key() {
    let command = Command::RegisterProject {
        project_path: "/work/app".into(),
        idempotency_key: "register-1".into(),
    };
    assert_eq!(command.idempotency_key(), "register-1");
    let json = serde_json::to_string(&command).unwrap();
    assert!(json.contains(r#""command_type":"RegisterProject""#));
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command);
}

#[test]
fn new_queries_round_trip_with_the_tagged_wire_format() {
    let receipt = Query::GetHandoffReceipt {
        approval_id: "approval-1".into(),
        project_path: "/work/app".into(),
    };
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains(r#""query_type":"GetHandoffReceipt""#));
    assert_eq!(serde_json::from_str::<Query>(&json).unwrap(), receipt);

    let policy = Query::GetCommandPolicy {
        command_name: "launch_agent".into(),
    };
    let json = serde_json::to_string(&policy).unwrap();
    assert!(json.contains(r#""query_type":"GetCommandPolicy""#));
    assert_eq!(serde_json::from_str::<Query>(&json).unwrap(), policy);
}

#[test]
fn response_dtos_round_trip() {
    let receipt = HandoffReceiptDto {
        approval_id: "approval-1".into(),
        project_path: "/work/app".into(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: "approved".into(),
        resolved_by: Some("codex_kaira".into()),
        resolved_at: Some("2026-10-05 15:00:00".into()),
        delivered_by_target: true,
    };
    let json = serde_json::to_string(&receipt).unwrap();
    assert_eq!(
        serde_json::from_str::<HandoffReceiptDto>(&json).unwrap(),
        receipt
    );

    let policy = CommandPolicyDto {
        command_name: "launch_agent".into(),
        action: "confirm".into(),
    };
    let json = serde_json::to_string(&policy).unwrap();
    assert_eq!(
        serde_json::from_str::<CommandPolicyDto>(&json).unwrap(),
        policy
    );
}
