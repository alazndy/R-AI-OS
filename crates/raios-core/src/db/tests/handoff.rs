use super::*;

#[test]
fn successful_return_closes_incoming_only_in_the_same_workspace() {
    let conn = in_memory();
    let queue = |path, from, to, status| {
        create_handoff_workflow(
            &conn,
            HandoffWorkflowInput {
                project_path: path,
                from_agent: from,
                to_agent: to,
                status,
                msg: "work",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap()
    };
    let incoming = queue("/repo", "claude_kaira", "codex_kaira", "success");
    queue("/other", "claude_kaira", "codex_kaira", "success");
    queue("/repo", "codex_kaira", "claude_kaira", "blocker");
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
            .unwrap()
            .is_some()
    );
    let outgoing = queue("/repo", "codex_kaira", "claude_kaira", "success");
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
            .unwrap()
            .is_none()
    );
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/other", "codex_kaira")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "claude_kaira")
            .unwrap()
            .unwrap()
            .task_id,
        outgoing.task_id
    );
    assert!(!cp_query_active_tasks(&conn)
        .unwrap()
        .iter()
        .any(|t| t.id == incoming.task_id));
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
}

#[test]
fn stale_delivery_cannot_resurrect_a_rejected_handoff() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "old work",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let ctx = cp_take_pending_handoff(&conn, None, "codex_kaira")
        .unwrap()
        .unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    assert!(cp_resolve_handoff(
        &tx,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner",
        Some("Already handled elsewhere")
    )
    .unwrap());
    tx.commit().unwrap();
    cp_consume_handoff(&conn, &ctx, "codex_kaira").unwrap();
    assert!(cp_query_active_tasks(&conn).unwrap().is_empty());
    assert!(cp_query_active_runs(&conn).unwrap().is_empty());
    let status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![ids.approval_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "rejected");
}

#[test]
fn delivery_is_atomic_and_audited_and_bound_to_receiver() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "review",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let ctx = cp_take_pending_handoff(&conn, None, "codex_kaira")
        .unwrap()
        .unwrap();
    cp_consume_handoff(&conn, &ctx, "opencode_kaira").unwrap();
    assert!(cp_take_pending_handoff(&conn, None, "codex_kaira")
        .unwrap()
        .is_some());
    conn.execute_batch("CREATE TRIGGER fail_handoff_task BEFORE UPDATE ON cp_tasks BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
    assert!(cp_consume_handoff(&conn, &ctx, "codex_kaira").is_err());
    let states: (String, String, String, String) = conn
        .query_row(
            "SELECT ap.status, a.status, r.status, t.status FROM cp_approvals ap
         JOIN cp_artifacts a ON a.id = ap.artifact_id JOIN cp_agent_runs r ON r.id = ap.agent_run_id
         JOIN cp_tasks t ON t.id = ap.task_id WHERE ap.id = ?1",
            params![ids.approval_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        states,
        (
            "pending".into(),
            "submitted".into(),
            "awaiting_approval".into(),
            "awaiting_approval".into()
        )
    );
    conn.execute_batch("DROP TRIGGER fail_handoff_task;")
        .unwrap();
    cp_consume_handoff(&conn, &ctx, "codex_kaira").unwrap();
    cp_consume_handoff(&conn, &ctx, "codex_kaira").unwrap();
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
    assert!(cp_query_active_tasks(&conn).unwrap().is_empty());
    assert!(cp_query_active_runs(&conn).unwrap().is_empty());
}

#[test]
fn superseding_handoff_preserves_unrelated_awaiting_task() {
    let conn = in_memory();
    let project_id = upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();
    conn.execute("INSERT INTO cp_tasks (id, project_id, title, description, status, assignee_id, created_at, updated_at) VALUES ('other-workflow', ?1, 'Unrelated approval', '', 'awaiting_approval', 'codex_kaira', '2026-10-10', '2026-10-10')", params![project_id]).unwrap();
    create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "review",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id = 'other-workflow'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "awaiting_approval");
}

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
    assert_eq!(
        pending.context_summary,
        "skeleton ready, implement auth handlers"
    );
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

    let for_wrong_agent = cp_take_pending_handoff(&conn, None, "antigravity_kaira").unwrap();
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
        edge_cases_considered: vec![
            "empty msg".into(),
            "concurrent handoff to same agent".into(),
        ],
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
    assert!(
        res.is_err(),
        "handoff SUCCESS must be blocked by unpassed verify gate"
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("verify_gate node 'v1' has not passed yet"));

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
    assert!(
        res_blocker.is_ok(),
        "non-SUCCESS handoffs are not blocked by verify gate"
    );

    // Complete the verify_gate task
    mark_control_task_completed(
        &conn,
        &node_ids.task_id,
        &node_ids.agent_run_id,
        "test passed",
    )
    .unwrap();

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
    assert!(
        res_after.is_ok(),
        "handoff SUCCESS must pass once verify gate is completed"
    );
}

