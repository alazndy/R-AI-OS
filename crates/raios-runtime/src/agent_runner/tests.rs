use super::*;

#[test]
fn handoff_menu_requires_an_explicit_choice_and_defers_on_eof() {
    for (input, expected) in [
        ("a\n", HandoffAction::Accept),
        ("r\n", HandoffAction::Reject),
        ("c\n", HandoffAction::Complete),
        ("l\n", HandoffAction::Defer),
        ("\n", HandoffAction::Defer),
        ("", HandoffAction::Defer),
        ("invalid\nreject\n", HandoffAction::Reject),
    ] {
        let mut output = Vec::new();
        assert_eq!(
            read_handoff_action(&mut io::Cursor::new(input), &mut output).unwrap(),
            expected
        );
    }
}

#[test]
fn detached_child_reaper_waits_for_short_lived_processes() {
    #[cfg(unix)]
    let child = Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("short-lived test child should spawn");
    #[cfg(windows)]
    let child = Command::new("cmd")
        .args(["/C", "exit 0"])
        .spawn()
        .expect("short-lived test child should spawn");

    let completion = submit_detached_child_for_test(child)
        .expect("child should be accepted by the detached reaper");
    let status = completion
        .recv_timeout(Duration::from_secs(5))
        .expect("reaper should wait for the child before the timeout");

    assert!(status.success());
}

#[test]
fn codex_command_always_enables_workspace_write_network_access() {
    let cmd = codex_command();
    assert_eq!(cmd.get_program(), "codex");
    let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
    assert_eq!(args, vec!["-c", CODEX_NETWORK_ACCESS_CONFIG]);
}

#[test]
fn wrapper_local_capture_accepts_only_one_bounded_prompt_for_unscoped_agents() {
    assert_eq!(
        wrapper_launch_input("codex", &["we decided to use SQLite".to_string()]),
        Some("we decided to use SQLite")
    );
    assert_eq!(
        wrapper_launch_input("opencode", &["use pnpm".to_string()]),
        Some("use pnpm")
    );
    assert_eq!(
        wrapper_launch_input("claude", &["use pnpm".to_string()]),
        None
    );
    assert_eq!(
        wrapper_launch_input("codex", &["--version".to_string()]),
        None
    );
    assert_eq!(
        wrapper_launch_input("codex", &["--model".to_string(), "gpt-5".to_string()]),
        None
    );
    assert_eq!(
        wrapper_launch_input("codex", &["x".repeat(MAX_WRAPPER_LAUNCH_INPUT_CHARS + 1)]),
        None
    );
    assert_eq!(
        wrapper_launch_input("codex", &["password = 'superSecretValue123'".to_string()]),
        None
    );
}

#[test]
fn informational_invocations_do_not_consume_handoffs() {
    assert!(is_informational_invocation(&["--help".to_string()]));
    assert!(is_informational_invocation(&["-V".to_string()]));
    assert!(is_informational_invocation(&[
        "exec".to_string(),
        "--version".to_string(),
    ]));
    assert!(!is_informational_invocation(&[
        "implement the feature".to_string()
    ]));
}

#[test]
fn claude_handoff_starts_work_without_print_mode_or_stdin_pause() {
    let cmd =
        handoff_agent_command("claude", Some("[HANDOVER CONTEXT]\nimplement auth"), &[]).unwrap();
    let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
    assert_eq!(cmd.get_program(), "claude");
    assert_eq!(
        args,
        [
            "--append-system-prompt",
            "[HANDOVER CONTEXT]\nimplement auth",
            "[HANDOVER CONTEXT]\nimplement auth"
        ]
    );
    assert!(!args.contains(&"--print"));
}

#[test]
fn claude_handoff_preserves_an_explicit_user_prompt() {
    let extra = vec!["--print".to_string(), "review the diff".to_string()];
    let cmd = handoff_agent_command("claude", Some("handoff context"), &extra).unwrap();
    let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
    assert_eq!(
        args,
        [
            "--append-system-prompt",
            "handoff context",
            "--print",
            "review the diff"
        ]
    );
}

