use super::*;

#[test]
fn quick_mode_requires_only_the_compact_intake_for_a_charter() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let actor = ControlActor::local_session();
    let workspace = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateWorkspace {
            name: "Quick pilots".into(),
            idempotency_key: "quick-workspace-0001".into(),
        },
    )
    .unwrap();
    let product = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateProductDraft {
            workspace_id: workspace["workspace_id"].as_str().unwrap().into(),
            title: "Quick pilot".into(),
            idempotency_key: "quick-product-0001".into(),
        },
    )
    .unwrap();
    let product_id = product["product_id"].as_str().unwrap().to_owned();
    dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::SetProductMode {
            product_id: product_id.clone(),
            mode: raios_contracts::FactoryMode::Quick,
            idempotency_key: "quick-mode-0001".into(),
        },
    )
    .unwrap();
    let intake = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::StartIntake {
            product_id: product_id.clone(),
            idempotency_key: "quick-intake-0001".into(),
        },
    )
    .unwrap();
    let session_id = intake["session_id"].as_str().unwrap().to_owned();
    for (index, key) in ["problem_statement", "core_outcome", "success_metric"]
        .iter()
        .enumerate()
    {
        dispatch_factory_command(
            &mut conn,
            &actor,
            true,
            &FactoryCommand::RecordIntakeAnswer {
                session_id: session_id.clone(),
                question_key: (*key).into(),
                response: format!("Answer {index}"),
                idempotency_key: format!("quick-answer-{index:04}"),
            },
        )
        .unwrap();
    }
    let charter = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::GenerateCharterDraft {
            product_id,
            idempotency_key: "quick-charter-0001".into(),
        },
    )
    .unwrap();
    assert!(charter["charter_revision_id"].is_string());
}

