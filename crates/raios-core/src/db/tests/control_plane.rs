use super::*;

fn insert_task(conn: &Connection, id: &str, status: &str, plan_id: Option<&str>, assignee_id: Option<&str>) {
    conn.execute(
        "INSERT INTO cp_tasks (id, plan_id, title, description, priority, status, assignee_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'desc', 50, ?4, ?5, datetime('now'), datetime('now'))",
        params![id, plan_id, format!("task {id}"), status, assignee_id],
    )
    .unwrap();
}

fn insert_agent_run(conn: &Connection, id: &str, task_id: &str, status: &str, provider: &str) {
    conn.execute(
        "INSERT INTO cp_agent_runs (id, task_id, provider, agent_name, run_contract_id, status)
         VALUES (?1, ?2, ?3, 'claude_kaira', 'contract-1', ?4)",
        params![id, task_id, provider, status],
    )
    .unwrap();
}

fn insert_approval(conn: &Connection, id: &str, task_id: &str, approval_type: &str, status: &str) {
    conn.execute(
        "INSERT INTO cp_approvals (id, task_id, approval_type, reason, status, requested_at)
         VALUES (?1, ?2, ?3, 'because', ?4, datetime('now'))",
        params![id, task_id, approval_type, status],
    )
    .unwrap();
}

// ─── cp_scheduler_list_ready ──────────────────────────────────────────────────

#[test]
fn scheduler_list_ready_returns_ready_task_with_no_gates() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, "t1");
}

#[test]
fn scheduler_list_ready_excludes_task_with_pending_approval() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_approval(&conn, "ap1", "t1", "file_write", "pending");
    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert!(ready.is_empty(), "task with a pending approval must not be scheduled");
}

#[test]
fn scheduler_list_ready_excludes_non_ready_status() {
    let conn = in_memory();
    insert_task(&conn, "t1", "running", None, None);
    insert_task(&conn, "t2", "completed", None, None);
    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert!(ready.is_empty());
}

#[test]
fn scheduler_list_ready_blocks_task_on_hard_budget_block() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, Some("claude"));
    conn.execute(
        "INSERT INTO cp_budget_ledger
            (id, scope_kind, scope_id, provider, metric, limit_value, used_value, remaining_value, confidence, source, observed_at)
         VALUES ('b1', 'provider', 'claude', 'claude', 'tokens', 100.0, 100.0, 0.0, 'exact', 'test', datetime('now'))",
        [],
    )
    .unwrap();

    let ready = cp_scheduler_list_ready(&conn).unwrap();
    assert!(ready.is_empty(), "exhausted budget should hard-block the task");

    let status: String = conn
        .query_row("SELECT status FROM cp_tasks WHERE id='t1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "blocked", "task should be moved to blocked status");
}

// ─── cp_task_graph_node_ids / cp_task_graph_shell_cmd ────────────────────────

fn seed_task_graph_node(conn: &Connection, graph_id: &str, node_id: &str, task_id: &str, shell_cmd: &str) {
    conn.execute(
        "INSERT INTO task_graphs (id, goal, agent) VALUES (?1, 'goal', 'claude')",
        params![graph_id],
    )
    .unwrap();
    insert_task(conn, task_id, "ready", Some(graph_id), None);
    insert_agent_run(conn, "run1", task_id, "pending", "claude");
    conn.execute(
        "INSERT INTO cp_task_graph_nodes (graph_id, node_id, task_id, agent_run_id, shell_cmd, created_at)
         VALUES (?1, ?2, ?3, 'run1', ?4, datetime('now'))",
        params![graph_id, node_id, task_id, shell_cmd],
    )
    .unwrap();
}

#[test]
fn task_graph_node_ids_round_trip() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    let ids = cp_task_graph_node_ids(&conn, "t1").unwrap();
    assert_eq!(ids, Some(("g1".to_string(), "n1".to_string())));
}

#[test]
fn task_graph_node_ids_returns_none_for_unknown_task() {
    let conn = in_memory();
    assert_eq!(cp_task_graph_node_ids(&conn, "does-not-exist").unwrap(), None);
}

#[test]
fn task_graph_shell_cmd_round_trip() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    assert_eq!(cp_task_graph_shell_cmd(&conn, "t1").unwrap().as_deref(), Some("echo hi"));
}

// ─── cp_query_active_tasks / cp_query_blocked_tasks ──────────────────────────

#[test]
fn query_active_tasks_excludes_terminal_statuses() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_task(&conn, "t2", "running", None, None);
    insert_task(&conn, "t3", "completed", None, None);
    insert_task(&conn, "t4", "cancelled", None, None);
    insert_task(&conn, "t5", "failed", None, None);

    let active = cp_query_active_tasks(&conn).unwrap();
    let ids: Vec<&str> = active.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"t1"));
    assert!(ids.contains(&"t2"));
}

