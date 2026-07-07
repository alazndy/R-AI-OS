use super::*;

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
fn tool_traces_table_exists() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tool_traces", [], |r| r.get(0))
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

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

/// GraphStore (raios-core::task_graph::store) and SwarmStore
/// (raios-runtime::swarm::store) used to each carry their own duplicate
/// CREATE TABLE + a bolt-on ALTER TABLE for these cp_* columns, which had
/// drifted out of sync with this central migration (task_graph_nodes was
/// missing them entirely until this was caught). Both stores now rely
/// solely on this migration via their connect()'s migrate_existing() call —
/// this test is what would have caught the original drift.
#[test]
fn task_graph_nodes_has_control_plane_link_columns() {
    let conn = in_memory();
    let cols = table_columns(&conn, "task_graph_nodes");
    assert!(cols.contains(&"cp_task_id".to_string()), "columns: {cols:?}");
    assert!(cols.contains(&"cp_agent_run_id".to_string()), "columns: {cols:?}");
}

#[test]
fn swarm_tasks_has_control_plane_link_columns() {
    let conn = in_memory();
    let cols = table_columns(&conn, "swarm_tasks");
    for expected in ["cp_task_id", "cp_agent_run_id", "cp_artifact_id", "cp_approval_id"] {
        assert!(cols.contains(&expected.to_string()), "columns: {cols:?}");
    }
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

    let ids =
        create_file_change_workflow(&conn, "/repo/src/main.rs", "old", "new", "claude").unwrap();
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
    let ids = create_file_change_workflow(&conn, "/tmp/notes.md", "old", "new", "claude").unwrap();
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
