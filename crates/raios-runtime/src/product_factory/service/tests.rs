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
    let _db = raios_core::test_support::IsolatedDb::new();
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

// ─── full lifecycle transcript (characterization of every command) ──────────
//
// Drives the factory through its whole lifecycle and pins every command's
// result - or its refusal - so the dispatcher can be reorganised without
// changing behaviour. Generated ids and timestamps are normalised.

fn normalize(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    let is_uuid = |s: &str| {
        s.len() == 36
            && s.char_indices().all(|(i, c)| match i {
                8 | 13 | 18 | 23 => c == '-',
                _ => c.is_ascii_hexdigit(),
            })
    };
    match value {
        Value::String(s) if is_uuid(s) => Value::String("<id>".into()),
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let v = if k == "oldest_open_created_at" {
                        Value::String("<timestamp>".into())
                    } else {
                        normalize(v)
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

struct Lifecycle {
    conn: Connection,
    actor: ControlActor,
    transcript: Vec<String>,
}

impl Lifecycle {
    fn run(&mut self, label: &str, command: FactoryCommand) -> serde_json::Value {
        let outcome = dispatch_factory_command(&mut self.conn, &self.actor, true, &command);
        let (line, value) = match outcome {
            Ok(value) => (format!("{label}: OK {}", normalize(&value)), value),
            Err(problem) => (
                format!("{label}: ERR {} / {}", problem.code, problem.message),
                serde_json::Value::Null,
            ),
        };
        self.transcript.push(line);
        value
    }
}

fn text(value: &serde_json::Value, field: &str) -> String {
    value[field].as_str().unwrap_or("<missing>").to_owned()
}

fn lifecycle_transcript() -> String {
    let _db = raios_core::test_support::IsolatedDb::new();
    let conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    let mut l = Lifecycle {
        conn,
        actor: ControlActor::local_session(),
        transcript: Vec::new(),
    };
    let key = |name: &str| format!("{name}-key-0001");

    let ws = l.run(
        "workspace",
        FactoryCommand::CreateWorkspace {
            name: "W".into(),
            idempotency_key: key("ws"),
        },
    );
    let p = l.run(
        "product",
        FactoryCommand::CreateProductDraft {
            workspace_id: text(&ws, "workspace_id"),
            title: "P".into(),
            idempotency_key: key("prod"),
        },
    );
    let pid = text(&p, "product_id");
    l.run(
        "mode",
        FactoryCommand::SetProductMode {
            product_id: pid.clone(),
            mode: raios_contracts::FactoryMode::Quick,
            idempotency_key: key("mode"),
        },
    );
    let intake = l.run(
        "intake",
        FactoryCommand::StartIntake {
            product_id: pid.clone(),
            idempotency_key: key("intake"),
        },
    );
    let sid = text(&intake, "session_id");
    for (i, answer) in ["problem_statement", "core_outcome", "success_metric"]
        .iter()
        .enumerate()
    {
        l.run(
            "answer",
            FactoryCommand::RecordIntakeAnswer {
                session_id: sid.clone(),
                question_key: (*answer).into(),
                response: format!("A{i}"),
                idempotency_key: format!("ans-key-{i:04}"),
            },
        );
    }
    l.run(
        "charter",
        FactoryCommand::GenerateCharterDraft {
            product_id: pid.clone(),
            idempotency_key: key("charter"),
        },
    );
    let req = l.run(
        "requirement",
        FactoryCommand::CreateRequirementDraft {
            product_id: pid.clone(),
            stable_key: "req_one".into(),
            content: "Do X".into(),
            idempotency_key: key("req"),
        },
    );
    let reqid = text(&req, "requirement_id");
    let cr = l.run(
        "submit_change_request",
        FactoryCommand::SubmitChangeRequest {
            product_id: pid.clone(),
            summary: "Change it".into(),
            idempotency_key: key("cr"),
        },
    );
    let crid = text(&cr, "change_request_id");
    let asmt = l.run(
        "assess_change_request",
        FactoryCommand::AssessChangeRequest {
            change_request_id: crid.clone(),
            idempotency_key: key("assess"),
        },
    );
    let aid = text(&asmt, "assessment_id");
    l.run(
        "resolve_impact",
        FactoryCommand::ResolveImpactAssessment {
            assessment_id: aid.clone(),
            approved: true,
            idempotency_key: key("resolve"),
        },
    );
    l.run(
        "apply_change",
        FactoryCommand::ApplyApprovedRequirementChange {
            assessment_id: aid,
            requirement_id: reqid.clone(),
            content: "Do X better".into(),
            idempotency_key: key("apply"),
        },
    );
    let plan = l.run(
        "plan_draft",
        FactoryCommand::CreatePlanDraft {
            product_id: pid.clone(),
            title: "Plan".into(),
            idempotency_key: key("plan"),
        },
    );
    let planid = text(&plan, "plan_id");
    l.run(
        "approve_plan",
        FactoryCommand::ApprovePlan {
            plan_id: planid.clone(),
            idempotency_key: key("appr"),
        },
    );
    let cyc = l.run(
        "materialize_cycle",
        FactoryCommand::MaterializePlannedCycle {
            plan_id: planid,
            idempotency_key: key("cyc"),
        },
    );
    let cid = text(&cyc, "cycle_id");
    l.run(
        "pause_cycle",
        FactoryCommand::PauseCycle {
            cycle_id: cid.clone(),
            idempotency_key: key("pause"),
        },
    );
    l.run(
        "resume_cycle",
        FactoryCommand::ResumeCycle {
            cycle_id: cid.clone(),
            idempotency_key: key("resume"),
        },
    );
    l.run(
        "task_graph",
        FactoryCommand::MaterializeStageTaskGraph {
            cycle_id: cid.clone(),
            stage: "discover".into(),
            idempotency_key: key("tg"),
        },
    );
    l.run(
        "activate_stage",
        FactoryCommand::ActivateApprovedStage {
            cycle_id: cid.clone(),
            stage: "discover".into(),
            idempotency_key: key("act"),
        },
    );
    let ev = l.run(
        "stage_evidence",
        FactoryCommand::RecordStageEvidence {
            cycle_id: cid.clone(),
            stage: "discover".into(),
            content_ref: "notes/discovery.md".into(),
            idempotency_key: key("ev"),
        },
    );
    l.run(
        "link_evidence",
        FactoryCommand::LinkStageEvidenceToRequirement {
            evidence_id: text(&ev, "evidence_id"),
            requirement_id: reqid,
            idempotency_key: key("lev"),
        },
    );
    l.run(
        "complete_stage",
        FactoryCommand::CompleteStage {
            cycle_id: cid.clone(),
            stage: "discover".into(),
            idempotency_key: key("cs"),
        },
    );
    let qp = l.run(
        "quality_profile",
        FactoryCommand::CreateQualityProfile {
            product_id: pid.clone(),
            name: "Gate".into(),
            required: true,
            idempotency_key: key("qp"),
        },
    );
    l.run(
        "react_native_profile",
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile {
            product_id: pid.clone(),
            idempotency_key: key("rn"),
        },
    );
    l.run(
        "quality_check",
        FactoryCommand::RecordQualityCheck {
            profile_id: text(&qp, "profile_id"),
            passed: true,
            evidence_ref: "ci/run-1".into(),
            idempotency_key: key("qc"),
        },
    );
    let rel = l.run(
        "release_draft",
        FactoryCommand::CreateReleaseDraft {
            product_id: pid.clone(),
            build_ref: "build-1".into(),
            idempotency_key: key("rel"),
        },
    );
    l.run(
        "release_readiness",
        FactoryCommand::InspectReleaseReadiness {
            product_id: pid.clone(),
            idempotency_key: key("ready"),
        },
    );
    l.run(
        "approve_release",
        FactoryCommand::ApproveClosedTestingRelease {
            release_id: text(&rel, "release_id"),
            idempotency_key: key("arel"),
        },
    );
    let sup = l.run(
        "support_item",
        FactoryCommand::CreateSupportItem {
            product_id: pid.clone(),
            source_kind: "user_report".into(),
            summary: "It broke".into(),
            idempotency_key: key("sup"),
        },
    );
    let supid = text(&sup, "support_item_id");
    l.run(
        "support_overview",
        FactoryCommand::InspectSupportOverview {
            product_id: pid,
            idempotency_key: key("sov"),
        },
    );
    l.run(
        "triage_support",
        FactoryCommand::TriageSupportItem {
            support_item_id: supid.clone(),
            idempotency_key: key("tri"),
        },
    );
    l.run(
        "link_support",
        FactoryCommand::LinkSupportToChangeRequest {
            support_item_id: supid.clone(),
            change_request_id: crid.clone(),
            idempotency_key: key("lsup"),
        },
    );
    l.run(
        "resolve_support",
        FactoryCommand::ResolveSupportItem {
            support_item_id: supid,
            resolution_ref: "fix-1".into(),
            idempotency_key: key("rsup"),
        },
    );
    l.run(
        "request_impact",
        FactoryCommand::RequestImpactAssessment {
            change_request_id: crid,
            idempotency_key: key("rimp"),
        },
    );
    l.run(
        "cancel_cycle",
        FactoryCommand::CancelCycle {
            cycle_id: cid,
            idempotency_key: key("cancel"),
        },
    );
    l.transcript.join("\n")
}

#[test]
fn factory_lifecycle_results_are_pinned() {
    let actual = lifecycle_transcript();
    // When a command's behaviour is changed on purpose, run this test with
    // `PIN_PRINT=1 cargo test factory_lifecycle_results_are_pinned -- --nocapture`
    // to print the new transcript and paste it into LIFECYCLE_TRANSCRIPT below.
    if std::env::var_os("PIN_PRINT").is_some() {
        println!("---TRANSCRIPT---\n{actual}\n---END---");
    }
    assert_eq!(actual, LIFECYCLE_TRANSCRIPT.trim());
}

const LIFECYCLE_TRANSCRIPT: &str = r#"
workspace: OK {"name":"W","workspace_id":"<id>"}
product: OK {"product_id":"<id>","title":"P","workspace_id":"<id>"}
mode: OK {"mode":"quick","product_id":"<id>"}
intake: OK {"product_id":"<id>","session_id":"<id>"}
answer: OK {"question_key":"problem_statement","session_id":"<id>","status":"answered"}
answer: OK {"question_key":"core_outcome","session_id":"<id>","status":"answered"}
answer: OK {"question_key":"success_metric","session_id":"<id>","status":"answered"}
charter: OK {"charter_revision_id":"<id>","generated":true,"product_id":"<id>","revision":1}
requirement: OK {"product_id":"<id>","requirement_id":"<id>","requirement_revision_id":"<id>","stable_key":"req_one"}
submit_change_request: OK {"change_request_id":"<id>","product_id":"<id>","status":"proposed"}
assess_change_request: OK {"affected_count":2,"assessment_id":"<id>","change_request_id":"<id>","status":"awaiting_approval"}
resolve_impact: OK {"approved":true,"assessment_id":"<id>"}
apply_change: OK {"assessment_id":"<id>","requirement_id":"<id>","requirement_revision_id":"<id>"}
plan_draft: OK {"plan_id":"<id>","product_id":"<id>","status":"planned","title":"Plan"}
approve_plan: OK {"plan_id":"<id>","status":"approved"}
materialize_cycle: OK {"created":true,"cycle_id":"<id>","plan_id":"<id>","product_id":"<id>","status":"planned"}
pause_cycle: OK {"cycle_id":"<id>","status":"paused"}
resume_cycle: OK {"cycle_id":"<id>","status":"resumed"}
task_graph: OK {"cycle_id":"<id>","execution":"disabled","stage":"discover","task_graph_id":"<id>"}
activate_stage: ERR FORBIDDEN / Stage requires an owned approved execution approval
stage_evidence: OK {"cycle_id":"<id>","evidence_id":"<id>","stage":"discover"}
link_evidence: OK {"evidence_id":"<id>","requirement_id":"<id>","status":"linked"}
complete_stage: ERR FORBIDDEN / Active owned stage requires evidence before completion
quality_profile: OK {"product_id":"<id>","profile_id":"<id>","required":true}
react_native_profile: OK {"product_id":"<id>","profile":"react_native_closed_testing","profile_ids":["<id>","<id>","<id>","<id>","<id>","<id>"]}
quality_check: OK {"check_id":"<id>","evidence_ref":"ci/run-1","passed":true,"profile_id":"<id>"}
release_draft: ERR FORBIDDEN / Owned product is not release-ready; required quality evidence is missing
release_readiness: OK {"completed_verify_stage":false,"pending_impact_assessments":0,"product_id":"<id>","ready":false,"required_quality_blockers":6,"stale_evidence_count":0}
approve_release: ERR INVALID_INPUT / Factory command contains an empty, oversized, or invalid field
support_item: OK {"status":"new","support_item_id":"<id>"}
support_overview: OK {"linked_change_count":0,"oldest_open_created_at":"<timestamp>","open_count":1,"product_id":"<id>","resolved_count":0}
triage_support: OK {"status":"triaged","support_item_id":"<id>"}
link_support: OK {"change_request_id":"<id>","relation":"raised_by","support_item_id":"<id>"}
resolve_support: OK {"resolution_ref":"fix-1","status":"resolved","support_item_id":"<id>"}
request_impact: ERR NOT_IMPLEMENTED / Impact assessment mutation is not implemented in Phase 3
cancel_cycle: OK {"cycle_id":"<id>","status":"cancelled"}
"#;
