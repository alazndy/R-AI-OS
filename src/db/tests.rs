use super::*;
use rusqlite::Connection;


fn in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    migrate_existing(&conn).unwrap();
    conn
}

#[test]
fn sqlite_open_and_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn project_crud_round_trip() {
    let conn = in_memory();
    upsert_project(
        &conn,
        "TestProj",
        "devtools",
        "/tmp/test",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let projects = load_all_projects(&conn).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "TestProj");
}

#[test]
fn upsert_is_idempotent() {
    let conn = in_memory();
    upsert_project(
        &conn,
        "P",
        "cat",
        "/tmp/p",
        Some("gh/p"),
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    upsert_project(
        &conn,
        "P-renamed",
        "cat",
        "/tmp/p",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let projects = load_all_projects(&conn).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "P-renamed");
    assert_eq!(projects[0].github.as_deref(), Some("gh/p")); // preserved
}

#[test]
fn health_cache_upsert() {
    let conn = in_memory();
    upsert_project(
        &conn, "P", "c", "/tmp/p", None, "active", None, None, None, None,
    )
    .unwrap();
    let id = project_id_for_path(&conn, "/tmp/p").unwrap();
    upsert_health(
        &conn,
        id,
        "A",
        Some(90),
        Some("A"),
        Some(95),
        0,
        0,
        false,
        true,
        true,
        None,
        "A",
        95,
        0,
    )
    .unwrap();
    let stats = query_stats(&conn).unwrap();
    assert_eq!(stats.grade_a, 1);
}

#[test]
fn task_insert_and_toggle() {
    let conn = in_memory();
    let id = insert_task(&conn, "Fix bug", Some("claude"), Some("RAIOS")).unwrap();
    toggle_task(&conn, id, true).unwrap();
    let tasks = load_tasks_db(&conn).unwrap();
    assert!(tasks[0].completed);
}

#[test]
fn cortex_table_exists_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cortex_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn bm25_tables_exist_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn sessions_table_exists_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn instinct_candidates_table_exists() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instinct_candidates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn task_graph_tables_exist() {
    let conn = in_memory();
    let count_graphs: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_graphs", [], |r| r.get(0))
        .unwrap();
    let count_nodes: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_graph_nodes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_graphs, 0);
    assert_eq!(count_nodes, 0);
}

#[test]
fn swarm_tasks_table_exists() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM swarm_tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn control_plane_tables_exist() {
    let conn = in_memory();
    let count_tasks: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_tasks", [], |r| r.get(0))
        .unwrap();
    let count_runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_agent_runs", [], |r| r.get(0))
        .unwrap();
    let count_artifacts: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_artifacts", [], |r| r.get(0))
        .unwrap();
    let count_approvals: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_approvals", [], |r| r.get(0))
        .unwrap();
    let count_edges: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_task_edges", [], |r| r.get(0))
        .unwrap();
    let count_graph_nodes: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_task_graph_nodes", [], |r| r.get(0))
        .unwrap();
    let count_graphs: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_task_graphs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_tasks, 0);
    assert_eq!(count_runs, 0);
    assert_eq!(count_artifacts, 0);
    assert_eq!(count_approvals, 0);
    assert_eq!(count_edges, 0);
    assert_eq!(count_graph_nodes, 0);
    assert_eq!(count_graphs, 0);
}

#[test]
fn file_change_workflow_round_trip_applied() {
    let conn = in_memory();
    upsert_project(
        &conn, "RAIOS", "kernel", "/repo", None, "active", None, None, None, None,
    )
    .unwrap();

    let ids = create_file_change_workflow(&conn, "/repo/src/main.rs", "old", "new", "claude")
        .unwrap();
    mark_file_change_workflow_applied(&conn, &ids, "human").unwrap();

    let approval_status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![ids.approval_id],
            |row| row.get(0),
        )
        .unwrap();
    let artifact_status: String = conn
        .query_row(
            "SELECT status FROM cp_artifacts WHERE id = ?1",
            params![ids.artifact_id],
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
    assert_eq!(artifact_status, "applied");
    assert_eq!(task_status, "completed");
}

