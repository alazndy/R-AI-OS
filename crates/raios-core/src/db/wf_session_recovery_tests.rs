use super::*;

#[test]
fn ending_a_wrapper_preserves_an_explicit_terminal_task_decision() {
    let conn = crate::db::tests::in_memory();
    let (task, run) = cp_session_start(&conn, "codex_kaira", None).unwrap();
    conn.execute(
        "UPDATE cp_tasks SET status='completed' WHERE id=?1",
        [&task],
    )
    .unwrap();
    cp_session_end(&conn, &task, &run, false).unwrap();
    assert_eq!(
        conn.query_row("SELECT status FROM cp_tasks WHERE id=?1", [&task], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "completed"
    );
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_agent_runs WHERE id=?1",
            [&run],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "failed"
    );
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
}

#[test]
fn session_start_rolls_back_the_task_and_contract_when_run_insert_fails() {
    let conn = crate::db::tests::in_memory();
    conn.execute_batch("CREATE TRIGGER fail_run BEFORE INSERT ON cp_agent_runs BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(cp_session_start(&conn, "codex_kaira", None).is_err());
    for table in ["cp_tasks", "cp_run_contracts", "cp_agent_runs"] {
        let count: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn session_end_is_atomic_and_stale_completion_cannot_reopen_a_terminal_run() {
    let conn = crate::db::tests::in_memory();
    let (task, run) = cp_session_start(&conn, "codex_kaira", None).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_end BEFORE UPDATE ON cp_tasks BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(cp_session_end(&conn, &task, &run, true).is_err());
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_agent_runs WHERE id=?1",
            [&run],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "running"
    );
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 0);
    conn.execute_batch("DROP TRIGGER fail_end").unwrap();
    cp_session_end(&conn, &task, &run, false).unwrap();
    cp_session_end(&conn, &task, &run, true).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_agent_runs WHERE id=?1",
            [&run],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "failed"
    );
    assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
}

#[test]
fn explicit_workspace_binds_the_contract_and_rejects_an_unrelated_project() {
    let conn = crate::db::tests::in_memory();
    let project = upsert_project(
        &conn,
        "target",
        "core",
        "/workspace/target",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let (_, run) = cp_session_start_in_workspace(
        &conn,
        "codex_kaira",
        Some(project),
        "/workspace/target",
        None,
    )
    .unwrap();
    assert_eq!(
        cp_get_run_contract_for_agent_run(&conn, &run)
            .unwrap()
            .unwrap()
            .workspace_root,
        "/workspace/target"
    );
    assert!(cp_session_start_in_workspace(
        &conn,
        "codex_kaira",
        Some(project),
        "/workspace/another",
        None
    )
    .is_err());
    assert!(cp_session_start_in_workspace(&conn, "codex_kaira", None, "relative", None).is_err());
}

#[test]
fn process_attachment_is_owner_bound_and_cannot_replace_the_child() {
    let conn = crate::db::tests::in_memory();
    let owner = WrapperProcessIdentity {
        host_id: "host".into(),
        boot_id: "boot".into(),
        pid_namespace: "ns".into(),
        pid: 10,
        start_ticks: 1,
    };
    let (_, run) =
        cp_session_start_in_workspace(&conn, "codex_kaira", None, "/workspace", Some(&owner))
            .unwrap();
    let mut impostor = owner.clone();
    impostor.start_ticks = 2;
    assert!(cp_attach_wrapper_child(&conn, &run, &impostor, None).is_err());
    let mut child = owner.clone();
    child.pid = 11;
    cp_attach_wrapper_child(&conn, &run, &owner, Some(&child)).unwrap();
    assert!(cp_attach_wrapper_child(&conn, &run, &owner, None).is_err());
}