#[test]
fn query_active_tasks_derives_task_graph_origin() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    let active = cp_query_active_tasks(&conn).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].origin, "task_graph");
}

#[test]
fn query_active_tasks_derives_file_approval_origin() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_approval(&conn, "ap1", "t1", "file_write", "pending");
    let active = cp_query_active_tasks(&conn).unwrap();
    assert_eq!(active[0].origin, "file_approval");
}

#[test]
fn query_blocked_tasks_returns_only_blocked() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_task(&conn, "t2", "blocked", None, None);
    let blocked = cp_query_blocked_tasks(&conn).unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].id, "t2");
}

// ─── cp_query_pending_approvals / cp_query_active_runs ───────────────────────

#[test]
fn query_pending_approvals_joins_task_title() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_approval(&conn, "ap1", "t1", "handover", "pending");
    insert_approval(&conn, "ap2", "t1", "handover", "approved");

    let pending = cp_query_pending_approvals(&conn).unwrap();
    assert_eq!(pending.len(), 1, "approved approvals must be excluded");
    assert_eq!(pending[0].id, "ap1");
    assert_eq!(pending[0].task_title.as_deref(), Some("task t1"));
}

#[test]
fn query_active_runs_filters_by_status() {
    let conn = in_memory();
    insert_task(&conn, "t1", "running", None, None);
    insert_task(&conn, "t2", "running", None, None);
    insert_agent_run(&conn, "r1", "t1", "running", "claude");
    insert_agent_run(&conn, "r2", "t2", "completed", "claude");

    let active = cp_query_active_runs(&conn).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "r1");
}

// ─── cp_daemon_snapshot ───────────────────────────────────────────────────────

#[test]
fn daemon_snapshot_aggregates_all_sections() {
    let conn = in_memory();
    insert_task(&conn, "t1", "ready", None, None);
    insert_task(&conn, "t2", "blocked", None, None);
    insert_agent_run(&conn, "r1", "t1", "running", "claude");
    insert_approval(&conn, "ap1", "t1", "handover", "pending");

    let snap = cp_daemon_snapshot(&conn).unwrap();
    assert_eq!(snap.active_tasks.len(), 2); // ready + blocked are both non-terminal
    assert_eq!(snap.blocked_tasks.len(), 1);
    assert_eq!(snap.active_runs.len(), 1);
    assert_eq!(snap.pending_approvals.len(), 1);
}

// ─── cp_detect_graph_cache_drift / cp_rebuild_task_graph_cache ───────────────

#[test]
fn detect_graph_cache_drift_finds_missing_and_stale() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    // canonical node with no matching legacy cache row -> missing_from_cache
    let report = cp_detect_graph_cache_drift(&conn, "g1").unwrap();
    assert_eq!(report.missing_from_cache, vec!["t1".to_string()]);
    assert!(report.stale_in_cache.is_empty());
}

#[test]
fn rebuild_task_graph_cache_syncs_status_from_canonical() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    conn.execute(
        "INSERT INTO task_graph_nodes (id, graph_id, description, shell_cmd, status)
         VALUES ('n1', 'g1', 'desc', 'echo hi', 'pending')",
        [],
    )
    .unwrap();
    conn.execute("UPDATE cp_tasks SET status='completed' WHERE id='t1'", [])
        .unwrap();

    let rebuilt = cp_rebuild_task_graph_cache(&conn).unwrap();
    assert_eq!(rebuilt, 1);

    let legacy_status: String = conn
        .query_row(
            "SELECT status FROM task_graph_nodes WHERE graph_id='g1' AND id='n1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy_status, "completed");
}

#[test]
fn rebuild_task_graph_cache_is_idempotent() {
    let conn = in_memory();
    seed_task_graph_node(&conn, "g1", "n1", "t1", "echo hi");
    conn.execute(
        "INSERT INTO task_graph_nodes (id, graph_id, description, shell_cmd, status)
         VALUES ('n1', 'g1', 'desc', 'echo hi', 'pending')",
        [],
    )
    .unwrap();
    conn.execute("UPDATE cp_tasks SET status='running' WHERE id='t1'", [])
        .unwrap();

    let first = cp_rebuild_task_graph_cache(&conn).unwrap();
    let second = cp_rebuild_task_graph_cache(&conn).unwrap();
    assert_eq!(first, 1, "first run should update the stale row");
    assert_eq!(second, 0, "second run has nothing left to change");
}
