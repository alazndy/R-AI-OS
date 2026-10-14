//! `ConsentRegistry`: the UI-local consent step before a `confirm` operation is sent.

use std::path::Path;
use std::thread;

use rwk_core::consent::{ConsentDecision, ConsentRegistry};
use rwk_core::error::ErrorCode;

fn begin(registry: &ConsentRegistry) -> (String, std::sync::mpsc::Receiver<ConsentDecision>) {
    let (request, receiver) = registry
        .begin(
            "launch_agent",
            Path::new("/work/app"),
            "Start Claude in /work/app",
        )
        .unwrap();
    (request.id, receiver)
}

#[test]
fn a_request_carries_the_operation_the_dialog_shows() {
    let registry = ConsentRegistry::new();

    let (request, _receiver) = registry
        .begin(
            "create_handoff",
            Path::new("/work/app"),
            "Hand off to Codex",
        )
        .unwrap();

    assert!(!request.id.is_empty());
    assert_eq!(request.command_name, "create_handoff");
    assert_eq!(request.project_root, Path::new("/work/app"));
    assert_eq!(request.summary, "Hand off to Codex");
}

#[test]
fn only_one_request_may_be_pending_at_a_time() {
    let registry = ConsentRegistry::new();
    let (id, receiver) = begin(&registry);

    let busy = registry
        .begin("launch_agent", Path::new("/other"), "Another")
        .unwrap_err();
    assert_eq!(busy.code, ErrorCode::Conflict);

    registry.decide(&id, ConsentDecision::Approved).unwrap();
    registry.wait(&id, receiver).unwrap();
    assert!(registry
        .begin("launch_agent", Path::new("/other"), "Another")
        .is_ok());
}

#[test]
fn an_approval_resolves_the_waiter_and_is_single_use() {
    let registry = ConsentRegistry::new();
    let (id, receiver) = begin(&registry);

    registry.decide(&id, ConsentDecision::Approved).unwrap();

    assert_eq!(
        registry.wait(&id, receiver).unwrap(),
        ConsentDecision::Approved
    );
    let again = registry.decide(&id, ConsentDecision::Approved).unwrap_err();
    assert_eq!(again.code, ErrorCode::NotFound);
}

#[test]
fn a_denial_is_reported_as_denied_and_never_as_approval() {
    let registry = ConsentRegistry::new();
    let (id, receiver) = begin(&registry);

    registry.decide(&id, ConsentDecision::Denied).unwrap();

    assert_eq!(
        registry.wait(&id, receiver).unwrap(),
        ConsentDecision::Denied
    );
    assert!(registry.decide(&id, ConsentDecision::Approved).is_err());
}

#[test]
fn deciding_an_unknown_request_is_not_found() {
    let registry = ConsentRegistry::new();
    let (_id, _receiver) = begin(&registry);

    let error = registry
        .decide("not-a-request", ConsentDecision::Approved)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn a_waiter_blocks_until_another_thread_decides() {
    let registry = ConsentRegistry::new();
    let (id, receiver) = begin(&registry);

    let outcome = thread::scope(|scope| {
        let waiter = scope.spawn(|| registry.wait(&id, receiver));
        registry.decide(&id, ConsentDecision::Approved).unwrap();
        waiter.join().unwrap()
    });

    assert_eq!(outcome.unwrap(), ConsentDecision::Approved);
}

#[test]
fn shutdown_wakes_the_waiter_with_an_error_and_refuses_everything_after() {
    let registry = ConsentRegistry::new();
    let (id, receiver) = begin(&registry);

    registry.shutdown();

    assert_eq!(
        registry.wait(&id, receiver).unwrap_err().code,
        ErrorCode::WorkspaceClosed
    );
    assert_eq!(
        registry
            .decide(&id, ConsentDecision::Approved)
            .unwrap_err()
            .code,
        ErrorCode::WorkspaceClosed
    );
    assert_eq!(
        registry
            .begin("launch_agent", Path::new("/work/app"), "x")
            .unwrap_err()
            .code,
        ErrorCode::WorkspaceClosed
    );
}
