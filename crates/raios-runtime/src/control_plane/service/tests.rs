use super::snapshots::compact_memory_preview;
use super::*;

/// Points `dirs::config_dir()` at an empty tempdir for the duration of a
/// test, so `Config::load()` inside `load_work_snapshot` never reads the
/// live `~/.config/raios/config.toml`. Without this, the
/// `factory.enabled` assertion in `snapshot_generation_on_in_memory_db`
/// depends on the machine running the tests. Serialized on the crate-wide
/// `TEST_ENV_LOCK` (see lib.rs) with the other env-redirecting tests.
struct IsolatedConfigDir {
    _lock: std::sync::MutexGuard<'static, ()>,
    original_xdg_config_home: Option<String>,
    _tmp: tempfile::TempDir,
}

impl IsolatedConfigDir {
    fn new() -> Self {
        let lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
        // dirs::config_dir() prefers XDG_CONFIG_HOME over HOME and only
        // accepts absolute paths; TempDir paths are absolute.
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        Self {
            _lock: lock,
            original_xdg_config_home,
            _tmp: tmp,
        }
    }
}

impl Drop for IsolatedConfigDir {
    fn drop(&mut self) {
        match &self.original_xdg_config_home {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}

#[test]
fn snapshot_generation_on_in_memory_db() {
    let _isolated_config = IsolatedConfigDir::new();
    let conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();

    let snap = load_system_snapshot(&conn).unwrap();
    assert!(snap.sequence >= 1);
    assert!(!snap.timestamp.is_empty());
    assert_eq!(snap.now.active_runs.len(), 0);
    assert_eq!(snap.work.factory.product_count, 0);
    // An empty database does not imply the user's Factory feature is off.
    // Default-off policy is tested using Config::default in raios-core.
}

#[test]
fn work_snapshot_projects_read_only_factory_overview() {
    let _db = raios_core::test_support::IsolatedDb::new();
    let conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    conn.execute(
        "INSERT INTO cp_workspaces (id, name, owner_subject) VALUES ('workspace-1', 'Workspace', 'local_control_owner')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_products (id, workspace_id, owner_subject, title, status) VALUES ('product-1', 'workspace-1', 'local_control_owner', 'Pilot', 'active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_plans (id, workspace_id, product_id, title, status) VALUES ('plan-1', 'workspace-1', 'product-1', 'Pilot plan', 'planned')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_cycles (id, product_id, plan_id, status, current_stage) VALUES ('cycle-1', 'product-1', 'plan-1', 'active', 'discover')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_change_requests (id, product_id, requested_by, status, summary) VALUES ('change-1', 'product-1', 'local_control_owner', 'awaiting_approval', 'Change')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_support_items (id, product_id, source_kind, status) VALUES ('support-1', 'product-1', 'manual', 'triaged')",
        [],
    )
    .unwrap();

    let snapshot = load_work_snapshot(&conn).unwrap();
    assert_eq!(snapshot.factory.product_count, 1);
    assert_eq!(snapshot.factory.active_cycle_count, 1);
    assert_eq!(snapshot.factory.pending_change_request_count, 1);
    assert_eq!(snapshot.factory.open_support_items, 1);
    assert_eq!(
        snapshot
            .factory
            .latest_product
            .as_ref()
            .map(|product| product.title.as_str()),
        Some("Pilot")
    );
}

#[test]
fn memory_preview_is_bounded_and_skips_blank_lines() {
    let contents = "\n# Project Memory\n\nStatus: Active\n\nObjective: Improve the TUI\n";

    assert_eq!(
        compact_memory_preview(contents).as_deref(),
        Some("# Project Memory\nStatus: Active\nObjective: Improve the TUI")
    );
    assert_eq!(compact_memory_preview("\n \n"), None);
}

#[test]
fn command_dispatch_idempotency_and_payload_check() {
    let _db = raios_core::test_support::IsolatedDb::new();
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();

    let workflow = raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: "/tmp/control-plane-test",
            from_agent: "codex_kaira",
            to_agent: "antigravity_kaira",
            status: "blocker",
            msg: "Needs explicit rejection",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    let cmd1 = Command::RejectHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: None,
        reason: "Not needed".into(),
        idempotency_key: "idem-key-1".into(),
    };

    let actor = ControlActor::test_local();
    let res1 = dispatch_control_command(&mut conn, &actor, &cmd1);
    assert!(res1.is_ok());
    // Rejection must close the entire delivery workflow, not just hide the
    // approval while its task/run remain active forever.
    let snapshot = load_system_snapshot(&conn).unwrap();
    assert!(!snapshot
        .work
        .tasks
        .iter()
        .any(|task| task.id == workflow.task_id));
    assert!(!snapshot
        .now
        .active_runs
        .iter()
        .any(|run| run.run_id == workflow.agent_run_id));
    let states: (String, String, String, String) = conn
        .query_row(
            "SELECT ap.status, a.status, r.status, t.status
         FROM cp_approvals ap JOIN cp_artifacts a ON a.id = ap.artifact_id
         JOIN cp_agent_runs r ON r.id = ap.agent_run_id JOIN cp_tasks t ON t.id = ap.task_id
         WHERE ap.id = ?1",
            params![workflow.approval_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        states,
        (
            "rejected".into(),
            "rejected".into(),
            "cancelled".into(),
            "cancelled".into()
        )
    );

    // Same key + same payload -> OK cached
    let res2 = dispatch_control_command(&mut conn, &actor, &cmd1);
    assert!(res2.is_ok());

    // Same key + different payload -> Error (invalid input / collision)
    let cmd2 = Command::RejectHandoff {
        approval_id: "app-different".into(),
        expected_revision: None,
        reason: "Different reason".into(),
        idempotency_key: "idem-key-1".into(),
    };
    let res3 = dispatch_control_command(&mut conn, &actor, &cmd2);
    assert!(res3.is_err());
    assert_eq!(res3.unwrap_err().code, "INVALID_INPUT");
}

#[test]
fn handoff_command_updates_snapshot_and_audit_atomically() {
    let _db = raios_core::test_support::IsolatedDb::new();
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let workflow = raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: "/tmp/control-plane-flow",
            from_agent: "codex_kaira",
            to_agent: "antigravity_kaira",
            status: "blocker",
            msg: "Ready for human approval",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    let before = load_system_snapshot(&conn).unwrap();
    let listed = before
        .now
        .approvals
        .iter()
        .find(|approval| approval.id == workflow.approval_id)
        .expect("pending approval must be listed");
    // The surface can only present (and decide) what carries its project root, both
    // agents and the revision the decision must echo back.
    assert_eq!(
        listed.project_path.as_deref(),
        Some("/tmp/control-plane-flow")
    );
    assert_eq!(listed.origin_agent, "codex_kaira");
    assert_eq!(listed.target_agent, "antigravity_kaira");
    assert_eq!(listed.revision, 1);

    let command = Command::ApproveHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: None,
        idempotency_key: "approve-control-plane-flow".into(),
    };
    let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &command)
        .unwrap()
        .unwrap();
    assert_eq!(result["status"], "approved");

