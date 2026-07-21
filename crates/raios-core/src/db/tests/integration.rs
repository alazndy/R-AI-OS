use super::*;

// ── Integration: file approval lifecycle ────────────────────────────────────

#[test]
fn cp_flow_file_approval_lifecycle() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/proj/src/main.rs", "old", "new", "claude").unwrap();

    let task_status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_status, "awaiting_approval");

    let run_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_agent_runs WHERE task_id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(run_count, 1);

    let artifact_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_artifacts WHERE task_id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(artifact_count, 1);

    let approval_status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id=?1",
            params![ids.approval_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(approval_status, "pending");

    let contract_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_run_contracts WHERE task_id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(contract_count, 1);

    let approvals = cp_query_pending_approvals(&conn).unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].approval_type, "file_write");

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals SET status='approved', resolved_at=?1 WHERE id=?2",
        params![now, ids.approval_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE cp_tasks SET status='completed', updated_at=?1 WHERE id=?2",
        params![now, ids.task_id],
    )
    .unwrap();

    let after = cp_query_pending_approvals(&conn).unwrap();
    assert_eq!(after.len(), 0);

    let active = cp_query_active_tasks(&conn).unwrap();
    assert!(!active.iter().any(|t| t.id == ids.task_id));
}

// ── Integration: swarm lifecycle ────────────────────────────────────────────

#[test]
fn cp_flow_swarm_lifecycle() {
    let conn = in_memory();
    let ids = create_swarm_workflow(&conn, "/proj", "add dark mode", "claude").unwrap();

    let task_status: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_status, "queued");

    mark_swarm_workflow_running(&conn, &ids.task_id, &ids.agent_run_id).unwrap();
    let task_status_2: String = conn
        .query_row(
            "SELECT status FROM cp_tasks WHERE id=?1",
            params![ids.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_status_2, "running");

    let runs = cp_query_active_runs(&conn).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].provider, "swarm");
    assert_eq!(runs[0].task_id, ids.task_id);

    cp_record_run_failure(
        &conn,
        &ids.agent_run_id,
        &ProviderFailureKind::Timeout,
        "exceeded 3600s",
    )
    .unwrap();

    let exit_reason: String = conn
        .query_row(
            "SELECT exit_reason FROM cp_agent_runs WHERE id=?1",
            params![ids.agent_run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exit_reason, "timeout");

    let runs_after = cp_query_active_runs(&conn).unwrap();
    assert_eq!(runs_after.len(), 0);
}

// ── Integration: task graph lifecycle ───────────────────────────────────────

#[test]
fn cp_flow_task_graph_lifecycle() {
    let conn = in_memory();
    let graph_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,'goal','claude','pending',?2)",
        params![graph_id, now],
    )
    .unwrap();

    let ids_a = create_task_graph_node_workflow(
        &conn,
        &graph_id,
        "a",
        "build",
        "cargo build",
        "claude",
        true,
    )
    .unwrap();

    let ids_b = create_task_graph_node_workflow(
        &conn,
        &graph_id,
        "b",
        "test",
        "cargo test",
        "claude",
        false,
    )
    .unwrap();

    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, ids_a.task_id);
    assert_eq!(ready[0].execution_kind, "task_graph");

    let cmd = cp_task_graph_shell_cmd(&conn, &ids_a.task_id).unwrap();
    assert_eq!(cmd.as_deref(), Some("cargo build"));

    let completed_at = now.clone();
    conn.execute(
        "UPDATE cp_tasks SET status='completed', updated_at=?1 WHERE id=?2",
        params![completed_at, ids_a.task_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE cp_tasks SET status='ready', updated_at=?1 WHERE id=?2",
        params![now, ids_b.task_id],
    )
    .unwrap();

    let ready2 = cp_scheduler_list_ready(&conn).unwrap();
    assert_eq!(ready2.len(), 1);
    assert_eq!(ready2[0].id, ids_b.task_id);
}

// ── Integration: personal task lifecycle ────────────────────────────────────

