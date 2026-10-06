//! `FakeControlPlane`: the scriptable stand-in for the daemon used by UI-level tests.

use std::path::{Path, PathBuf};

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, WorkSnapshot};
use rwk_core::control::fake::{FakeControlPlane, RecordedCall};
use rwk_core::control::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
    QueuedHandoff, RegistrationOutcome,
};
use rwk_core::error::{AppError, ErrorCode};
use rwk_core::sessions::SessionKind;

fn handoff_request(summary: &str) -> CreateHandoffRequest {
    CreateHandoffRequest {
        project_root: PathBuf::from("/work/app"),
        from: SessionKind::Claude,
        to: SessionKind::Codex,
        summary: summary.into(),
        idempotency_key: IdempotencyKey::new("h-1"),
    }
}

fn queued(id: &str) -> QueuedHandoff {
    QueuedHandoff {
        approval_id: id.into(),
        project_path: PathBuf::from("/work/app"),
    }
}

#[test]
fn scripted_responses_are_returned_in_order_then_the_script_runs_out() {
    let fake = FakeControlPlane::new();
    fake.push_handoff(Ok(queued("a-1")))
        .push_handoff(Err(AppError::new(ErrorCode::Forbidden, "denied")));

    let first = fake.create_handoff(&handoff_request("one")).unwrap();
    let second = fake.create_handoff(&handoff_request("two")).unwrap_err();
    let third = fake.create_handoff(&handoff_request("three")).unwrap_err();

    assert_eq!(first, queued("a-1"));
    assert_eq!(second.code, ErrorCode::Forbidden);
    assert_eq!(third.code, ErrorCode::Internal);
    assert!(third.message.contains("no scripted response"));
}

#[test]
fn an_unscripted_call_fails_closed_and_never_allows_a_command() {
    let fake = FakeControlPlane::new();

    assert!(fake.work_snapshot().is_err());
    assert!(fake.govern_snapshot().is_err());
    assert!(fake.command_policy("launch_agent").is_err());
    assert!(fake
        .register_project(Path::new("/work/app"), &IdempotencyKey::new("r-1"))
        .is_err());
    assert!(fake
        .launch_agent(&LaunchAgentRequest {
            project_root: PathBuf::from("/work/app"),
            agent: SessionKind::Claude,
            prompt: None,
            idempotency_key: IdempotencyKey::new("l-1"),
        })
        .is_err());
    assert!(fake.handoff_receipt("a-1", Path::new("/work/app")).is_err());
}

#[test]
fn policies_are_looked_up_by_command_name() {
    let fake = FakeControlPlane::new();
    fake.set_policy("launch_agent", PolicyAction::Confirm)
        .set_policy("approve_handoff", PolicyAction::Deny);

    assert_eq!(
        fake.command_policy("launch_agent").unwrap(),
        PolicyAction::Confirm
    );
    assert_eq!(
        fake.command_policy("approve_handoff").unwrap(),
        PolicyAction::Deny
    );
    assert!(fake.command_policy("create_handoff").is_err());
}

#[test]
fn every_call_is_recorded_with_its_arguments_in_order() {
    let fake = FakeControlPlane::new();
    fake.push_work_snapshot(Ok(WorkSnapshot::default()))
        .push_govern_snapshot(Ok(GovernSnapshot::default()))
        .push_registration(Ok(RegistrationOutcome {
            project_path: PathBuf::from("/work/app"),
            already_registered: false,
        }))
        .push_launch(Ok(()))
        .push_receipt(Ok(HandoffReceiptDto {
            approval_id: "a-1".into(),
            project_path: "/work/app".into(),
            from_agent: "claude_kaira".into(),
            to_agent: "codex_kaira".into(),
            status: "pending".into(),
            resolved_by: None,
            resolved_at: None,
            delivered_by_target: false,
        }))
        .set_policy("launch_agent", PolicyAction::Allow);

    fake.work_snapshot().unwrap();
    fake.govern_snapshot().unwrap();
    fake.command_policy("launch_agent").unwrap();
    fake.register_project(Path::new("/work/app"), &IdempotencyKey::new("r-1"))
        .unwrap();
    let launch = LaunchAgentRequest {
        project_root: PathBuf::from("/work/app"),
        agent: SessionKind::Codex,
        prompt: Some("hello".into()),
        idempotency_key: IdempotencyKey::new("l-1"),
    };
    fake.launch_agent(&launch).unwrap();
    fake.handoff_receipt("a-1", Path::new("/work/app")).unwrap();

    assert_eq!(
        fake.calls(),
        vec![
            RecordedCall::WorkSnapshot,
            RecordedCall::GovernSnapshot,
            RecordedCall::CommandPolicy {
                command_name: "launch_agent".into()
            },
            RecordedCall::RegisterProject {
                project_root: PathBuf::from("/work/app"),
                key: IdempotencyKey::new("r-1"),
            },
            RecordedCall::LaunchAgent(launch),
            RecordedCall::HandoffReceipt {
                approval_id: "a-1".into(),
                project_root: PathBuf::from("/work/app"),
            },
        ]
    );
}

#[test]
fn a_call_that_fails_is_still_recorded() {
    let fake = FakeControlPlane::new();

    let _ = fake.create_handoff(&handoff_request("unscripted"));

    assert_eq!(
        fake.calls(),
        vec![RecordedCall::CreateHandoff(handoff_request("unscripted"))]
    );
}

#[test]
fn the_fake_can_stand_in_for_the_trait_object() {
    let fake = FakeControlPlane::new();
    fake.push_work_snapshot(Ok(WorkSnapshot::default()));
    let control: Box<dyn ControlPlane> = Box::new(fake);

    assert_eq!(control.work_snapshot().unwrap(), WorkSnapshot::default());
}