    let after = load_system_snapshot(&conn).unwrap();
    assert!(!after
        .now
        .approvals
        .iter()
        .any(|approval| approval.id == workflow.approval_id));

    // The owner's approval is consent, not delivery: the run leaves the queue, but the
    // task stays open and the receipt must not read "delivered by the target".
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            params![workflow.task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "awaiting_approval"
    );
    assert_eq!(
        conn.query_row(
            "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
            params![workflow.agent_run_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "handover_approved_by_owner"
    );
    let receipt =
        raios_core::db::cp_handoff_receipt(&conn, &workflow.approval_id, "/tmp/control-plane-flow")
            .unwrap()
            .expect("receipt");
    assert_eq!(receipt.status, "approved");
    assert!(!receipt.delivered_by_target());
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 2);

    // Only the addressed agent's own delivery closes the task and flips the receipt.
    assert!(raios_core::db::cp_mark_handoff_delivered(
        &conn,
        &workflow.approval_id,
        "antigravity_kaira"
    )
    .unwrap());
    let delivered =
        raios_core::db::cp_handoff_receipt(&conn, &workflow.approval_id, "/tmp/control-plane-flow")
            .unwrap()
            .expect("receipt");
    assert_eq!(delivered.status, "approved");
    assert!(delivered.delivered_by_target());
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            params![workflow.task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "completed"
    );
    // A run that already ended by owner approval keeps that reason: delivery is recorded
    // on the approval, artifact and task, never by re-labelling the run.
    assert_eq!(
        conn.query_row(
            "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
            params![workflow.agent_run_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "handover_approved_by_owner"
    );
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 3);
}

#[test]
fn stale_expected_revision_is_refused_with_conflict() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let workflow = raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: "/tmp/control-plane-conflict",
            from_agent: "codex_kaira",
            to_agent: "antigravity_kaira",
            status: "blocker",
            msg: "Conflict coverage",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();
    let actor = ControlActor::test_local();

    // A decision on a revision the row never had is refused without touching the row.
    let stale = Command::RejectHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: Some(99),
        reason: "stale decision".into(),
        idempotency_key: "conflict-stale".into(),
    };
    let error = dispatch_control_command(&mut conn, &actor, &stale).unwrap_err();
    assert_eq!(error.code, "CONFLICT");
    let status: String = conn
        .query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![workflow.approval_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "pending");