#[test]
fn handoff_is_delivered_only_in_the_registered_project() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let project_id = raios_core::db::upsert_project(
        &conn,
        "handoff-project",
        "tools",
        "/handoff-project",
        None,
        "active",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: "/handoff-project",
            from_agent: "codex_kaira",
            to_agent: "claude_kaira",
            status: "SUCCESS",
            msg: "implement auth",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    assert!(
        pending_handoff_for_project(&conn, "/unregistered", "claude_kaira")
            .unwrap()
            .is_none()
    );
    assert!(
        raios_core::db::cp_take_pending_handoff(&conn, Some(project_id), "claude_kaira")
            .unwrap()
            .is_some()
    );
    assert!(
        pending_handoff_for_project(&conn, "/handoff-project", "claude_kaira")
            .unwrap()
            .is_some()
    );
}

#[test]
fn handoff_delivery_supports_unregistered_directories_without_global_fallback() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    for project in ["/unregistered-a", "/unregistered-b"] {
        raios_core::db::create_handoff_workflow(
            &conn,
            raios_core::db::HandoffWorkflowInput {
                project_path: project,
                from_agent: "codex_kaira",
                to_agent: "claude_kaira",
                status: "SUCCESS",
                msg: project,
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();
    }
    assert_eq!(
        pending_handoff_for_project(&conn, "/unregistered-a", "claude_kaira")
            .unwrap()
            .unwrap()
            .context_summary,
        "/unregistered-a"
    );
    let ctx = pending_handoff_for_project(&conn, "/unregistered-b", "claude_kaira")
        .unwrap()
        .unwrap();
    assert_eq!(ctx.context_summary, "/unregistered-b");
    assert!(
        pending_handoff_for_project(&conn, "/another-project", "claude_kaira")
            .unwrap()
            .is_none()
    );
}

// ─── canonical_agent_identity ────────────────────────────────────────

#[test]
fn maps_known_agents_to_kaira_identities() {
    assert_eq!(canonical_agent_identity("claude"), Some("claude_kaira"));
    assert_eq!(canonical_agent_identity("codex"), Some("codex_kaira"));
    assert_eq!(canonical_agent_identity("opencode"), Some("opencode_kaira"));
    assert_eq!(
        canonical_agent_identity("antigravity"),
        Some("antigravity_kaira")
    );
    assert_eq!(canonical_agent_identity("agy"), Some("antigravity_kaira"));
}

#[test]
fn agent_identity_lookup_is_case_insensitive() {
    assert_eq!(canonical_agent_identity("Claude"), Some("claude_kaira"));
    assert_eq!(canonical_agent_identity("CODEX"), Some("codex_kaira"));
    assert_eq!(canonical_agent_identity("AGY"), Some("antigravity_kaira"));
}

#[test]
fn unknown_agent_returns_none_without_blocking_spawn() {
    // Agents outside the 4-agent matrix (e.g. "cursor") still spawn
    // normally — this must fail open (None), never panic or error.
    assert_eq!(canonical_agent_identity("cursor"), None);
    assert_eq!(canonical_agent_identity(""), None);
    assert_eq!(canonical_agent_identity("gpt-5"), None);
}

// ─── ext_command_from_task_description ──────────────────────────────

#[test]
fn parses_a_well_formed_ext_task_description() {
    assert_eq!(
        ext_command_from_task_description("raios ext atc process"),
        Some(("atc", "process"))
    );
}

#[test]
fn preserves_multi_word_commands_after_the_extension_name() {
    // splitn(2, ' ') must not truncate a command that itself has spaces.
    assert_eq!(
        ext_command_from_task_description("raios ext atc per project"),
        Some(("atc", "per project"))
    );
}

#[test]
fn plain_ai_agent_task_descriptions_do_not_match() {
    assert_eq!(ext_command_from_task_description("backup database"), None);
    assert_eq!(ext_command_from_task_description(""), None);
}

#[test]
fn rejects_prefix_without_a_trailing_command() {
    assert_eq!(ext_command_from_task_description("raios ext atc"), None);
    assert_eq!(ext_command_from_task_description("raios ext "), None);
}

// ─── strip_session_block ─────────────────────────────────────────────

#[test]
fn strip_session_block_removes_an_existing_block() {
    let content = "# My Notes\n\nSome content.\n\n\
        <!-- raios-session-begin -->\n\
        # RAIOS WRAPPER SESSION\n- Session ID: `abc12345`\n\
        <!-- raios-session-end -->\n\n\
        More content after.\n";
    let stripped = strip_session_block(content);
    assert!(!stripped.contains("raios-session-begin"));
    assert!(!stripped.contains("Session ID"));
    assert!(stripped.contains("# My Notes"));
    assert!(stripped.contains("More content after."));
}

#[test]
fn strip_session_block_is_a_noop_without_a_block() {
    let content = "# My Notes\n\nJust some plain content, no session block.\n";
    assert_eq!(strip_session_block(content), content);
}

#[test]
fn strip_session_block_replaces_stale_block_on_reinjection() {
    // inject_session_to_claude_md always strips first, then appends —
    // this is what guarantees re-running an agent never accumulates
    // multiple stacked session blocks in CLAUDE.md.
    let content = "# Notes\n\n\
        <!-- raios-session-begin -->\nold session\n<!-- raios-session-end -->\n";
    let stripped = strip_session_block(content);
    let reinjected = format!(
        "{}\n<!-- raios-session-begin -->\nnew session\n<!-- raios-session-end -->\n",
        stripped.trim_end()
    );
    assert_eq!(reinjected.matches("raios-session-begin").count(), 1);
    assert!(reinjected.contains("new session"));
    assert!(!reinjected.contains("old session"));
}

#[test]
fn strip_session_block_handles_multiple_begin_markers_by_taking_outermost_span() {
    // find() picks the first BEGIN, rfind() picks the last END — this
    // documents the actual (not obviously correct) behavior on
    // malformed/doubled markers rather than leaving it unverified.
    let content = "pre\n<!-- raios-session-begin -->\nA\n<!-- raios-session-begin -->\nB\n<!-- raios-session-end -->\npost";
    let stripped = strip_session_block(content);
    assert_eq!(stripped, "pre\npost");
}

// ─── compact context (budget + sigmap) ────────────────────────────────

/// A project of `bytes` source bytes, optionally with a fake `sigmap` script
/// that records that it ran and exits with `exit_code`.
#[cfg(unix)]
fn budget_fixture(bytes: usize) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), vec![b'x'; bytes]).unwrap();
    dir
}

