use super::*;
const ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
fn roots(base: &Path) -> RecoveryRoots {
    RecoveryRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        opencode: base.join("opencode.db"),
    }
}
fn codex_file(roots: &RecoveryRoots, cwd: &Path, id: &str) {
    let dir = roots.codex.join("sessions/2026/10/11");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("rollout-2026-10-11-{id}.jsonl")),format!("{}\n{{\"partial\":",serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"source":"cli","timestamp":"2026-10-11T00:00:00Z"}}))).unwrap();
}
#[test]
fn reconstructs_native_sessions_across_projects_with_partial_last_turn_and_no_shutdown() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = roots(tmp.path());
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    codex_file(&roots, &project, ID);
    std::fs::write(roots.codex.join("session_index.jsonl"),format!("{}\n",serde_json::json!({"id":ID,"thread_name":"Database recovery","updated_at":"2026-10-11T01:00:00Z"}))).unwrap();
    let claude = roots.claude.join("projects/scope");
    std::fs::create_dir_all(&claude).unwrap();
    let cid = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    std::fs::write(claude.join(format!("{cid}.jsonl")),format!("{}\n",serde_json::json!({"sessionId":cid,"cwd":tmp.path(),"isSidechain":false,"type":"user","message":{"content":"not imported"}}))).unwrap();
    let c = catalog(&roots, &tmp.path().join("absent.db")).unwrap();
    assert_eq!(c.sessions.len(), 2);
    let p = plan(&c, ID).unwrap();
    assert_eq!(p.project_path, project.to_string_lossy());
    assert_eq!(p.name, "Database recovery");
    assert_eq!(p.args, vec!["resume", ID]);
    assert!(p.work.is_none());
    assert!(plan(&c, "last").is_err());
    assert!(!tmp.path().join("absent.db").exists());
}
#[test]
fn rejects_forged_ids_subagents_symlinks_and_missing_project() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = roots(tmp.path());
    codex_file(&roots, tmp.path(), ID);
    let dir = roots.codex.join("sessions/2026/10/11");
    let source = dir.join(format!("rollout-2026-10-11-{ID}.jsonl"));
    std::fs::rename(&source, dir.join("forged.jsonl")).unwrap();
    assert!(catalog(&roots, &tmp.path().join("none"))
        .unwrap()
        .sessions
        .is_empty());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("forged.jsonl"), &source).unwrap();
        assert!(catalog(&roots, &tmp.path().join("none"))
            .unwrap()
            .sessions
            .is_empty());
        std::fs::remove_file(&source).unwrap();
    }
    codex_file(&roots, &tmp.path().join("missing-project"), ID);
    let c = catalog(&roots, &tmp.path().join("none")).unwrap();
    assert!(!c.sessions[0].resumable);
    assert!(plan(&c, ID).is_err());
    std::fs::write(&source,serde_json::json!({"type":"session_meta","payload":{"id":ID,"cwd":tmp.path(),"source":{"subagent":{"parent_thread_id":ID}}}}).to_string()).unwrap();
    assert!(catalog(&roots, &tmp.path().join("none"))
        .unwrap()
        .sessions
        .is_empty());
}
#[test]
fn inventory_is_readonly_and_running_or_unverified_link_cannot_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = roots(tmp.path());
    codex_file(&roots, tmp.path(), ID);
    let path = tmp.path().join("workspace.db");
    let conn = Connection::open(&path).unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let (task, run) = raios_core::db::cp_session_start_named(
        &conn,
        "codex_kaira",
        None,
        tmp.path().to_str().unwrap(),
        None,
        Some("Saved work"),
        Some("Testing recovery"),
    )
    .unwrap();
    raios_core::db::cp_bind_wrapper_resume(&conn, &run, ID).unwrap();
    let c = catalog(&roots, &path).unwrap();
    assert_eq!(c.sessions.len(), 1);
    assert!(!c.sessions[0].resumable);
    assert!(plan(&c, &run).is_err());
    assert_eq!(c.sessions[0].name, "Saved work");
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
    raios_core::db::cp_session_end(&conn, &task, &run, false).unwrap();
    let c = catalog(&roots, &path).unwrap();
    assert!(plan(&c, &run).is_ok());
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 2);
}
#[test]
fn older_control_plane_is_not_migrated_by_listing() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = roots(tmp.path());
    codex_file(&roots, tmp.path(), ID);
    let path = tmp.path().join("workspace.db");
    let conn = Connection::open(&path).unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    conn.execute_batch("DROP TABLE cp_wrapper_resume").unwrap();
    let c = catalog(&roots, &path).unwrap();
    assert_eq!(c.sessions.len(), 1);
    assert!(conn.prepare("SELECT * FROM cp_wrapper_resume").is_err());
}
#[test]
fn exact_native_checkpoint_preserves_continuation_flags_and_never_guesses_last() {
    let mut args = vec!["--".into(), "prompt".into()];
    let id = prepare_native_checkpoint("claude", &mut args).unwrap();
    assert!(uuid::Uuid::parse_str(&id).is_ok());
    assert_eq!(&args[..2], &["--session-id", &id]);
    let mut args = vec!["--resume".into(), ID.into()];
    assert_eq!(
        prepare_native_checkpoint("claude", &mut args).as_deref(),
        Some(ID)
    );
    assert_eq!(args.len(), 2);
    for flag in [
        "--continue",
        "--fork-session",
        "--no-session-persistence",
        "--resume=last",
    ] {
        assert!(prepare_native_checkpoint("claude", &mut vec![flag.into()]).is_none());
    }
    assert!(
        prepare_native_checkpoint("codex", &mut vec!["resume".into(), "--last".into()]).is_none()
    );
    assert_eq!(
        prepare_native_checkpoint("codex", &mut vec!["resume".into(), ID.into()]).as_deref(),
        Some(ID)
    );
}
#[test]
fn opencode_database_has_exact_id_and_excludes_child_and_archived_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = roots(tmp.path());
    let conn = Connection::open(&roots.opencode).unwrap();
    conn.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,title TEXT,time_updated INTEGER,parent_id TEXT,time_archived INTEGER)").unwrap();
    for (id, parent, archived) in [
        ("ses_main", None, None),
        ("ses_child", Some("ses_main"), None),
        ("ses_archive", None, Some(1)),
    ] {
        conn.execute(
            "INSERT INTO session VALUES(?1,?2,'OpenCode work',1,?3,?4)",
            rusqlite::params![id, tmp.path().to_str().unwrap(), parent, archived],
        )
        .unwrap();
    }
    let c = catalog(&roots, &tmp.path().join("none")).unwrap();
    assert_eq!(c.sessions.len(), 1);
    assert_eq!(
        plan(&c, "opencode:ses_main").unwrap().args,
        vec!["--session", "ses_main"]
    );
}