    // The revision read before the decision is accepted once.
    let fresh = Command::ApproveHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: Some(1),
        idempotency_key: "conflict-fresh".into(),
    };
    let result = dispatch_control_command(&mut conn, &actor, &fresh)
        .unwrap()
        .unwrap();
    assert_eq!(result["status"], "approved");

    // Re-deciding on the same, now stale revision is a conflict, not a silent success.
    let again = Command::ApproveHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: Some(1),
        idempotency_key: "conflict-again".into(),
    };
    let error = dispatch_control_command(&mut conn, &actor, &again).unwrap_err();
    assert_eq!(error.code, "CONFLICT");

    // A named revision that matches no approval at all stays `NOT_FOUND`.
    let missing = Command::RejectHandoff {
        approval_id: "no-such-approval".into(),
        expected_revision: Some(1),
        reason: "nowhere".into(),
        idempotency_key: "conflict-missing".into(),
    };
    let error = dispatch_control_command(&mut conn, &actor, &missing).unwrap_err();
    assert_eq!(error.code, "NOT_FOUND");
}

/// Drains the typed-event bus for `expected` messages matching `matching`.
///
/// `emit` sends synchronously inside dispatch, so by the time a dispatch
/// call returns its events are already queued. Other tests may dispatch
/// control-plane commands concurrently on the same process-wide bus, hence
/// the filter.
fn collect_typed_events(
    rx: &mut tokio::sync::broadcast::Receiver<String>,
    expected: usize,
    matching: impl Fn(&serde_json::Value) -> bool,
) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    while found.len() < expected {
        match rx.try_recv() {
            Ok(msg) => {
                let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
                if matching(&value) {
                    found.push(value);
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                panic!(
                    "expected {expected} matching typed events, got {}: {found:?}",
                    found.len()
                );
            }
            Err(error) => panic!("typed event bus receive failed: {error}"),
        }
    }
    found
}

