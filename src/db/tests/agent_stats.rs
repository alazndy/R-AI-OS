use super::*;

#[test]
fn empty_db_has_no_stats() {
    let conn = in_memory();
    assert_eq!(cp_agent_stats(&conn).unwrap(), vec![]);
    assert_eq!(cp_agent_stats_for(&conn, "claude_kaira").unwrap(), None);
}

#[test]
fn aggregates_success_rate_across_runs() {
    let conn = in_memory();
    let (t1, r1) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t1, &r1, true).unwrap();
    let (t2, r2) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t2, &r2, false).unwrap();
    let (t3, r3) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t3, &r3, true).unwrap();

    let stats = cp_agent_stats_for(&conn, "claude_kaira").unwrap().unwrap();
    assert_eq!(stats.total_runs, 3);
    assert_eq!(stats.succeeded, 2);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.success_rate, Some(2.0 / 3.0));
}

#[test]
fn still_open_runs_are_excluded_from_success_rate_denominator() {
    let conn = in_memory();
    // Two closed runs, one succeeded — plus three abandoned sessions that
    // never called cp_session_end (status stays 'running' forever, e.g. a
    // killed terminal). The abandoned ones must not drag success_rate down.
    let (t1, r1) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t1, &r1, true).unwrap();
    let (t2, r2) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t2, &r2, false).unwrap();
    for _ in 0..3 {
        cp_session_start(&conn, "claude_kaira", None).unwrap();
    }

    let stats = cp_agent_stats_for(&conn, "claude_kaira").unwrap().unwrap();
    assert_eq!(stats.total_runs, 5);
    assert_eq!(stats.succeeded, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.still_open, 3);
    assert_eq!(stats.success_rate, Some(0.5), "rate must be 1/(1+1), not 1/5");
}

#[test]
fn separates_agents_independently() {
    let conn = in_memory();
    let (t1, r1) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    cp_session_end(&conn, &t1, &r1, true).unwrap();
    let (t2, r2) = cp_session_start(&conn, "codex_kaira", None).unwrap();
    cp_session_end(&conn, &t2, &r2, false).unwrap();

    let all = cp_agent_stats(&conn).unwrap();
    assert_eq!(all.len(), 2);
    let claude = all.iter().find(|s| s.agent_name == "claude_kaira").unwrap();
    let codex = all.iter().find(|s| s.agent_name == "codex_kaira").unwrap();
    assert_eq!(claude.succeeded, 1);
    assert_eq!(codex.failed, 1);
}

#[test]
fn exit_reason_distribution_sorted_by_frequency() {
    let conn = in_memory();
    for success in [true, true, false] {
        let (t, r) = cp_session_start(&conn, "claude_kaira", None).unwrap();
        cp_session_end(&conn, &t, &r, success).unwrap();
    }
    let stats = cp_agent_stats_for(&conn, "claude_kaira").unwrap().unwrap();
    assert_eq!(stats.top_exit_reasons[0], ("clean_exit".to_string(), 2));
    assert_eq!(stats.top_exit_reasons[1], ("nonzero_exit".to_string(), 1));
}

#[test]
fn avg_duration_computed_from_valid_timestamps() {
    let conn = in_memory();
    let (t, r) = cp_session_start(&conn, "claude_kaira", None).unwrap();
    // Backdate started_at using the same clock+format cp_session_start itself
    // uses (chrono::Local, "%Y-%m-%d %H:%M:%S") so this doesn't drift against
    // whatever timezone SQLite's own datetime('now') (UTC) would assume.
    let ten_sec_ago = (chrono::Local::now() - chrono::Duration::seconds(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.execute(
        "UPDATE cp_agent_runs SET started_at = ?1 WHERE id = ?2",
        params![ten_sec_ago, r],
    )
    .unwrap();
    cp_session_end(&conn, &t, &r, true).unwrap();

    let stats = cp_agent_stats_for(&conn, "claude_kaira").unwrap().unwrap();
    let avg = stats.avg_duration_secs.expect("should compute a duration");
    assert!((8.0..=12.0).contains(&avg), "avg={avg}, expected ~10s");
}
