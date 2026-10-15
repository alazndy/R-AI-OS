use super::*;
use rusqlite::Connection;

fn input<'a>(project_path: &'a str, to_agent: &'a str, msg: &'a str) -> HandoffWorkflowInput<'a> {
    HandoffWorkflowInput {
        project_path,
        from_agent: "claude_kaira",
        to_agent,
        status: "BLOCKER",
        msg,
        diff_stat: None,
        report: None,
    }
}

fn approval_status(conn: &Connection, approval_id: &str) -> String {
    conn.query_row(
        "SELECT status FROM cp_approvals WHERE id = ?1",
        [approval_id],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn handoff_workflow_in_tx_rolls_back_with_the_callers_transaction() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let ids =
        create_handoff_workflow_in_tx(&tx, input("/repo-a", "codex_kaira", "inside caller tx"))
            .unwrap();
    drop(tx);

    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_approvals WHERE id = ?1",
            [&ids.approval_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        rows, 0,
        "dropping the caller's transaction must roll the handoff back"
    );
}

#[test]
fn handoff_workflow_in_tx_commits_with_the_callers_transaction() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let ids =
        create_handoff_workflow_in_tx(&tx, input("/repo-a", "codex_kaira", "inside caller tx"))
            .unwrap();
    tx.commit().unwrap();

    assert_eq!(approval_status(&conn, &ids.approval_id), "pending");
}

#[test]
fn handoff_to_one_unregistered_project_does_not_supersede_another() {
    let conn = in_memory();
    let first = create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "for a")).unwrap();
    let other = create_handoff_workflow(&conn, input("/repo-b", "codex_kaira", "for b")).unwrap();

    assert_eq!(approval_status(&conn, &first.approval_id), "pending");
    assert_eq!(approval_status(&conn, &other.approval_id), "pending");

    let again =
        create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "for a again")).unwrap();
    assert_eq!(approval_status(&conn, &first.approval_id), "expired");
    assert_eq!(approval_status(&conn, &again.approval_id), "pending");
    assert_eq!(approval_status(&conn, &other.approval_id), "pending");
}

#[test]
fn receipt_reports_a_pending_handoff_with_its_agents_and_path() {
    let conn = in_memory();
    let ids = create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "ctx")).unwrap();

    let receipt = cp_handoff_receipt(&conn, &ids.approval_id, "/repo-a")
        .unwrap()
        .expect("receipt");
    assert_eq!(receipt.status, "pending");
    assert_eq!(receipt.project_path, "/repo-a");
    assert_eq!(receipt.from_agent, "claude_kaira");
    assert_eq!(receipt.to_agent, "codex_kaira");
    assert_eq!(receipt.resolved_by, None);
    assert!(!receipt.delivered_by_target());
}

#[test]
fn receipt_is_scoped_to_its_project() {
    let conn = in_memory();
    let ids = create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "ctx")).unwrap();

    assert!(cp_handoff_receipt(&conn, &ids.approval_id, "/repo-b")
        .unwrap()
        .is_none());
    assert!(cp_handoff_receipt(&conn, "missing-id", "/repo-a")
        .unwrap()
        .is_none());
}

#[test]
fn receipt_marks_delivery_only_when_the_target_consumed_it() {
    let conn = in_memory();
    let ids = create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "ctx")).unwrap();
    let pending = cp_take_pending_handoff_for_path(&conn, "/repo-a", "codex_kaira")
        .unwrap()
        .expect("pending handoff");

    cp_consume_handoff(&conn, &pending, "codex_kaira").unwrap();
    let receipt = cp_handoff_receipt(&conn, &ids.approval_id, "/repo-a")
        .unwrap()
        .expect("receipt");
    assert_eq!(receipt.status, "approved");
    assert_eq!(receipt.resolved_by.as_deref(), Some("codex_kaira"));
    assert!(receipt.delivered_by_target());
}

#[test]
fn receipt_resolved_by_someone_else_is_not_target_delivery() {
    let conn = in_memory();
    let ids = create_handoff_workflow(&conn, input("/repo-a", "codex_kaira", "ctx")).unwrap();
    // A human approval (ApproveHandoff) resolves the handoff as the control owner. Consuming it
    // through the wrapper path requires the receiver itself, so that path cannot be used by
    // anyone else.
    let pending = cp_take_pending_handoff_for_path(&conn, "/repo-a", "codex_kaira")
        .unwrap()
        .expect("pending handoff");
    cp_consume_handoff(&conn, &pending, "local_control_owner").unwrap();
    let still_pending = cp_handoff_receipt(&conn, &ids.approval_id, "/repo-a")
        .unwrap()
        .expect("receipt");
    assert_eq!(still_pending.status, "pending");

    assert!(cp_resolve_handoff(
        &conn,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner",
        None
    )
    .unwrap());
    let receipt = cp_handoff_receipt(&conn, &ids.approval_id, "/repo-a")
        .unwrap()
        .expect("receipt");
    assert_eq!(receipt.status, "approved");
    assert!(!receipt.delivered_by_target());
}