#[test]
fn handoff_lifecycle_emits_typed_events_only_after_commit() {
    // The typed bus is process-wide; tests that install it serialize on
    // the crate's shared global-state lock.
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(64);
    crate::events::install(&tx);

    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();

    // 1. A queued handoff publishes the approval request plus the run's
    // observed creation state. The dispatcher requires an absolute path;
    // a POSIX literal like `/workspace/...` is not `Path::is_absolute` on
    // Windows, so the fixture comes from the platform temp root.
    let fixture_path = std::env::temp_dir()
        .join("raios-typed-events-fixture")
        .to_string_lossy()
        .into_owned();
    let create = Command::CreateHandoff {
        project_path: fixture_path,
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        summary: "Typed event coverage".into(),
        idempotency_key: "typed-events-create".into(),
    };
    let created = dispatch_control_command(&mut conn, &ControlActor::test_local(), &create)
        .unwrap()
        .unwrap();
    let approval_id = created["approval_id"].as_str().unwrap().to_owned();
    let run_id = created["agent_run_id"].as_str().unwrap().to_owned();

    let own_ids = |value: &serde_json::Value| {
        value["payload"]["approval_id"].as_str() == Some(approval_id.as_str())
            || value["payload"]["run_id"].as_str() == Some(run_id.as_str())
    };
    let created_events = collect_typed_events(&mut rx, 2, own_ids);
    let requested = created_events
        .iter()
        .find(|v| v["event_type"] == "ApprovalRequested")
        .expect("ApprovalRequested must be emitted");
    assert_eq!(requested["payload"]["kind"], "handover");
    assert_eq!(requested["payload"]["target"], "codex_kaira");
    assert!(requested["payload"]["title"]
        .as_str()
        .unwrap()
        .contains("Handoff"));
    let run_created = created_events
        .iter()
        .find(|v| v["event_type"] == "AgentRunStateChanged")
        .expect("run creation must be emitted");
    assert_eq!(run_created["payload"]["agent_name"], "claude_kaira");
    let db_status: String = conn
        .query_row(
            "SELECT status FROM cp_agent_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        run_created["payload"]["status"].as_str(),
        Some(db_status.as_str()),
        "the emitted run status must mirror the committed row"
    );

    // 2. Approving publishes the resolution plus the run's observed end
    // state (never an assumed constant).
    let approve = Command::ApproveHandoff {
        approval_id: approval_id.clone(),
        expected_revision: None,
        idempotency_key: "typed-events-approve".into(),
    };
    dispatch_control_command(&mut conn, &ControlActor::test_local(), &approve)
        .unwrap()
        .unwrap();
    let resolved_events = collect_typed_events(&mut rx, 2, own_ids);
    let resolved = resolved_events
        .iter()
        .find(|v| v["event_type"] == "ApprovalResolved")
        .expect("ApprovalResolved must be emitted");
    assert_eq!(resolved["payload"]["approval_id"], approval_id);
    assert_eq!(resolved["payload"]["status"], "approved");
    let run_ended = resolved_events
        .iter()
        .find(|v| v["event_type"] == "AgentRunStateChanged")
        .expect("run end must be emitted");
    let db_status: String = conn
        .query_row(
            "SELECT status FROM cp_agent_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        run_ended["payload"]["status"].as_str(),
        Some(db_status.as_str())
    );

    // 3. An idempotent replay returns the cached result and must not emit
    // the same transition a second time.
    dispatch_control_command(&mut conn, &ControlActor::test_local(), &approve)
        .unwrap()
        .unwrap();
    while let Ok(msg) = rx.try_recv() {
        let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert!(
            !own_ids(&value),
            "a replayed command must not re-emit its events: {value:?}"
        );
    }
}

#[test]
fn remote_principal_cannot_resolve_local_owned_approval() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let workflow = raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path: "/tmp/control-plane-owner",
            from_agent: "codex_kaira",
            to_agent: "antigravity_kaira",
            status: "blocker",
            msg: "Owner-bound approval",
            diff_stat: None,
            report: None,
        },
    )
    .unwrap();

    let command = Command::ApproveHandoff {
        approval_id: workflow.approval_id.clone(),
        expected_revision: None,
        idempotency_key: "remote-owner-check".into(),
    };
    let error = dispatch_control_command(
        &mut conn,
        &ControlActor::remote_session("remote_api_key"),
        &command,
    )
    .unwrap_err();
    assert_eq!(error.code, "UNAUTHORIZED");
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_approvals WHERE id = ?1",
            params![workflow.approval_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "pending"
    );
}

#[test]
fn launch_agent_rejects_unknown_identity_before_terminal_spawn() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();

    let unsupported = Command::LaunchAgent {
        agent_name: "codex; touch /tmp/pwned".into(),
        prompt: Some("Do something".into()),
        project_path: "/tmp".into(),
        idempotency_key: "idem-invalid-agent".into(),
    };

    let res = dispatch_control_command(&mut conn, &ControlActor::test_local(), &unsupported);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().code, "INVALID_INPUT");
}

#[test]
fn launch_validation_bounds_identity_and_prompt_without_spawning() {
    assert_eq!(validate_agent_name(" AGY ").unwrap(), "antigravity");
    assert!(validate_agent_name("codex; touch /tmp/pwned").is_err());
    assert_eq!(
        validate_agent_prompt(Some("  inspect the project ")).unwrap(),
        Some("inspect the project".into())
    );
    assert!(validate_agent_prompt(Some("--dangerous-extra-flag")).is_err());
    assert!(validate_agent_prompt(Some(&"x".repeat(MAX_AGENT_PROMPT_CHARS + 1))).is_err());
}

