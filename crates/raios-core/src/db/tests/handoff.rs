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
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: "skeleton ready, implement auth handlers",
            diff_stat: Some(" src/db.rs | 12 ++++++++----"),
            report: None,
        },
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
        HandoffWorkflowInput {
            project_path: "/tmp/unknown-project",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "BLOCKER",
            msg: "stuck on flaky test, needs investigation",
            diff_stat: None,
            report: None,
        },
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
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "BLOCKER",
            msg: "stale note nobody picked up",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let fresh = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: "fresh note replaces the stale one",
            diff_stat: None,
            report: None,
        },
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
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "this is a workflow task, not a personal todo",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    let personal = cp_list_personal_tasks(&conn).unwrap();
    assert!(
        personal.is_empty(),
        "handoff task leaked into personal tasks: {personal:?}"
    );
}

#[test]
fn handoff_report_round_trips_through_inbox_query() {
    let conn = in_memory();
    let project_id = upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();

    let report = HandoffReport {
        findings: "auth handlers wired, budget gate untested".into(),
        evidence: vec!["cargo test wf_handoff::tests -- --nocapture passed".into()],
        edge_cases_considered: vec!["empty msg".into(), "concurrent handoff to same agent".into()],
        open_questions: vec!["should budget gate reject or warn on soft cap?".into()],
        confidence: 0.8,
        what_i_did_not_check: vec!["MCP surface wiring".into()],
    };

    create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: &report.findings,
            diff_stat: None,
            report: Some(&report),
        },
    )
    .unwrap();

    // Read back via the exact query the TUI inbox panel (`load_inbox_panel_data`)
    // and the MCP `get_inbox` tool both use.
    let scored = cp_query_pending_approvals_scored(&conn).unwrap();
    let handover = scored
        .iter()
        .find(|s| s.approval.approval_type == "handover")
        .expect("handover approval should be pending");
    let round_tripped = handover
        .handoff_report
        .as_ref()
        .expect("structured report should round-trip through metadata_json");
    assert_eq!(round_tripped, &report);

    // The delivery path (`cp_take_pending_handoff`, consumed by `agent_runner.rs`)
    // must also surface the same structured report.
    let ctx = cp_take_pending_handoff(&conn, Some(project_id), "opencode_kaira")
        .unwrap()
        .expect("handoff should be pending for the assignee");
    assert_eq!(ctx.report.as_ref(), Some(&report));
}

#[test]
fn legacy_free_text_handoff_has_no_structured_report() {
    let conn = in_memory();
    create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: "plain free-text handoff, no --report given",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    let scored = cp_query_pending_approvals_scored(&conn).unwrap();
    let handover = scored
        .iter()
        .find(|s| s.approval.approval_type == "handover")
        .expect("handover approval should be pending");
    assert!(
        handover.handoff_report.is_none(),
        "legacy free-text handoff must not fabricate a structured report"
    );
}

#[test]
fn verify_gate_blocks_handoff_success_until_completed() {
    let conn = in_memory();
    let _project_id = upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();

    // Create a task graph with a verify gate node
    let graph_id = "g-1";
    conn.execute(
        "INSERT INTO task_graphs (id, goal, agent, status) VALUES (?1, 'goal', 'agent', 'pending')",
        params![graph_id],
    )
    .unwrap();

    let node_ids = insert_verify_gate_node(
        &conn,
        graph_id,
        "v1",
        "run cargo test",
        "cargo test",
        "claude_kaira",
        true,
    )
    .unwrap();

    // Handoff with status = SUCCESS while verify_gate is unpassed -> expect error
    let res = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: "work complete",
            diff_stat: None,
            report: None,
        },
    );
    assert!(res.is_err(), "handoff SUCCESS must be blocked by unpassed verify gate");
    assert!(res.unwrap_err().to_string().contains("verify_gate node 'v1' has not passed yet"));

    // Handoff with status = BLOCKER is NOT blocked by verify gate
    let res_blocker = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "BLOCKER",
            msg: "need help",
            diff_stat: None,
            report: None,
        },
    );
    assert!(res_blocker.is_ok(), "non-SUCCESS handoffs are not blocked by verify gate");

    // Complete the verify_gate task
    mark_control_task_completed(&conn, &node_ids.task_id, &node_ids.agent_run_id, "test passed").unwrap();

    // Handoff SUCCESS now succeeds
    let res_after = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "opencode_kaira",
            status: "SUCCESS",
            msg: "work complete after gate passed",
            diff_stat: None,
            report: None,
        },
    );
    assert!(res_after.is_ok(), "handoff SUCCESS must pass once verify gate is completed");
}
