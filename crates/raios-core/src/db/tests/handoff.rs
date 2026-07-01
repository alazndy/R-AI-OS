use super::*;

#[test]
fn handoff_workflow_round_trip_consumed() {
    let conn = in_memory();
    let project_id = upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();

    let ids = create_handoff_workflow(
        &conn,
        "/repo",
        "claude_kaira",
        "opencode_kaira",
        "SUCCESS",
        "skeleton ready, implement auth handlers",
        Some(" src/db.rs | 12 ++++++++----"),
    )
    .unwrap();
    assert_eq!(ids.project_id, Some(project_id));

    let pending = cp_take_pending_handoff(&conn, Some(project_id), "opencode_kaira")
        .unwrap()
        .expect("handoff should be pending for the assignee");
    assert_eq!(pending.from_agent, "claude_kaira");
    assert_eq!(pending.to_agent, "opencode_kaira");
    assert_eq!(pending.status, "success");
    assert_eq!(pending.context_summary, "skeleton ready, implement auth handlers");
    assert_eq!(
        pending.diff_stat.as_deref(),
        Some(" src/db.rs | 12 ++++++++----")
    );

    cp_consume_handoff(&conn, &pending, "opencode_kaira").unwrap();

    let after = cp_take_pending_handoff(&conn, Some(project_id), "opencode_kaira").unwrap();
    assert!(after.is_none());

    let approval_status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![ids.approval_id],
            |row| row.get(0),
        )
        .unwrap();
    let task_status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            params![ids.task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(approval_status, "approved");
    assert_eq!(task_status, "completed");
}

#[test]
fn handoff_workflow_not_visible_to_other_agent() {
    let conn = in_memory();
    create_handoff_workflow(
        &conn,
        "/tmp/unknown-project",
        "claude_kaira",
        "opencode_kaira",
        "BLOCKER",
        "stuck on flaky test, needs investigation",
        None,
    )
    .unwrap();

    let for_wrong_agent =
        cp_take_pending_handoff(&conn, None, "antigravity_kaira").unwrap();
    assert!(for_wrong_agent.is_none());

    let for_right_agent = cp_take_pending_handoff(&conn, None, "opencode_kaira")
        .unwrap()
        .expect("handoff should be visible regardless of project filter when None");
    assert_eq!(for_right_agent.status, "blocker");
}

#[test]
fn newer_handoff_supersedes_stale_pending_one_for_same_assignee() {
    let conn = in_memory();
    let project_id = upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();

    let stale = create_handoff_workflow(
        &conn,
        "/repo",
        "claude_kaira",
        "opencode_kaira",
        "BLOCKER",
        "stale note nobody picked up",
        None,
    )
    .unwrap();
    let fresh = create_handoff_workflow(
        &conn,
        "/repo",
        "claude_kaira",
        "opencode_kaira",
        "SUCCESS",
        "fresh note replaces the stale one",
        None,
    )
    .unwrap();

    let pending = cp_take_pending_handoff(&conn, Some(project_id), "opencode_kaira")
        .unwrap()
        .expect("fresh handoff should be pending");
    assert_eq!(pending.context_summary, "fresh note replaces the stale one");
    assert_eq!(pending.task_id, fresh.task_id);

    let stale_approval_status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![stale.approval_id],
            |row| row.get(0),
        )
        .unwrap();
    let stale_artifact_status: String = conn
        .query_row(
            "SELECT status FROM cp_artifacts WHERE id = ?1",
            params![stale.artifact_id],
            |row| row.get(0),
        )
        .unwrap();
    let stale_task_status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            params![stale.task_id],
            |row| row.get(0),
        )
        .unwrap();
    let stale_run_status: String = conn
        .query_row(
            "SELECT status FROM cp_agent_runs WHERE id = ?1",
            params![stale.agent_run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stale_approval_status, "expired");
    assert_eq!(stale_artifact_status, "superseded");
    assert_eq!(stale_task_status, "cancelled");
    assert_eq!(
        stale_run_status, "cancelled",
        "superseded handoff's agent_run must not linger as awaiting_approval forever"
    );
}

#[test]
fn handoff_tasks_do_not_leak_into_personal_task_list() {
    let conn = in_memory();
    create_handoff_workflow(
        &conn,
        "/repo",
        "claude_kaira",
        "codex_kaira",
        "SUCCESS",
        "this is a workflow task, not a personal todo",
        None,
    )
    .unwrap();

    let personal = cp_list_personal_tasks(&conn).unwrap();
    assert!(
        personal.is_empty(),
        "handoff task leaked into personal tasks: {personal:?}"
    );
}