#[test]
fn user_handoff_management_closes_queue_preserves_history_and_is_idempotent() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "Ready to handle",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    cp_manage_handoff(&conn, &ids.approval_id, false).unwrap();
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cp_list_handoffs(&conn, false, None, 30).unwrap()[0]
            .user_status
            .as_deref(),
        Some("completed")
    );
    cp_manage_handoff(&conn, &ids.approval_id, false).unwrap();
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
    cp_manage_handoff(&conn, &ids.approval_id, true).unwrap();
    assert!(cp_list_handoffs(&conn, false, None, 30).unwrap().is_empty());
    assert_eq!(
        cp_list_handoffs(&conn, true, Some("/repo"), 30).unwrap()[0]
            .user_status
            .as_deref(),
        Some("removed")
    );
    assert!(cp_manage_handoff(&conn, &ids.approval_id, false).is_err());
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 2);
}

#[test]
fn user_handoff_removal_blocks_stale_delivery_and_audit_failure_rolls_back() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "SUCCESS",
            msg: "Queued",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let stale = cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
        .unwrap()
        .unwrap();
    conn.execute_batch("CREATE TRIGGER refuse_handoff_audit BEFORE INSERT ON audit_log BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;").unwrap();
    assert!(cp_manage_handoff(&conn, &ids.approval_id, true).is_err());
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
            .unwrap()
            .is_some()
    );
    conn.execute_batch("DROP TRIGGER refuse_handoff_audit")
        .unwrap();
    cp_manage_handoff(&conn, &ids.approval_id, true).unwrap();
    cp_consume_handoff(&conn, &stale, "codex_kaira").unwrap();
    let row = cp_list_handoffs(&conn, true, None, 30).unwrap().remove(0);
    assert_eq!(row.delivery_status, "rejected");
    assert_eq!(row.user_status.as_deref(), Some("removed"));
}