#[test]
fn cp_flow_personal_task_lifecycle() {
    let conn = in_memory();

    let inputs = vec![
        PersonalTaskInput {
            id: None,
            title: "Write tests".into(),
            completed: false,
            agent: None,
            project_name: Some("RAIOS".into()),
            display_order: 0,
        },
        PersonalTaskInput {
            id: None,
            title: "Update docs".into(),
            completed: false,
            agent: Some("claude".into()),
            project_name: None,
            display_order: 1,
        },
    ];
    cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
    let rows = cp_list_personal_tasks(&conn).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].completed);

    let first_id = rows[0].id.clone();
    let updated = vec![
        PersonalTaskInput {
            id: Some(first_id.clone()),
            title: rows[0].title.clone(),
            completed: true,
            agent: rows[0].assignee_id.clone(),
            project_name: rows[0].project_name.clone(),
            display_order: 0,
        },
        PersonalTaskInput {
            id: Some(rows[1].id.clone()),
            title: rows[1].title.clone(),
            completed: false,
            agent: rows[1].assignee_id.clone(),
            project_name: rows[1].project_name.clone(),
            display_order: 1,
        },
    ];
    cp_sync_personal_tasks(&conn, &updated, "/dev_ops/tasks.md").unwrap();

    let after = cp_list_personal_tasks(&conn).unwrap();
    let first = after.iter().find(|r| r.id == first_id).unwrap();
    assert!(first.completed);

    let dir = tempfile::tempdir().unwrap();
    cp_rebuild_personal_markdown(&conn, dir.path()).unwrap();
    let content = std::fs::read_to_string(dir.path().join("tasks.md")).unwrap();
    assert!(content.contains("- [x]"));
    assert!(content.contains("- [ ]"));
}

// ── Integration: unified inbox view ────────────────────────────────────────

#[test]
fn cp_flow_unified_inbox() {
    let conn = in_memory();

    create_file_change_workflow(&conn, "/proj/a.rs", "old", "new", "claude").unwrap();
    create_swarm_workflow(&conn, "/proj", "dark mode", "claude").unwrap();
    cp_sync_personal_tasks(
        &conn,
        &[PersonalTaskInput {
            id: None,
            title: "Personal work".into(),
            completed: false,
            agent: None,
            project_name: None,
            display_order: 0,
        }],
        "/dev_ops/tasks.md",
    )
    .unwrap();

    let snapshot = cp_daemon_snapshot(&conn).unwrap();

    assert_eq!(snapshot.active_tasks.len(), 3);
    assert_eq!(snapshot.active_runs.len(), 2);
    assert_eq!(snapshot.pending_approvals.len(), 1);
    assert_eq!(snapshot.pending_approvals[0].approval_type, "file_write");
    assert_eq!(snapshot.blocked_tasks.len(), 0);

    let origins: Vec<&str> = snapshot
        .active_tasks
        .iter()
        .map(|t| t.origin.as_str())
        .collect();
    assert!(origins.contains(&"swarm"));
    assert!(origins.contains(&"file_approval"));
    assert!(origins.contains(&"personal"));
}

// ── Integration: budget deferral appears in snapshot ────────────────────────

#[test]
fn cp_snapshot_budget_deferral_visible() {
    let conn = in_memory();
    cp_upsert_budget_ledger(
        &conn,
        "provider",
        "claude",
        Some("claude"),
        "tokens",
        Some(10000.0),
        Some(10000.0),
        Some(0.0),
        "exact",
        "test",
    )
    .unwrap();
    let snap = cp_daemon_snapshot(&conn).unwrap();
    assert!(snap.budget_deferrals.contains(&"claude".to_string()));
}

// ── Integration: pending file-change approvals survive simulated restart ────