#[test]
fn file_change_workflow_round_trip_rejected() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/tmp/notes.md", "old", "new", "claude").unwrap();
    mark_file_change_workflow_rejected(&conn, &ids, "human", "rejected_by_user").unwrap();

    let run_status: String = conn
        .query_row(
            "SELECT status FROM cp_agent_runs WHERE id = ?1",
            params![ids.agent_run_id],
            |row| row.get(0),
        )
        .unwrap();
    let artifact_status: String = conn
        .query_row(
            "SELECT status FROM cp_artifacts WHERE id = ?1",
            params![ids.artifact_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(run_status, "failed");
    assert_eq!(artifact_status, "rejected");
}

// ── Agent handoff workflow tests ──────────────────────────────────────────

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

    // Delivered exactly once: a second take must find nothing left pending.
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

    // Only the fresh handoff should be deliverable.
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

    // Same NULL plan_id/parent_task_id shape as a real personal task, but it carries
    // a cp_approvals row — that's what must exclude it from the sidebar checklist.
    let personal = cp_list_personal_tasks(&conn).unwrap();
    assert!(
        personal.is_empty(),
        "handoff task leaked into personal tasks: {personal:?}"
    );
}

// ── Canonical personal task tests ─────────────────────────────────────────

#[test]
fn cp_personal_tasks_insert_list_round_trip() {
    let conn = in_memory();
    let inputs = vec![
        PersonalTaskInput {
            id: None,
            title: "Write tests".into(),
            completed: false,
            agent: Some("claude".into()),
            project_name: Some("RAIOS".into()),
            display_order: 0,
        },
        PersonalTaskInput {
            id: None,
            title: "Deploy".into(),
            completed: true,
            agent: None,
            project_name: None,
            display_order: 1,
        },
    ];
    cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
    let rows = cp_list_personal_tasks(&conn).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].title, "Write tests");
    assert!(!rows[0].completed);
    assert_eq!(rows[0].assignee_id.as_deref(), Some("claude"));
    assert_eq!(rows[0].project_name.as_deref(), Some("RAIOS"));
    assert_eq!(rows[1].title, "Deploy");
    assert!(rows[1].completed);
}

#[test]
fn cp_personal_tasks_cancel_on_sync() {
    let conn = in_memory();
    let inputs = vec![
        PersonalTaskInput {
            id: None,
            title: "Task A".into(),
            completed: false,
            agent: None,
            project_name: None,
            display_order: 0,
        },
        PersonalTaskInput {
            id: None,
            title: "Task B".into(),
            completed: false,
            agent: None,
            project_name: None,
            display_order: 1,
        },
    ];
    cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
    assert_eq!(cp_list_personal_tasks(&conn).unwrap().len(), 2);

    // Re-sync with only Task A — Task B should be cancelled
    let keep_one = vec![PersonalTaskInput {
        id: None,
        title: "Task A".into(),
        completed: false,
        agent: None,
        project_name: None,
        display_order: 0,
    }];
    cp_sync_personal_tasks(&conn, &keep_one, "/dev_ops/tasks.md").unwrap();
    let rows = cp_list_personal_tasks(&conn).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Task A");

    let cancelled_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_tasks WHERE status='cancelled' AND plan_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cancelled_count, 1);
}

#[test]
fn cp_personal_tasks_title_dedup() {
    let conn = in_memory();
    let input = vec![PersonalTaskInput {
        id: None,
        title: "Do something".into(),
        completed: false,
        agent: None,
        project_name: None,
        display_order: 0,
    }];
    cp_sync_personal_tasks(&conn, &input, "/dev_ops/tasks.md").unwrap();
    cp_sync_personal_tasks(&conn, &input, "/dev_ops/tasks.md").unwrap();
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_tasks WHERE plan_id IS NULL AND title='Do something'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(total, 1);
}