#[cfg(unix)]
fn fake_sigmap(exit_code: i32) -> (tempfile::TempDir, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::TempDir::new().unwrap();
    let script = dir.path().join("sigmap");
    // Runs in the project dir (current_dir): leave a marker there.
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch sigmap-ran\nexit {exit_code}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, script)
}

#[cfg(unix)]
#[test]
fn small_project_stays_under_budget_and_never_runs_sigmap() {
    let project = budget_fixture(100);
    let (_keep, sigmap) = fake_sigmap(0);
    assert!(!prepare_compact_context_with(project.path(), 1024, &sigmap));
    assert!(!project.path().join("sigmap-ran").exists());
}

#[cfg(unix)]
#[test]
fn large_project_runs_sigmap_and_enables_compact_mode_on_success() {
    let project = budget_fixture(5000);
    let (_keep, sigmap) = fake_sigmap(0);
    assert!(prepare_compact_context_with(project.path(), 1024, &sigmap));
    assert!(project.path().join("sigmap-ran").exists());
}

#[cfg(unix)]
#[test]
fn failing_sigmap_does_not_promise_compact_context() {
    let project = budget_fixture(5000);
    let (_keep, sigmap) = fake_sigmap(3);
    assert!(!prepare_compact_context_with(project.path(), 1024, &sigmap));
}

#[test]
fn missing_sigmap_does_not_promise_compact_context() {
    let project = tempfile::TempDir::new().unwrap();
    std::fs::write(project.path().join("main.rs"), vec![b'x'; 5000]).unwrap();
    assert!(!prepare_compact_context_with(
        project.path(),
        1024,
        "/definitely/not/installed/sigmap"
    ));
}

#[cfg(unix)]
#[test]
fn fresh_sigmap_is_reused_without_running_the_tool_again() {
    let project = budget_fixture(5000);
    let main = project.path().join("main.rs");
    let map = project.path().join("SIGMAP.md");
    std::fs::write(&map, "map").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(100);
    std::fs::File::options()
        .write(true)
        .open(&main)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let (_keep, sigmap) = fake_sigmap(1);
    assert!(prepare_compact_context_with(project.path(), 1024, &sigmap));
    assert!(
        !project.path().join("sigmap-ran").exists(),
        "an up-to-date map must not be regenerated"
    );
}

// ─── wrapper-note IPC — real loopback TCP transport ────────────────────

