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

/// Reproduces the `raios stats` denominator bug: `total` is
/// `COUNT(*) FROM projects`, but `grade_a/b/c/d` are `COUNT(*) FROM
/// health_cache` with no join back to `projects`. A `health_cache` row
/// whose `project_id` no longer has a matching `projects` row (the real
/// workspace.db carries 146 such orphans, left behind by out-of-band
/// maintenance that ran without `PRAGMA foreign_keys=ON`, so
/// `ON DELETE CASCADE` never fired) still gets counted into the grade
/// buckets, so their sum can exceed `total`.
#[test]
fn query_stats_grade_totals_never_exceed_project_total() {
    let conn = in_memory();
    upsert_project(
        &conn,
        "Kept",
        "c",
        "/tmp/kept",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let kept_id = project_id_for_path(&conn, "/tmp/kept").unwrap();
    upsert_health(
        &conn,
        kept_id,
        "A",
        Some(90),
        None,
        None,
        0,
        0,
        false,
        true,
        true,
        None,
        "A",
        90,
        0,
    )
    .unwrap();

    upsert_project(
        &conn,
        "Removed",
        "c",
        "/tmp/removed",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let removed_id = project_id_for_path(&conn, "/tmp/removed").unwrap();
    upsert_health(
        &conn,
        removed_id,
        "A",
        Some(80),
        None,
        None,
        0,
        0,
        false,
        true,
        true,
        None,
        "A",
        80,
        0,
    )
    .unwrap();

    // Simulate the real-world orphaning mechanism: a manual maintenance
    // session (e.g. the sqlite3 CLI, which does not enable foreign key
    // enforcement by default) deletes the project row without cascading
    // to health_cache.
    conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    conn.execute("DELETE FROM projects WHERE id = ?1", params![removed_id])
        .unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

    let stats = query_stats(&conn).unwrap();
    assert_eq!(stats.total, 1, "the deleted project must not be counted");
    let grade_sum = stats.grade_a + stats.grade_b + stats.grade_c + stats.grade_d;
    assert!(
        grade_sum <= stats.total,
        "grade buckets ({grade_sum}) must not exceed total projects ({}); \
         an orphaned health_cache row is being counted",
        stats.total
    );
}

/// `upsert_security_score` must be safe to call from `raios security`,
/// which only ever has a security report on hand — never fresh
/// compliance/git/refactor data. It must not clobber whatever the
/// background health worker already wrote for those other columns.
#[test]
fn upsert_security_score_preserves_other_health_columns() {
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
        None,
        None,
        0,
        0,
        true,
        true,
        true,
        Some("gh/p"),
        "B",
        70,
        2,
    )
    .unwrap();

    upsert_security_score(&conn, id, "C", 55, 3, 1).unwrap();

    let row = conn
        .query_row(
            "SELECT compliance_grade, compliance_score, git_dirty, has_memory, has_sigmap,
                    remote_url, refactor_grade, refactor_score, refactor_high,
                    security_grade, security_score, security_issues, security_critical
             FROM health_cache WHERE project_id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "A", "compliance_grade must be preserved");
    assert_eq!(row.1, Some(90), "compliance_score must be preserved");
    assert_eq!(row.2, 1, "git_dirty must be preserved");
    assert_eq!(row.3, 1, "has_memory must be preserved");
    assert_eq!(row.4, 1, "has_sigmap must be preserved");
    assert_eq!(
        row.5.as_deref(),
        Some("gh/p"),
        "remote_url must be preserved"
    );
    assert_eq!(row.6, "B", "refactor_grade must be preserved");
    assert_eq!(row.7, 70, "refactor_score must be preserved");
    assert_eq!(row.8, 2, "refactor_high must be preserved");
    assert_eq!(row.9.as_deref(), Some("C"));
    assert_eq!(row.10, Some(55));
    assert_eq!(row.11, 3);
    assert_eq!(row.12, 1);
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

    let mut stmt = conn.prepare("PRAGMA index_list(bm25_postings)").unwrap();
    let indexes: Vec<String> = stmt
        .query_map([], |row| row.get(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(indexes.contains(&"idx_bm25_postings_file".to_string()));
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
    assert!(
        cols.contains(&"cp_task_id".to_string()),
        "columns: {cols:?}"
    );
    assert!(
        cols.contains(&"cp_agent_run_id".to_string()),
        "columns: {cols:?}"
    );
}

#[test]
fn swarm_tasks_has_control_plane_link_columns() {
    let conn = in_memory();
    let cols = table_columns(&conn, "swarm_tasks");
    for expected in [
        "cp_task_id",
        "cp_agent_run_id",
        "cp_artifact_id",
        "cp_approval_id",
    ] {
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
fn approvals_have_an_owner_and_owner_status_index() {
    let conn = in_memory();
    let cols = table_columns(&conn, "cp_approvals");
    assert!(
        cols.contains(&"owner_subject".to_string()),
        "columns: {cols:?}"
    );

    let mut stmt = conn.prepare("PRAGMA index_list(cp_approvals)").unwrap();
    let indexes: Vec<String> = stmt
        .query_map([], |row| row.get(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        indexes.contains(&"idx_cp_approvals_owner_status".to_string()),
        "indexes: {indexes:?}"
    );
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