#[test]
fn cp_rebuild_personal_markdown_creates_file() {
    let conn = in_memory();
    let dir = tempfile::tempdir().unwrap();
    let inputs = vec![
        PersonalTaskInput {
            id: None,
            title: "First task".into(),
            completed: false,
            agent: Some("claude".into()),
            project_name: None,
            display_order: 0,
        },
        PersonalTaskInput {
            id: None,
            title: "Second task".into(),
            completed: true,
            agent: None,
            project_name: Some("PROJ".into()),
            display_order: 1,
        },
    ];
    cp_sync_personal_tasks(&conn, &inputs, "/dev_ops/tasks.md").unwrap();
    cp_rebuild_personal_markdown(&conn, dir.path()).unwrap();
    let content = std::fs::read_to_string(dir.path().join("tasks.md")).unwrap();
    assert!(content.contains("- [ ] First task @claude"));
    assert!(content.contains("- [x] Second task #PROJ"));
}

#[test]
fn cp_provider_capabilities_upsert_and_get() {
    let conn = in_memory();
    let caps = ProviderCapabilities {
        provider: "claude".into(),
        supports_tool_calling: true,
        supports_patch_diff: true,
        supports_long_running: true,
        supports_streaming: true,
        supports_exact_quota_visibility: false,
    };
    cp_upsert_provider_capabilities(&conn, &caps).unwrap();
    let got = cp_get_provider_capabilities(&conn, "claude").unwrap().unwrap();
    assert!(got.supports_tool_calling);
    assert!(got.supports_patch_diff);
    assert!(!got.supports_exact_quota_visibility);
    assert_eq!(got.provider, "claude");
}

#[test]
fn cp_provider_capabilities_seed_and_list() {
    let conn = in_memory();
    cp_seed_provider_capabilities(&conn).unwrap();
    let all = cp_list_provider_capabilities(&conn).unwrap();
    assert!(all.len() >= 4);
    let names: Vec<&str> = all.iter().map(|c| c.provider.as_str()).collect();
    assert!(names.contains(&"claude"));
    assert!(names.contains(&"shell"));
}

#[test]
fn cp_provider_capabilities_seed_no_overwrite() {
    let conn = in_memory();
    // First seed
    cp_seed_provider_capabilities(&conn).unwrap();
    // Manually override
    conn.execute(
        "UPDATE cp_provider_capabilities SET supports_tool_calling=0 WHERE provider='claude'",
        [],
    )
    .unwrap();
    // Re-seed should NOT overwrite
    cp_seed_provider_capabilities(&conn).unwrap();
    let got = cp_get_provider_capabilities(&conn, "claude").unwrap().unwrap();
    assert!(!got.supports_tool_calling, "seed should not overwrite existing rows");
}

#[test]
fn cp_failure_kind_classify() {
    assert_eq!(ProviderFailureKind::classify("401 unauthorized"), ProviderFailureKind::Auth);
    assert_eq!(ProviderFailureKind::classify("rate limit exceeded 429"), ProviderFailureKind::Quota);
    assert_eq!(ProviderFailureKind::classify("connection timed out"), ProviderFailureKind::Timeout);
    assert_eq!(ProviderFailureKind::classify("sandbox permission denied"), ProviderFailureKind::Sandbox);
    assert_eq!(ProviderFailureKind::classify("invalid tool call argument"), ProviderFailureKind::ToolError);
    assert_eq!(ProviderFailureKind::classify("service unavailable 503"), ProviderFailureKind::ProviderUnavailable);
    assert!(matches!(ProviderFailureKind::classify("some weird error"), ProviderFailureKind::Unknown(_)));
}

#[test]
fn cp_failure_kind_round_trip() {
    let kinds = [
        ProviderFailureKind::Auth,
        ProviderFailureKind::Quota,
        ProviderFailureKind::Timeout,
        ProviderFailureKind::Sandbox,
        ProviderFailureKind::ToolError,
        ProviderFailureKind::HumanRejection,
        ProviderFailureKind::ProviderUnavailable,
    ];
    for kind in &kinds {
        assert_eq!(&ProviderFailureKind::from_stored(kind.as_str()), kind);
    }
}

#[test]
fn cp_route_to_capable_provider_prefers_claude() {
    let conn = in_memory();
    cp_seed_provider_capabilities(&conn).unwrap();
    let best = cp_route_to_capable_provider(&conn, true, false, false).unwrap();
    assert_eq!(best, Some("claude".to_string()));
}