#[test]
fn tui_personal_task_create_and_status_update_are_transactional() {
    let _db = raios_core::test_support::IsolatedDb::new();
    let project_path = if cfg!(windows) {
        r"C:\workspace\raios"
    } else {
        "/workspace/raios"
    };
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, path) VALUES ('R-AI-OS', ?1)",
        params![project_path],
    )
    .unwrap();

    let create = Command::CreateTask {
        title: "Build WORK task composer".into(),
        project_path: Some(project_path.into()),
        priority: 180,
        idempotency_key: "create-tui-task".into(),
    };
    let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &create)
        .unwrap()
        .unwrap();
    let task_id = result["task_id"].as_str().unwrap().to_owned();
    assert_eq!(result["status"], "queued");

    let snapshot = load_work_snapshot(&conn).unwrap();
    let task = snapshot
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.project_path.as_deref(), Some(project_path));
    assert_eq!(task.priority, 180);

    let update = Command::UpdateTaskStatus {
        task_id: task_id.clone(),
        status: "in_progress".into(),
        idempotency_key: "update-tui-task".into(),
    };
    let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &update)
        .unwrap()
        .unwrap();
    assert_eq!(result["status"], "in_progress");
    assert_eq!(
        conn.query_row(
            "SELECT status FROM cp_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "in_progress"
    );
}

#[test]
fn task_commands_reject_invalid_but_allow_canonical_personal_task_mutations() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let invalid_create = Command::CreateTask {
        title: " ".into(),
        project_path: None,
        priority: 50,
        idempotency_key: "empty-tui-task".into(),
    };
    assert_eq!(
        dispatch_control_command(&mut conn, &ControlActor::test_local(), &invalid_create)
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );

    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO cp_tasks
         (id, plan_id, parent_task_id, title, description, priority, status,
          acceptance_criteria, created_at, updated_at)
         VALUES ('markdown-task', NULL, NULL, 'Legacy task', '', 50, 'queued', '', ?1, ?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_task_list_items
         (task_id, source_kind, source_path, display_order, created_at)
         VALUES ('markdown-task', 'markdown', 'tasks.md', 0, ?1)",
        params![now],
    )
    .unwrap();
    let markdown_update = Command::UpdateTaskStatus {
        task_id: "markdown-task".into(),
        status: "completed".into(),
        idempotency_key: "complete-markdown-task".into(),
    };
    assert_eq!(
        dispatch_control_command(&mut conn, &ControlActor::test_local(), &markdown_update)
            .unwrap()
            .unwrap()["status"],
        "completed"
    );
}

fn new_operator_task(conn: &mut Connection, key: &str) -> String {
    let create = Command::CreateTask {
        title: "Assign me".into(),
        project_path: None,
        priority: 50,
        idempotency_key: key.into(),
    };
    let result = dispatch_control_command(conn, &ControlActor::test_local(), &create)
        .unwrap()
        .unwrap();
    result["task_id"].as_str().unwrap().to_owned()
}

fn assign(
    conn: &mut Connection,
    key: &str,
    task_id: &str,
    assignee: Option<&str>,
    expected: Option<&str>,
) -> Result<Option<serde_json::Value>, Problem> {
    dispatch_control_command(
        conn,
        &ControlActor::test_local(),
        &Command::AssignTask {
            task_id: task_id.into(),
            assignee: assignee.map(str::to_owned),
            expected_updated_at: expected.map(str::to_owned),
            idempotency_key: key.into(),
        },
    )
}