#[test]
fn approval_revisions_move_with_every_transition() {
    let conn = in_memory();
    let queue = |path, from, to, status| {
        create_handoff_workflow(
            &conn,
            HandoffWorkflowInput {
                project_path: path,
                from_agent: from,
                to_agent: to,
                status,
                msg: "work",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap()
    };

    let ids = queue("/repo", "claude_kaira", "codex_kaira", "BLOCKER");
    assert_eq!(
        cp_approval_revision(&conn, &ids.approval_id).unwrap(),
        Some(1)
    );
    assert_eq!(
        cp_approval_revision(&conn, "missing-approval").unwrap(),
        None
    );

    // Resolution moves the revision, so a decision made on revision 1 is refused afterwards.
    assert!(cp_resolve_handoff(
        &conn,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner",
        None
    )
    .unwrap());
    assert_eq!(
        cp_approval_revision(&conn, &ids.approval_id).unwrap(),
        Some(2)
    );

    // Superseding an older pending handoff moves its revision too.
    let first = queue("/repo-b", "claude_kaira", "codex_kaira", "BLOCKER");
    let _second = queue("/repo-b", "claude_kaira", "codex_kaira", "BLOCKER");
    assert_eq!(
        cp_approval_revision(&conn, &first.approval_id).unwrap(),
        Some(2)
    );
}

#[test]
fn owner_approval_is_not_delivery_and_the_wrapper_still_delivers() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "BLOCKER",
            msg: "Queued",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    assert!(cp_approve_handoff_by_owner(
        &conn,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner"
    )
    .unwrap());

    // Consent closes the queue but not the task: the wrapper still takes the handoff and
    // knows it was owner-approved, so it delivers without asking again.
    let ctx = cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
        .unwrap()
        .expect("owner-approved handoff is still deliverable");
    assert!(ctx.approved_by_owner);
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            rusqlite::params![ids.task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "awaiting_approval"
    );
    assert_eq!(
        conn.query_row(
            "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
            rusqlite::params![ids.agent_run_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "handover_approved_by_owner"
    );

    // A stranger cannot mark the delivery, and a resolved approval cannot be approved again.
    assert!(!cp_mark_handoff_delivered(&conn, &ids.approval_id, "claude_kaira").unwrap());
    assert!(!cp_approve_handoff_by_owner(
        &conn,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner"
    )
    .unwrap());

    cp_consume_handoff(&conn, &ctx, "codex_kaira").unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            rusqlite::params![ids.task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "completed"
    );
    // The run's reason stays what it was: the owner's approval ended the wait; delivery is
    // recorded on the approval, artifact, task and audit — not by re-labelling the run.
    assert_eq!(
        conn.query_row(
            "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
            rusqlite::params![ids.agent_run_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "handover_approved_by_owner"
    );
    assert!(
        cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
            .unwrap()
            .is_none()
    );
    let receipt = cp_handoff_receipt(&conn, &ids.approval_id, "/repo")
        .unwrap()
        .expect("receipt");
    assert!(receipt.delivered_by_target());
    assert_eq!(receipt.resolved_by.as_deref(), Some("codex_kaira"));
}

#[test]
fn a_newer_handoff_supersedes_an_owner_approved_one_that_was_never_delivered() {
    let conn = in_memory();
    let queue = |msg: &str| {
        create_handoff_workflow(
            &conn,
            HandoffWorkflowInput {
                project_path: "/repo",
                from_agent: "claude_kaira",
                to_agent: "codex_kaira",
                status: "BLOCKER",
                msg,
                diff_stat: None,
                report: None,
            },
        )
        .unwrap()
    };
    let approved = queue("First");
    assert!(cp_approve_handoff_by_owner(
        &conn,
        &approved.approval_id,
        "local_control_owner",
        "local_control_owner"
    )
    .unwrap());

    let _newer = queue("Second");

    // The old note must not deliver later: it is expired, its task cancelled, and the
    // newer handoff is the one waiting for the wrapper.
    let ctx = cp_take_pending_handoff_for_path(&conn, "/repo", "codex_kaira")
        .unwrap()
        .expect("newer handoff is deliverable");
    assert!(!ctx.approved_by_owner);
    assert!(!cp_mark_handoff_delivered(&conn, &approved.approval_id, "codex_kaira").unwrap());
    let receipt = cp_handoff_receipt(&conn, &approved.approval_id, "/repo")
        .unwrap()
        .expect("receipt");
    assert_eq!(receipt.status, "expired");
    assert!(!receipt.delivered_by_target());
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            rusqlite::params![approved.task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "cancelled"
    );
}

#[test]
fn a_user_completed_handoff_is_never_superseded() {
    let conn = in_memory();
    let queue = |msg: &str| {
        create_handoff_workflow(
            &conn,
            HandoffWorkflowInput {
                project_path: "/repo",
                from_agent: "claude_kaira",
                to_agent: "codex_kaira",
                status: "BLOCKER",
                msg,
                diff_stat: None,
                report: None,
            },
        )
        .unwrap()
    };
    let completed = queue("First");
    cp_manage_handoff(&conn, &completed.approval_id, false).unwrap();

    let _newer = queue("Second");

    // The user closed this handoff explicitly; a later handoff must not rewrite that.
    let receipt = cp_handoff_receipt(&conn, &completed.approval_id, "/repo")
        .unwrap()
        .expect("receipt");
    assert_eq!(receipt.status, "approved");
    assert_eq!(receipt.resolved_by.as_deref(), Some("local_user"));
}

#[test]
fn pending_approvals_carry_project_agents_and_revision() {
    let conn = in_memory();
    let ids = create_handoff_workflow(
        &conn,
        HandoffWorkflowInput {
            project_path: "/repo-approvals",
            from_agent: "claude_kaira",
            to_agent: "codex_kaira",
            status: "BLOCKER",
            msg: "Carry context",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    for row in [
        cp_query_pending_approvals(&conn).unwrap().remove(0),
        cp_query_pending_approvals_scored(&conn)
            .unwrap()
            .remove(0)
            .approval,
    ] {
        assert_eq!(row.id, ids.approval_id);
        assert_eq!(row.project_path.as_deref(), Some("/repo-approvals"));
        assert_eq!(row.origin_agent.as_deref(), Some("claude_kaira"));
        assert_eq!(row.target_agent.as_deref(), Some("codex_kaira"));
        assert_eq!(row.revision, 1);
    }

    // The revision read here is what a surface sends back with its decision.
    assert!(cp_approve_handoff_by_owner(
        &conn,
        &ids.approval_id,
        "local_control_owner",
        "local_control_owner"
    )
    .unwrap());
    assert_eq!(
        cp_approval_revision(&conn, &ids.approval_id).unwrap(),
        Some(2)
    );
}