#[test]
fn cp_route_to_capable_provider_no_match() {
    let conn = in_memory();
    // Register only a provider that lacks tool calling
    cp_upsert_provider_capabilities(
        &conn,
        &ProviderCapabilities {
            provider: "basic".into(),
            supports_tool_calling: false,
            supports_patch_diff: false,
            supports_long_running: false,
            supports_streaming: false,
            supports_exact_quota_visibility: false,
        },
    )
    .unwrap();
    let best = cp_route_to_capable_provider(&conn, true, false, false).unwrap();
    assert_eq!(best, None);
}

// ── Integration: file approval lifecycle ────────────────────────────────────

#[test]
fn cp_flow_file_approval_lifecycle() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/proj/src/main.rs", "old", "new", "claude").unwrap();

    // Rows created in all four tables
    let task_status: String = conn
        .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(task_status, "awaiting_approval");

    let run_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_agent_runs WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(run_count, 1);

    let artifact_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_artifacts WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(artifact_count, 1);

    let approval_status: String = conn
        .query_row("SELECT status FROM cp_approvals WHERE id=?1", params![ids.approval_id], |r| r.get(0))
        .unwrap();
    assert_eq!(approval_status, "pending");

    // Run contract was persisted
    let contract_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cp_run_contracts WHERE task_id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(contract_count, 1);

    // Inbox query sees the pending approval
    let approvals = cp_query_pending_approvals(&conn).unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].approval_type, "file_write");

    // Approve: resolve the approval and mark task completed
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

    // No more pending approvals
    let after = cp_query_pending_approvals(&conn).unwrap();
    assert_eq!(after.len(), 0);

    // Active tasks no longer includes the completed task
    let active = cp_query_active_tasks(&conn).unwrap();
    assert!(!active.iter().any(|t| t.id == ids.task_id));
}

// ── Integration: swarm lifecycle ────────────────────────────────────────────

#[test]
fn cp_flow_swarm_lifecycle() {
    let conn = in_memory();
    let ids = create_swarm_workflow(&conn, "/proj", "add dark mode", "claude").unwrap();

    // Task starts as 'queued', run as 'pending'
    let task_status: String = conn
        .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(task_status, "queued");

    // Mark running
    mark_swarm_workflow_running(&conn, &ids.task_id, &ids.agent_run_id).unwrap();
    let task_status_2: String = conn
        .query_row("SELECT status FROM cp_tasks WHERE id=?1", params![ids.task_id], |r| r.get(0))
        .unwrap();
    assert_eq!(task_status_2, "running");

    // Active runs query shows the run
    let runs = cp_query_active_runs(&conn).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].provider, "swarm");
    assert_eq!(runs[0].task_id, ids.task_id);

    // Record a failure
    cp_record_run_failure(
        &conn,
        &ids.agent_run_id,
        &ProviderFailureKind::Timeout,
        "exceeded 3600s",
    )
    .unwrap();

    let exit_reason: String = conn
        .query_row("SELECT exit_reason FROM cp_agent_runs WHERE id=?1", params![ids.agent_run_id], |r| r.get(0))
        .unwrap();
    assert_eq!(exit_reason, "timeout");

    // No more active runs after failure
    let runs_after = cp_query_active_runs(&conn).unwrap();
    assert_eq!(runs_after.len(), 0);
}

// ── Integration: task graph lifecycle ───────────────────────────────────────