#[test]
fn cp_pending_file_change_approvals_hydrated_from_db() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/proj/src/lib.rs", "old code", "new code", "claude")
            .unwrap();

    let rows = cp_load_pending_file_change_approvals(&conn).unwrap();
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.approval_id, ids.approval_id);
    assert_eq!(row.path, "/proj/src/lib.rs");
    assert_eq!(row.original_content, "old code");
    assert_eq!(row.new_content, "new code");
    assert_eq!(row.agent_name, "claude");
    assert_eq!(row.task_id.as_deref(), Some(ids.task_id.as_str()));
    assert_eq!(row.agent_run_id.as_deref(), Some(ids.agent_run_id.as_str()));
    assert_eq!(row.artifact_id.as_deref(), Some(ids.artifact_id.as_str()));
}

#[test]
fn cp_pending_file_change_approvals_disappears_after_resolve() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/proj/src/main.rs", "v1", "v2", "claude").unwrap();

    assert_eq!(
        cp_load_pending_file_change_approvals(&conn).unwrap().len(),
        1
    );

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals SET status='approved', resolved_at=?1 WHERE id=?2",
        rusqlite::params![now, ids.approval_id],
    )
    .unwrap();

    assert_eq!(
        cp_load_pending_file_change_approvals(&conn).unwrap().len(),
        0
    );
}

#[test]
fn scheduled_job_create_sets_correct_next_run_at() {
    let conn = in_memory();
    let id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", 60).unwrap();
    let jobs = cp_scheduled_jobs_list(&conn).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, id);
    assert_eq!(jobs[0].title, "Test Job");
    assert_eq!(jobs[0].interval_secs, 60);
    assert_eq!(jobs[0].status, "active");
    assert!(jobs[0].next_run_at > jobs[0].created_at);
}

#[test]
fn scheduled_job_claim_due_is_atomic() {
    let conn = in_memory();
    let id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", -5).unwrap();

    let due_jobs = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert_eq!(due_jobs.len(), 1);
    assert_eq!(due_jobs[0].id, id);
    assert_eq!(due_jobs[0].status, "firing");

    let due_jobs_again = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert!(due_jobs_again.is_empty());
}

#[test]
fn scheduled_job_mark_fired_advances_next_run() {
    let conn = in_memory();
    let id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", 60).unwrap();

    conn.execute("UPDATE cp_scheduled_jobs SET next_run_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 seconds') WHERE id = ?1", params![id]).unwrap();

    let claimed = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert_eq!(claimed.len(), 1);

    cp_scheduled_job_mark_fired(&conn, &id, 60).unwrap();

    let jobs = cp_scheduled_jobs_list(&conn).unwrap();
    assert_eq!(jobs[0].status, "active");
    assert_eq!(jobs[0].run_count, 1);
    assert!(jobs[0].last_run_at.is_some());
}

#[test]
fn scheduled_job_revert_firing_backs_off_next_run() {
    let conn = in_memory();
    let id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", 60).unwrap();

    conn.execute("UPDATE cp_scheduled_jobs SET next_run_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 seconds') WHERE id = ?1", params![id]).unwrap();

    let claimed = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert_eq!(claimed.len(), 1);

    cp_scheduled_job_revert_firing(&conn, &id, 60).unwrap();

    let jobs = cp_scheduled_jobs_list(&conn).unwrap();
    assert_eq!(jobs[0].status, "active");
    assert_eq!(jobs[0].run_count, 0);
    assert!(jobs[0].last_run_at.is_none());
    assert!(
        jobs[0].next_run_at > jobs[0].created_at,
        "a failed spawn must back off next_run_at instead of leaving it due, \
         or the scheduler retries it every tick forever"
    );

    // Immediately re-checking due jobs must NOT reclaim it — this is the
    // exact retry-storm the bug caused (JSON Backup job hammered every
    // ~60s for 3 days instead of respecting its 86400s interval).
    let due_again = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert!(due_again.is_empty());
}

#[test]
fn scheduled_jobs_reset_firing_on_restart() {
    let conn = in_memory();
    let _id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", -5).unwrap();

    let claimed = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert_eq!(claimed[0].status, "firing");

    cp_scheduled_jobs_reset_firing(&conn).unwrap();

    let jobs = cp_scheduled_jobs_list(&conn).unwrap();
    assert_eq!(jobs[0].status, "active");
}