#[test]
fn guided_intake_and_charter_are_owner_bound_and_idempotent() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let actor = ControlActor::local_session();

    let workspace = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateWorkspace {
            name: "Mobile products".into(),
            idempotency_key: "factory-ws-0001".into(),
        },
    )
    .unwrap();
    let workspace_id = workspace["workspace_id"].as_str().unwrap().to_owned();
    let product = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateProductDraft {
            workspace_id,
            title: "React Native pilot".into(),
            idempotency_key: "factory-product-0001".into(),
        },
    )
    .unwrap();
    let product_id = product["product_id"].as_str().unwrap().to_owned();
    let intake = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::StartIntake {
            product_id: product_id.clone(),
            idempotency_key: "factory-intake-0001".into(),
        },
    )
    .unwrap();
    let session_id = intake["session_id"].as_str().unwrap().to_owned();
    let answer = FactoryCommand::RecordIntakeAnswer {
        session_id: session_id.clone(),
        question_key: "target_user".into(),
        response: "Independent builders".into(),
        idempotency_key: "factory-answer-0001".into(),
    };
    let first = dispatch_factory_command(&mut conn, &actor, true, &answer).unwrap();
    let second = dispatch_factory_command(&mut conn, &actor, true, &answer).unwrap();
    assert_eq!(first, second);

    let blocked = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateCharterDraft {
            product_id: product_id.clone(),
            content: "Pilot outcome: reach closed testing safely.".into(),
            idempotency_key: "factory-charter-blocked-0001".into(),
        },
    )
    .unwrap_err();
    assert_eq!(blocked.code, "INVALID_INPUT");
    assert!(blocked.message.contains("problem_statement"));

    for (index, key) in [
        "problem_statement",
        "core_outcome",
        "first_platform",
        "success_metric",
    ]
    .iter()
    .enumerate()
    {
        dispatch_factory_command(
            &mut conn,
            &actor,
            true,
            &FactoryCommand::RecordIntakeAnswer {
                session_id: session_id.clone(),
                question_key: (*key).into(),
                response: format!("Answer {index}"),
                idempotency_key: format!("factory-answer-required-{index:04}"),
            },
        )
        .unwrap();
    }

    let charter = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateCharterDraft {
            product_id: product_id.clone(),
            content: "Pilot outcome: reach closed testing safely.".into(),
            idempotency_key: "factory-charter-0001".into(),
        },
    )
    .unwrap();
    assert_eq!(charter["revision"], 1);
    let generated = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::GenerateCharterDraft {
            product_id,
            idempotency_key: "factory-charter-generated-0001".into(),
        },
    )
    .unwrap();
    assert_eq!(generated["revision"], 2);
    assert_eq!(generated["generated"], true);
    assert_eq!(
        conn.query_row(
            "SELECT response_text FROM cp_factory_intake_items WHERE question_key = 'target_user'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "Independent builders"
    );
    assert!(conn
        .query_row(
            "SELECT content_text FROM cp_factory_charter_revisions WHERE revision = 2",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .contains("Independent builders"));
}

#[test]
fn factory_mutations_are_disabled_for_remote_or_disabled_sessions() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let command = FactoryCommand::CreateWorkspace {
        name: "Workspace".into(),
        idempotency_key: "factory-blocked-0001".into(),
    };
    assert_eq!(
        dispatch_factory_command(&mut conn, &ControlActor::local_session(), false, &command)
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    assert_eq!(
        dispatch_factory_command(
            &mut conn,
            &ControlActor::remote_session("remote"),
            true,
            &command,
        )
        .unwrap_err()
        .code,
        "UNAUTHORIZED"
    );
}

#[test]
fn factory_rejects_probable_secrets_before_persistence() {
    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let error = dispatch_factory_command(
        &mut conn,
        &ControlActor::local_session(),
        true,
        &FactoryCommand::CreateWorkspace {
            name: "password = 'sup3rSecretValue123'".into(),
            idempotency_key: "factory-secret-0001".into(),
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_INPUT");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM cp_workspaces", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn factory_stage_validation_accepts_only_canonical_lifecycle_stages() {
    assert!(is_factory_stage("discover"));
    assert!(is_factory_stage("support"));
    assert!(!is_factory_stage("publish_to_store"));
    assert!(!is_factory_stage("verify; rm -rf"));
}

#[test]
fn scaffold_project_rejects_pre_existing_nonempty_directory_and_writes_raios_yaml() {
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = raios_core::config::Config::load().unwrap_or_default();
    cfg.dev_ops_path = temp.path().to_path_buf();

    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let actor = ControlActor::local_session();

    let ws = dispatch_factory_command_with_config(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateWorkspace {
            name: "Scaffold WS".into(),
            idempotency_key: "scaffold-ws-0001".into(),
        },
        Some(&cfg),
    )
    .unwrap();

    let prod = dispatch_factory_command_with_config(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateProductDraft {
            workspace_id: ws["workspace_id"].as_str().unwrap().into(),
            title: "Scaffold App".into(),
            idempotency_key: "scaffold-prod-0001".into(),
        },
        Some(&cfg),
    )
    .unwrap();
    let product_id = prod["product_id"].as_str().unwrap().to_string();

    let target_dir = temp.path().join("tools").join("scaffold-app");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("existing.txt"), "pre-existing").unwrap();

    let err = dispatch_factory_command_with_config(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::ScaffoldProject {
            product_id: product_id.clone(),
            idempotency_key: "scaffold-cmd-0001".into(),
        },
        Some(&cfg),
    )
    .unwrap_err();
    assert_eq!(err.code, "INVALID_INPUT");
    assert!(err.message.contains("already exists and is non-empty"));

    // Clean pre-existing file and retry
    std::fs::remove_file(target_dir.join("existing.txt")).unwrap();
    let result = dispatch_factory_command_with_config(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::ScaffoldProject {
            product_id: product_id.clone(),
            idempotency_key: "scaffold-cmd-0002".into(),
        },
        Some(&cfg),
    )
    .unwrap();
    assert_eq!(result["created"], true);
    let project_path = result["project_path"].as_str().unwrap();
    let yaml_path = std::path::Path::new(project_path).join(".raios.yaml");
    assert!(yaml_path.is_file());
    let yaml_content = std::fs::read_to_string(yaml_path).unwrap();
    assert!(yaml_content.contains(&format!("product_id: \"{}\"", product_id)));
    assert!(yaml_content.contains("mode: \"governed\""));

    // Idempotency: second call returns created: false
    let second = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::ScaffoldProject {
            product_id: product_id.clone(),
            idempotency_key: "scaffold-cmd-0003".into(),
        },
    )
    .unwrap();
    assert_eq!(second["created"], false);
    assert_eq!(second["project_path"], project_path);
}

#[test]
fn existing_git_project_attachment_is_verified_owner_bound_and_idempotent() {
    let repository = tempfile::tempdir().unwrap();
    let repo_path = repository.path().to_string_lossy().to_string();
    run_git(&repo_path, &["init", "--quiet"]);
    run_git(
        &repo_path,
        &["config", "user.email", "factory@example.test"],
    );
    run_git(&repo_path, &["config", "user.name", "Factory Test"]);
    std::fs::write(repository.path().join("README.md"), "# Attached project\n").unwrap();
    run_git(&repo_path, &["add", "README.md"]);
    run_git(&repo_path, &["commit", "--quiet", "-m", "initial"]);
    run_git(
        &repo_path,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/attached-project.git",
        ],
    );

    let mut conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let actor = ControlActor::local_session();
    let workspace = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateWorkspace {
            name: "Attached repositories".into(),
            idempotency_key: "attach-workspace-0001".into(),
        },
    )
    .unwrap();
    let product = dispatch_factory_command(
        &mut conn,
        &actor,
        true,
        &FactoryCommand::CreateProductDraft {
            workspace_id: workspace["workspace_id"].as_str().unwrap().into(),
            title: "Attached project".into(),
            idempotency_key: "attach-product-0001".into(),
        },
    )
    .unwrap();
    let product_id = product["product_id"].as_str().unwrap().to_string();
    let command = FactoryCommand::AttachExistingProject {
        product_id: product_id.clone(),
        project_path: repo_path.clone(),
        idempotency_key: "attach-project-0001".into(),
    };

    let first = dispatch_factory_command(&mut conn, &actor, true, &command).unwrap();
    let second = dispatch_factory_command(&mut conn, &actor, true, &command).unwrap();
    assert_eq!(first, second);
    assert_eq!(first["attached"], true);
    assert_eq!(
        first["source_remote"],
        "https://github.com/example/attached-project.git"
    );
    assert_eq!(first["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(
        conn.query_row(
            "SELECT source_remote FROM cp_factory_products WHERE id=?1",
            [&product_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "https://github.com/example/attached-project.git"
    );
}

#[test]
fn credential_bearing_http_remotes_are_rejected() {
    assert!(remote_has_embedded_http_credentials(
        "https://token@github.com/example/private.git"
    ));
    assert!(!remote_has_embedded_http_credentials(
        "git@github.com:example/private.git"
    ));
}

fn run_git(project_path: &str, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}
