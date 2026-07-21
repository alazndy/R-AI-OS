use super::*;

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
    let got = cp_get_provider_capabilities(&conn, "claude")
        .unwrap()
        .unwrap();
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
    cp_seed_provider_capabilities(&conn).unwrap();
    conn.execute(
        "UPDATE cp_provider_capabilities SET supports_tool_calling=0 WHERE provider='claude'",
        [],
    )
    .unwrap();
    cp_seed_provider_capabilities(&conn).unwrap();
    let got = cp_get_provider_capabilities(&conn, "claude")
        .unwrap()
        .unwrap();
    assert!(
        !got.supports_tool_calling,
        "seed should not overwrite existing rows"
    );
}

#[test]
fn cp_failure_kind_classify() {
    assert_eq!(
        ProviderFailureKind::classify("401 unauthorized"),
        ProviderFailureKind::Auth
    );
    assert_eq!(
        ProviderFailureKind::classify("rate limit exceeded 429"),
        ProviderFailureKind::Quota
    );
    assert_eq!(
        ProviderFailureKind::classify("connection timed out"),
        ProviderFailureKind::Timeout
    );
    assert_eq!(
        ProviderFailureKind::classify("sandbox permission denied"),
        ProviderFailureKind::Sandbox
    );
    assert_eq!(
        ProviderFailureKind::classify("invalid tool call argument"),
        ProviderFailureKind::ToolError
    );
    assert_eq!(
        ProviderFailureKind::classify("service unavailable 503"),
        ProviderFailureKind::ProviderUnavailable
    );
    assert!(matches!(
        ProviderFailureKind::classify("some weird error"),
        ProviderFailureKind::Unknown(_)
    ));
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
