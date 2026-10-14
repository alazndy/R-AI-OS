use super::*;

#[test]
fn named_sessions_can_be_tracked_and_relabelled_without_changing_execution_state() {
    let conn = crate::db::tests::in_memory();
    let (task, run) = cp_session_start_named(
        &conn,
        "codex_kaira",
        None,
        "/workspace",
        None,
        Some("  Database maintenance  "),
        Some("Investigate cache growth"),
    )
    .unwrap();
    let rows = cp_sessions_list(&conn, 10).unwrap();
    assert_eq!(rows[0].name, "Database maintenance");
    assert_eq!(rows[0].work, "Investigate cache growth");
    assert_eq!(rows[0].task_id, task);
    cp_label_wrapper_session(&conn, &run, None, Some("Verify the backup")).unwrap();
    cp_session_end(&conn, &task, &run, true).unwrap();
    cp_label_wrapper_session(&conn, &run, Some("Maintenance complete"), None).unwrap();
    let rows = cp_sessions_list(&conn, 10).unwrap();
    assert_eq!(rows[0].name, "Maintenance complete");
    assert_eq!(rows[0].work, "Verify the backup");
    assert_eq!(rows[0].status, "succeeded");
    assert_eq!(rows[0].task_status, "completed");
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 3);
    assert!(cp_label_wrapper_session(&conn, "missing", Some("Oops"), None).is_err());
}

#[test]
fn session_filters_are_applied_before_limit_and_search_is_literal() {
    let conn = crate::db::tests::in_memory();
    let (task, run) = cp_session_start_named(
        &conn,
        "codex_kaira",
        None,
        "/workspace",
        None,
        Some("100% backup"),
        Some("SQLite work"),
    )
    .unwrap();
    let (_, other) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    conn.execute(
        "UPDATE cp_agent_runs SET started_at='2099-01-01 00:00:00' WHERE id=?1",
        [other],
    )
    .unwrap();
    let rows =
        cp_sessions_list_filtered(&conn, 1, Some("codex_kaira"), true, None, Some("100%")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].run_id, run);
    assert!(
        cp_sessions_list_filtered(&conn, 1, None, false, None, Some("_wildcard_"))
            .unwrap()
            .is_empty()
    );
    cp_session_end(&conn, &task, &run, true).unwrap();
    assert!(
        cp_sessions_list_filtered(&conn, 1, Some("codex_kaira"), true, None, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn label_audit_failure_rolls_back_and_invalid_labels_leave_no_partial_launch() {
    let conn = crate::db::tests::in_memory();
    let (_, run) = cp_session_start(&conn, "codex_kaira", None).unwrap();
    conn.execute_batch("DROP TABLE audit_log").unwrap();
    assert!(cp_label_wrapper_session(&conn, &run, Some("Changed"), None).is_err());
    assert_eq!(
        cp_sessions_list(&conn, 1).unwrap()[0].name,
        "Session: codex_kaira"
    );
    for name in ["", " ", "unsafe\x1b[2J", "password = 'superSecretValue123'"] {
        assert!(cp_session_start_named(
            &conn,
            "codex_kaira",
            None,
            "/workspace",
            None,
            Some(name),
            None
        )
        .is_err());
    }
    assert!(validate_session_labels(Some(&"x".repeat(121)), None).is_err());
    assert!(validate_session_labels(None, Some(&"x".repeat(2001))).is_err());
    assert_eq!(cp_sessions_list(&conn, 10).unwrap().len(), 1);
}

#[test]
fn project_filter_reports_registered_project_and_preserves_legacy_labels() {
    let conn = crate::db::tests::in_memory();
    let pid = upsert_project(
        &conn,
        "Tracked",
        "core",
        "/workspace/project",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let (_, run) = cp_session_start(&conn, "codex_kaira", Some(pid)).unwrap();
    cp_session_start(&conn, "codex_kaira", None).unwrap();
    for project in ["Tracked", "/workspace/project"] {
        let rows = cp_sessions_list_filtered(&conn, 10, None, false, Some(project), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, run);
        assert_eq!(rows[0].project_name.as_deref(), Some("Tracked"));
        assert_eq!(rows[0].name, "Session: codex_kaira");
    }
}