fn stored(conn: &Connection, task_id: &str) -> (Option<String>, Option<String>, String) {
    conn.query_row(
        "SELECT assignee_kind, assignee_id, updated_at FROM cp_tasks WHERE id = ?1",
        params![task_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .unwrap()
}

#[test]
fn assigning_a_task_sets_and_clears_the_agent_and_shows_in_the_snapshot() {
    let _db = raios_core::test_support::IsolatedDb::new();
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let task_id = new_operator_task(&mut conn, "assign-create");

    let result = assign(&mut conn, "assign-1", &task_id, Some("codex"), None)
        .unwrap()
        .unwrap();
    assert_eq!(result["assignee"], "codex");
    let (kind, id, updated) = stored(&conn, &task_id);
    assert_eq!(
        (kind.as_deref(), id.as_deref()),
        (Some("agent"), Some("codex"))
    );
    assert_eq!(result["updated_at"], updated.as_str());

    let snapshot = load_work_snapshot(&conn).unwrap();
    let task = snapshot.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.assignee.as_deref(), Some("codex"));
    assert_eq!(
        task.updated_at, updated,
        "the snapshot carries the revision token"
    );

    assign(&mut conn, "assign-2", &task_id, None, Some(&updated)).unwrap();
    let (kind, id, _) = stored(&conn, &task_id);
    assert_eq!((kind, id), (None, None), "None clears the assignee");
}

#[test]
fn assigning_refuses_unknown_agents_missing_tasks_and_a_stale_revision() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let task_id = new_operator_task(&mut conn, "assign-create");
    let (_, _, first_revision) = stored(&conn, &task_id);

    for bad in ["", "claude_kaira", "cursor", "claude; drop"] {
        assert_eq!(
            assign(&mut conn, &format!("bad-{bad}"), &task_id, Some(bad), None)
                .unwrap_err()
                .code,
            "INVALID_INPUT",
            "{bad:?}"
        );
    }
    assert_eq!(
        assign(&mut conn, "blank-id", " ", Some("claude"), None)
            .unwrap_err()
            .code,
        "INVALID_INPUT"
    );
    assert_eq!(
        assign(&mut conn, "missing", "no-such-task", Some("claude"), None)
            .unwrap_err()
            .code,
        "NOT_FOUND"
    );
    assert_eq!(stored(&conn, &task_id).1, None, "nothing was assigned");

    // Someone else changes the task; the stale token is refused and nothing is overwritten.
    let status = Command::UpdateTaskStatus {
        task_id: task_id.clone(),
        status: "in_progress".into(),
        idempotency_key: "status-in-between".into(),
    };
    dispatch_control_command(&mut conn, &ControlActor::test_local(), &status).unwrap();
    let error = assign(
        &mut conn,
        "stale",
        &task_id,
        Some("claude"),
        Some(&first_revision),
    )
    .unwrap_err();
    assert_eq!(error.code, "CONFLICT");
    assert_eq!(stored(&conn, &task_id).1, None);

    let (_, _, fresh) = stored(&conn, &task_id);
    assign(&mut conn, "fresh", &task_id, Some("claude"), Some(&fresh)).unwrap();
    assert_eq!(stored(&conn, &task_id).1.as_deref(), Some("claude"));
}