#[cfg(unix)]
mod wrapper_note_ipc {
    use super::*;
    use rusqlite::params;
    use std::io::Read;
    use std::net::TcpStream;

    // `RAIOS_DB_PATH` is process-global; serialize with the crate-wide
    // `crate::DB_ENV_LOCK` (see lib.rs) so parallel `cargo test` threads
    // never race on the same env var with other tests in this binary.

    // The listener's accept loop is a non-blocking poll (20ms interval,
    // see start_wrapper_note_ipc), so a connect() can complete at the
    // kernel level slightly before the worker thread is scheduled to
    // accept() it. Under CI load (observed twice on macOS runners, with
    // two different symptoms: a validation mismatch and a raw
    // ConnectionReset) that window is enough for the OS to reset an
    // established-but-not-yet-accepted connection. This is a transient
    // test-harness timing issue, not a bug in the production capability
    // check itself, so send_note retries the whole round-trip a few
    // times before giving up rather than the test flaking outright.
    fn send_note(addr: SocketAddr, run_id: &str, note: &str) -> serde_json::Value {
        let payload = format!("{}\n", serde_json::json!({"run_id": run_id, "note": note}));
        let mut last_err = None;
        for attempt in 0..5 {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(50));
            }
            let attempted: io::Result<serde_json::Value> = (|| {
                let mut stream = TcpStream::connect(addr)?;
                stream.write_all(payload.as_bytes())?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                Ok(serde_json::from_str(&response).expect("server sent invalid JSON"))
            })();
            match attempted {
                Ok(value) => return value,
                Err(e) => last_err = Some(e),
            }
        }
        panic!("send_note to {addr} failed after retries: {last_err:?}");
    }

    fn stop(ipc: WrapperNoteIpc) {
        ipc.stop.store(true, Ordering::Relaxed);
        let _ = ipc.worker.join();
    }

    #[test]
    fn wrapper_note_ipc_two_projects_parallel_no_cross_leak() {
        let _lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original_db_path = std::env::var("RAIOS_DB_PATH").ok();
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        std::env::set_var("RAIOS_DB_PATH", tmp_db.path());

        let conn = raios_core::db::open_db().unwrap();
        let project_a = raios_core::db::upsert_project(
            &conn,
            "Project A",
            "core",
            "/tmp/raios-wrapper-note-test-a",
            None,
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let project_b = raios_core::db::upsert_project(
            &conn,
            "Project B",
            "core",
            "/tmp/raios-wrapper-note-test-b",
            None,
            "active",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let (_, run_a) =
            raios_core::db::cp_session_start(&conn, "codex_kaira", Some(project_a)).unwrap();
        let (_, run_b) =
            raios_core::db::cp_session_start(&conn, "codex_kaira", Some(project_b)).unwrap();
        drop(conn);

        let ipc_a =
            start_wrapper_note_ipc(run_a.clone(), "/tmp/raios-wrapper-note-test-a".to_string())
                .unwrap();
        let ipc_b =
            start_wrapper_note_ipc(run_b.clone(), "/tmp/raios-wrapper-note-test-b".to_string())
                .unwrap();

        // Legitimate note into each project's own socket succeeds.
        let resp_a = send_note(ipc_a.addr, &run_a, "note for A");
        assert_eq!(resp_a["recorded"], true, "resp_a = {resp_a}");
        let resp_b = send_note(ipc_b.addr, &run_b, "note for B");
        assert_eq!(resp_b["recorded"], true, "resp_b = {resp_b}");

        // Cross-wiring: each socket only recognizes the run ID it was
        // started with, so presenting the *other* project's run ID must
        // be rejected by the socket itself, never reach the DB as a
        // write into the wrong project.
        let cross_into_a = send_note(ipc_a.addr, &run_b, "cross leak into A");
        assert_eq!(
            cross_into_a["recorded"], false,
            "cross_into_a = {cross_into_a}"
        );
        let cross_into_b = send_note(ipc_b.addr, &run_a, "cross leak into B");
        assert_eq!(
            cross_into_b["recorded"], false,
            "cross_into_b = {cross_into_b}"
        );

        stop(ipc_a);
        stop(ipc_b);

        let conn = raios_core::db::open_db().unwrap();
        let events_a: Vec<String> = conn
            .prepare("SELECT content FROM cp_wrapper_events WHERE project_id=?1")
            .unwrap()
            .query_map(params![project_a], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(events_a, vec!["note for A".to_string()]);

        let events_b: Vec<String> = conn
            .prepare("SELECT content FROM cp_wrapper_events WHERE project_id=?1")
            .unwrap()
            .query_map(params![project_b], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(events_b, vec!["note for B".to_string()]);
        drop(conn);

        match original_db_path {
            Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
            None => std::env::remove_var("RAIOS_DB_PATH"),
        }
    }
}

// ─── the handoff hint printed after a run ─────────────────────────────

#[test]
fn handoff_target_is_the_value_raios_handoff_accepts() {
    // These are exactly the `raios handoff --to` choices (HandoffTarget).
    assert_eq!(handoff_target("claude"), "claude-kaira");
    assert_eq!(handoff_target("codex"), "codex-kaira");
    assert_eq!(handoff_target("opencode"), "opencode-kaira");
    assert_eq!(handoff_target("antigravity"), "antigravity-kaira");
    // Aliases and case resolve through the canonical identity.
    assert_eq!(handoff_target("agy"), "antigravity-kaira");
    assert_eq!(handoff_target("Claude"), "claude-kaira");
}

#[test]
fn handoff_target_for_an_agent_without_an_identity_keeps_the_plain_form() {
    assert_eq!(handoff_target("cursor"), "cursor-kaira");
}

#[cfg(unix)]
mod cleanup_guard {
    use super::*;

    /// Points HOME at a temp dir holding a `.claude/CLAUDE.md` for the duration of a test.
    struct FakeHome {
        _dir: tempfile::TempDir,
        claude_md: std::path::PathBuf,
        old_home: Option<std::ffi::OsString>,
    }

    impl FakeHome {
        fn new(content: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
            let claude_md = dir.path().join(".claude/CLAUDE.md");
            std::fs::write(&claude_md, content).unwrap();
            let old_home = std::env::var_os("HOME");
            std::env::set_var("HOME", dir.path());
            FakeHome {
                _dir: dir,
                claude_md,
                old_home,
            }
        }
    }

    impl Drop for FakeHome {
        fn drop(&mut self) {
            match &self.old_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn dropping_the_cleanup_removes_the_session_block_on_any_exit_path() {
        let _lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = FakeHome::new("# my rules\n");
        inject_session_to_claude_md("0123456789abcdef", "claude_kaira", "now");
        assert!(std::fs::read_to_string(&home.claude_md)
            .unwrap()
            .contains("raios-session-begin"));

        // Simulates an early return / panic: the guard is dropped without `release()`.
        drop(RunCleanup::new(true, None));

        let after = std::fs::read_to_string(&home.claude_md).unwrap();
        assert!(
            !after.contains("raios-session"),
            "block left behind: {after}"
        );
        assert!(after.contains("# my rules"));
    }

    #[test]
    fn release_is_idempotent_and_leaves_a_later_block_alone() {
        let _lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = FakeHome::new("# my rules\n");
        let mut cleanup = RunCleanup::new(true, None);
        cleanup.release();
        // Another wrapper injects its own block after our explicit release...
        inject_session_to_claude_md("fedcba9876543210", "claude_kaira", "later");
        // ...and our guard dropping afterwards must not strip it.
        drop(cleanup);
        assert!(std::fs::read_to_string(&home.claude_md)
            .unwrap()
            .contains("fedcba98"));
    }

    #[test]
    fn sigterm_to_the_wrapper_stops_the_agent_instead_of_killing_the_wrapper() {
        let _lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _signals = SignalGuard::install();
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let started = Instant::now();

        // SAFETY: raises a signal on this process; the installed handler only sets a flag.
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        let result = wait_for_agent(&mut child, "sleep", None);

        assert!(result.unwrap_err().contains("terminated"));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "agent was not stopped promptly"
        );
        assert!(child.try_wait().unwrap().is_some(), "agent still running");
    }

    #[test]
    fn signal_guard_restores_the_previous_handler_and_clears_the_flag() {
        let _lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        {
            let _signals = SignalGuard::install();
            unsafe { libc::raise(libc::SIGHUP) };
            assert!(termination_requested());
        }
        let _again = SignalGuard::install();
        assert!(!termination_requested(), "flag must reset on a new run");
    }
}