#[test]
fn cp_flow_task_graph_lifecycle() {
    let conn = in_memory();
    let graph_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // task_graphs is the FK parent; cp_task_graph_nodes references it
    conn.execute(
        "INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,'goal','claude','pending',?2)",
        params![graph_id, now],
    )
    .unwrap();

    // Node A — ready immediately
    let ids_a = create_task_graph_node_workflow(
        &conn, &graph_id, "a", "build", "cargo build", "claude", true,
    )
    .unwrap();

    // Node B — queued (depends on A)
    let ids_b = create_task_graph_node_workflow(
        &conn, &graph_id, "b", "test", "cargo test", "claude", false,
    )
    .unwrap();

    // Scheduler sees node A as ready
    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, ids_a.task_id);
    assert_eq!(ready[0].execution_kind, "task_graph");

    // Shell cmd is retrievable
    let cmd = cp_task_graph_shell_cmd(&conn, &ids_a.task_id).unwrap();
    assert_eq!(cmd.as_deref(), Some("cargo build"));

    // Complete node A, promote node B to ready
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

    // Create two tasks
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

    // Toggle first task to completed
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

    // Rebuild markdown
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

    // File approval task
    create_file_change_workflow(&conn, "/proj/a.rs", "old", "new", "claude").unwrap();

    // Swarm task
    create_swarm_workflow(&conn, "/proj", "dark mode", "claude").unwrap();

    // Personal task
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

    // All 3 tasks show up as active
    assert_eq!(snapshot.active_tasks.len(), 3);

    // 2 agent runs (file-change + swarm; personal has no run)
    assert_eq!(snapshot.active_runs.len(), 2);

    // 1 pending file_write approval
    assert_eq!(snapshot.pending_approvals.len(), 1);
    assert_eq!(snapshot.pending_approvals[0].approval_type, "file_write");

    // No blocked tasks
    assert_eq!(snapshot.blocked_tasks.len(), 0);

    // Origins cover both swarm and file_approval
    let origins: Vec<&str> = snapshot.active_tasks.iter().map(|t| t.origin.as_str()).collect();
    assert!(origins.contains(&"swarm"));
    assert!(origins.contains(&"file_approval"));
    assert!(origins.contains(&"personal"));
}

// ── Integration: budget deferral appears in snapshot ────────────────────────

#[test]
fn cp_snapshot_budget_deferral_visible() {
    let conn = in_memory();
    // Exhaust the claude provider budget
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
    // Create a workflow — this stores everything in canonical tables
    let ids =
        create_file_change_workflow(&conn, "/proj/src/lib.rs", "old code", "new code", "claude")
            .unwrap();

    // Simulate daemon state reload from DB (what refresh_pending_from_db does)
    let rows = cp_load_pending_file_change_approvals(&conn).unwrap();
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    // Canonical ID matches what the workflow inserted into cp_approvals
    assert_eq!(row.approval_id, ids.approval_id);
    // Path and content are hydrated from cp_artifacts.metadata_json
    assert_eq!(row.path, "/proj/src/lib.rs");
    assert_eq!(row.original_content, "old code");
    assert_eq!(row.new_content, "new code");
    assert_eq!(row.agent_name, "claude");
    // FK chains are intact
    assert_eq!(row.task_id.as_deref(), Some(ids.task_id.as_str()));
    assert_eq!(row.agent_run_id.as_deref(), Some(ids.agent_run_id.as_str()));
    assert_eq!(row.artifact_id.as_deref(), Some(ids.artifact_id.as_str()));
}

#[test]
fn cp_pending_file_change_approvals_disappears_after_resolve() {
    let conn = in_memory();
    let ids =
        create_file_change_workflow(&conn, "/proj/src/main.rs", "v1", "v2", "claude").unwrap();

    // Pre-condition: visible
    assert_eq!(cp_load_pending_file_change_approvals(&conn).unwrap().len(), 1);

    // Approve via canonical DB update
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_approvals SET status='approved', resolved_at=?1 WHERE id=?2",
        rusqlite::params![now, ids.approval_id],
    )
    .unwrap();

    // Post-condition: no longer pending
    assert_eq!(cp_load_pending_file_change_approvals(&conn).unwrap().len(), 0);
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
fn scheduled_jobs_reset_firing_on_restart() {
    let conn = in_memory();
    let _id = cp_scheduled_job_create(&conn, "Test Job", "claude", "desc", -5).unwrap();

    let claimed = cp_scheduled_jobs_claim_due(&conn).unwrap();
    assert_eq!(claimed[0].status, "firing");

    cp_scheduled_jobs_reset_firing(&conn).unwrap();

    let jobs = cp_scheduled_jobs_list(&conn).unwrap();
    assert_eq!(jobs[0].status, "active");
}