#[test]
fn assigning_leaves_agent_owned_tasks_alone_and_replays_by_idempotency_key() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let now = Utc::now().to_rfc3339();
    // A task an approval hangs on belongs to the agent workflow, not to the operator.
    conn.execute(
        "INSERT INTO cp_tasks
         (id, plan_id, parent_task_id, title, description, priority, status,
          acceptance_criteria, created_at, updated_at)
         VALUES ('agent-task', NULL, NULL, 'Agent work', '', 50, 'awaiting_approval', '', ?1, ?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_task_list_items
         (task_id, source_kind, source_path, display_order, created_at)
         VALUES ('agent-task', 'markdown', 'tasks.md', 0, ?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_approvals (id, task_id, approval_type, reason, status, requested_at)
         VALUES ('ap-1', 'agent-task', 'handover', 'x', 'pending', ?1)",
        params![now],
    )
    .unwrap();
    assert_eq!(
        assign(&mut conn, "agent-owned", "agent-task", Some("claude"), None)
            .unwrap_err()
            .code,
        "NOT_FOUND"
    );

    let task_id = new_operator_task(&mut conn, "assign-create");
    let first = assign(&mut conn, "same-key", &task_id, Some("opencode"), None).unwrap();
    let replay = assign(&mut conn, "same-key", &task_id, Some("opencode"), None).unwrap();
    assert_eq!(first, replay, "a retry returns the stored result");
    // The same key with another payload is a conflict, never a second change.
    assert!(assign(&mut conn, "same-key", &task_id, Some("claude"), None).is_err());
    assert_eq!(stored(&conn, &task_id).1.as_deref(), Some("opencode"));
}

// ─── cron commands (characterization: pinned before the dispatcher was split) ─

fn new_cron_job(conn: &Connection) -> String {
    // Far-future first run, so "run now" is observable as a change.
    raios_core::db::cp_scheduled_job_create(conn, "nightly", "claude", "check things", 86_400)
        .unwrap()
}

fn cron_row(conn: &Connection, job_id: &str) -> (String, bool) {
    conn.query_row(
        "SELECT status, next_run_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         FROM cp_scheduled_jobs WHERE id = ?1",
        params![job_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

fn trigger(
    conn: &mut Connection,
    key: &str,
    job_id: &str,
) -> Result<Option<serde_json::Value>, Problem> {
    dispatch_control_command(
        conn,
        &ControlActor::test_local(),
        &Command::TriggerCronJob {
            job_id: job_id.into(),
            idempotency_key: key.into(),
        },
    )
}

fn toggle(
    conn: &mut Connection,
    key: &str,
    job_id: &str,
    paused: bool,
) -> Result<Option<serde_json::Value>, Problem> {
    dispatch_control_command(
        conn,
        &ControlActor::test_local(),
        &Command::ToggleCronJob {
            job_id: job_id.into(),
            paused,
            idempotency_key: key.into(),
        },
    )
}

#[test]
fn triggering_a_cron_job_makes_it_due_now() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let job = new_cron_job(&conn);
    assert!(!cron_row(&conn, &job).1, "fixture: not due yet");

    let result = trigger(&mut conn, "trigger-1", &job).unwrap().unwrap();

    assert_eq!(result["job_id"], job.as_str());
    assert_eq!(result["status"], "scheduled_now");
    let (status, due) = cron_row(&conn, &job);
    assert_eq!(
        status, "active",
        "triggering does not change the job's status"
    );
    assert!(due, "next_run_at moved to now");
}

#[test]
fn toggling_a_cron_job_pauses_and_resumes_it() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let job = new_cron_job(&conn);

    let paused = toggle(&mut conn, "pause-1", &job, true).unwrap().unwrap();
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["job_id"], job.as_str());
    assert_eq!(cron_row(&conn, &job).0, "paused");

    let resumed = toggle(&mut conn, "resume-1", &job, false).unwrap().unwrap();
    assert_eq!(resumed["status"], "active");
    assert_eq!(cron_row(&conn, &job).0, "active");
}

#[test]
fn cron_commands_refuse_unknown_and_deleted_jobs_and_leave_no_idempotency_row() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let deleted = new_cron_job(&conn);
    raios_core::db::cp_scheduled_job_delete(&conn, &deleted).unwrap();

    for (label, job) in [("unknown", "no-such-job"), ("deleted", deleted.as_str())] {
        let err = trigger(&mut conn, &format!("t-{label}"), job).unwrap_err();
        assert_eq!(err.code, "NOT_FOUND", "trigger {label}");
        assert_eq!(err.message, "Scheduled job not found", "trigger {label}");
        let err = toggle(&mut conn, &format!("p-{label}"), job, true).unwrap_err();
        assert_eq!(err.code, "NOT_FOUND", "toggle {label}");
    }

    // A refused command rolled back: its key is free to be used again.
    let used: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cp_idempotency WHERE idempotency_key IN ('t-unknown','p-unknown','t-deleted','p-deleted')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(used, 0);
}

#[test]
fn cron_commands_replay_by_idempotency_key_without_acting_twice() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let job = new_cron_job(&conn);

    let first = toggle(&mut conn, "same-key", &job, true).unwrap();
    // Un-pause behind the dispatcher's back; a replay must not re-run the command.
    raios_core::db::cp_scheduled_job_set_status(&conn, &job, "active").unwrap();
    let replay = toggle(&mut conn, "same-key", &job, true).unwrap();

    assert_eq!(first, replay, "the cached result is returned");
    assert_eq!(
        cron_row(&conn, &job).0,
        "active",
        "the command was not applied again"
    );
}
