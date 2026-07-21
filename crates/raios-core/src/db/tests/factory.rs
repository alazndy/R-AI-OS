use super::*;
use crate::product_factory::DISCOVERY_INTAKE_PROMPTS;

#[test]
fn product_factory_skeleton_tables_exist_after_migration() {
    let conn = in_memory();
    let tables = [
        "cp_workspaces",
        "cp_plans",
        "cp_factory_products",
        "cp_factory_intake_sessions",
        "cp_factory_charter_revisions",
        "cp_factory_requirements",
        "cp_factory_requirement_revisions",
        "cp_factory_requirement_links",
        "cp_factory_decisions",
        "cp_factory_cycles",
        "cp_factory_stage_runs",
        "cp_factory_change_requests",
        "cp_factory_impact_assessments",
        "cp_factory_impact_targets",
        "cp_factory_evidence_links",
        "cp_factory_evidence_dependencies",
        "cp_factory_quality_profiles",
        "cp_factory_quality_checks",
        "cp_factory_releases",
        "cp_factory_release_channels",
        "cp_factory_support_items",
        "cp_factory_automation_policies",
        "cp_factory_integrations",
        "cp_factory_events",
    ];

    for table in tables {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "missing Product Factory table: {table}");
    }

    assert!(factory_products_schema_available(&conn).unwrap());
}

#[test]
fn factory_overview_reads_canonical_lifecycle_rows() {
    let conn = in_memory();
    conn.execute(
        "INSERT INTO cp_workspaces (id, name, owner_subject) VALUES (?1, ?2, ?3)",
        params!["workspace-1", "Workspace", "local_control_owner"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_products (id, workspace_id, owner_subject, title, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["product-1", "workspace-1", "local_control_owner", "Pilot", "active"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_plans (id, workspace_id, product_id, title, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["plan-1", "workspace-1", "product-1", "Pilot plan", "planned"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_cycles (id, product_id, plan_id, status, current_stage) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["cycle-1", "product-1", "plan-1", "active", "discover"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_change_requests (id, product_id, requested_by, status, summary) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["change-1", "product-1", "local_control_owner", "awaiting_approval", "Review"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cp_factory_support_items (id, product_id, source_kind, status) VALUES (?1, ?2, ?3, ?4)",
        params!["support-1", "product-1", "manual", "triaged"],
    )
    .unwrap();

    let overview = load_factory_overview(&conn).unwrap();
    assert_eq!(overview.product_count, 1);
    assert_eq!(overview.active_cycle_count, 1);
    assert_eq!(overview.pending_change_request_count, 1);
    assert_eq!(overview.open_support_item_count, 1);
    assert_eq!(overview.latest_product.unwrap().title, "Pilot");
}

#[test]
fn discovery_repositories_enforce_owner_bound_drafts() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    let intake = start_factory_intake(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_factory_intake_items WHERE session_id = ?1 AND status = 'open'",
            [&intake.id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        5
    );
    assert_eq!(
        missing_required_intake_prompt_keys(&tx, &product.id).unwrap(),
        vec![
            "problem_statement",
            "target_user",
            "core_outcome",
            "first_platform",
            "success_metric",
        ]
    );
    let resumed = start_factory_intake(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(resumed.id, intake.id);
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_factory_intake_items WHERE session_id = ?1",
            [&intake.id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        5
    );
    assert!(record_factory_intake_answer(
        &tx,
        "owner-a",
        &intake.id,
        "target_user",
        "Independent builders"
    )
    .unwrap());
    assert_eq!(
        missing_required_intake_prompt_keys(&tx, &product.id).unwrap(),
        vec![
            "problem_statement",
            "core_outcome",
            "first_platform",
            "success_metric",
        ]
    );
    let charter =
        create_factory_charter_draft(&tx, "owner-a", &product.id, "Pilot charter content")
            .unwrap()
            .unwrap();
    assert_eq!(charter.revision, 1);
    let requirement = create_factory_requirement_draft(
        &tx,
        "owner-a",
        &product.id,
        "closed_testing_readiness",
        "The pilot must reach closed testing with a verified build.",
    )
    .unwrap()
    .unwrap();
    assert!(
        create_factory_product_draft(&tx, "owner-b", &workspace.id, "Blocked")
            .unwrap()
            .is_none()
    );
    tx.commit().unwrap();

    assert!(factory_product_owned_by(&conn, &product.id, "owner-a").unwrap());
    assert!(!factory_intake_session_owned_by(&conn, &intake.id, "owner-b").unwrap());
    assert_eq!(
        conn.query_row(
            "SELECT content_text FROM cp_factory_charter_revisions WHERE id = ?1",
            params![charter.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "Pilot charter content"
    );
    assert_eq!(
        conn.query_row(
            "SELECT content_text FROM cp_factory_requirement_revisions WHERE id = ?1",
            params![requirement.revision_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "The pilot must reach closed testing with a verified build."
    );
    assert_eq!(
        conn.query_row(
            "SELECT target_id FROM cp_factory_requirement_links WHERE requirement_id = ?1 AND relation_kind = 'derived_from'",
            params![requirement.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        charter.id
    );
}

#[test]
fn discovery_intake_prompts_have_stable_unique_keys() {
    let keys: std::collections::BTreeSet<_> = DISCOVERY_INTAKE_PROMPTS
        .iter()
        .map(|prompt| prompt.key)
        .collect();
    assert_eq!(keys.len(), DISCOVERY_INTAKE_PROMPTS.len());
    assert!(DISCOVERY_INTAKE_PROMPTS
        .iter()
        .all(|prompt| prompt.required));
}

#[test]
fn approved_change_creates_an_immutable_requirement_revision() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    let charter = create_factory_charter_draft(&tx, "owner-a", &product.id, "Charter")
        .unwrap()
        .unwrap();
    let requirement = create_factory_requirement_draft(
        &tx,
        "owner-a",
        &product.id,
        "closed_testing_readiness",
        "A verified build must be ready for closed testing.",
    )
    .unwrap()
    .unwrap();
    tx.execute(
        "INSERT INTO cp_factory_evidence_links (id, product_id, subject_kind, subject_id, content_ref, storage_class)
         VALUES ('evidence-1', ?1, 'stage_run', 'stage-1', 'local://verification', 'inline_reference')",
        [&product.id],
    )
    .unwrap();
    assert!(link_factory_stage_evidence_to_requirement(
        &tx,
        "owner-a",
        "evidence-1",
        &requirement.id,
    )
    .unwrap());
    let change_id = submit_factory_change_request(
        &tx,
        "owner-a",
        &product.id,
        "Require a rollback plan for the closed-testing build.",
    )
    .unwrap()
    .unwrap();
    let (assessment_id, affected_count) = assess_factory_change_request(&tx, "owner-a", &change_id)
        .unwrap()
        .unwrap();
    assert_eq!(affected_count, 2);
    assert!(resolve_factory_impact_assessment(&tx, "owner-a", &assessment_id, true).unwrap());

    let revised = apply_approved_requirement_change(
        &tx,
        "owner-a",
        &assessment_id,
        &requirement.id,
        "A verified build and rollback plan must be ready for closed testing.",
    )
    .unwrap()
    .unwrap();
    assert_ne!(revised.revision_id, requirement.revision_id);

    assert_eq!(
        tx.query_row(
            "SELECT current_revision FROM cp_factory_requirements WHERE id = ?1",
            [&requirement.id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
    assert_eq!(
        tx.query_row(
            "SELECT staleness_state FROM cp_factory_impact_targets WHERE assessment_id = ?1 AND target_kind = 'requirement' AND target_id = ?2",
            params![assessment_id, requirement.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "current"
    );
    assert_eq!(
        tx.query_row(
            "SELECT staleness_state FROM cp_factory_evidence_links WHERE id='evidence-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "stale"
    );
    assert_eq!(
        tx.query_row(
            "SELECT content_text FROM cp_factory_requirement_revisions WHERE id = ?1",
            [&requirement.revision_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "A verified build must be ready for closed testing."
    );
    assert_eq!(
        tx.query_row(
            "SELECT content_text FROM cp_factory_requirement_revisions WHERE id = ?1",
            [&revised.revision_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "A verified build and rollback plan must be ready for closed testing."
    );
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_factory_impact_targets WHERE assessment_id = ?1 AND staleness_state = 'stale'",
            [&assessment_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        tx.query_row(
            "SELECT target_id FROM cp_factory_requirement_links WHERE requirement_id = ?1 AND relation_kind = 'supersedes'",
            [&requirement.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        requirement.revision_id
    );
    assert_eq!(charter.revision, 1);
}

#[test]
fn approved_plan_materializes_each_lifecycle_stage_once() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    let plan = create_factory_plan_draft(&tx, "owner-a", &product.id, "Closed testing")
        .unwrap()
        .unwrap();

    assert!(materialize_factory_cycle(&tx, "owner-a", &plan.id)
        .unwrap()
        .is_none());
    assert!(approve_factory_plan(&tx, "owner-a", &plan.id).unwrap());

    let cycle = materialize_factory_cycle(&tx, "owner-a", &plan.id)
        .unwrap()
        .unwrap();
    assert!(cycle.created);
    assert_eq!(cycle.product_id, product.id);
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_factory_stage_runs WHERE cycle_id = ?1 AND status = 'pending'",
            [&cycle.id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        FACTORY_LIFECYCLE_STAGES.len() as i64
    );
    assert_eq!(
        tx.query_row(
            "SELECT GROUP_CONCAT(stage, ',') FROM (SELECT stage FROM cp_factory_stage_runs WHERE cycle_id = ?1 ORDER BY rowid)",
            [&cycle.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        FACTORY_LIFECYCLE_STAGES.join(",")
    );

    let resumed = materialize_factory_cycle(&tx, "owner-a", &plan.id)
        .unwrap()
        .unwrap();
    assert!(!resumed.created);
    assert_eq!(resumed.id, cycle.id);

    let graph_id = materialize_factory_stage_task_graph(&tx, "owner-a", &cycle.id, "discover")
        .unwrap()
        .unwrap();
    assert_eq!(
        tx.query_row(
            "SELECT task_graph_id FROM cp_factory_stage_runs WHERE cycle_id=?1 AND stage='discover'",
            [&cycle.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        graph_id
    );
    assert_eq!(
        tx.query_row(
            "SELECT shell_cmd FROM cp_task_graph_nodes WHERE graph_id=?1",
            [&graph_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        ""
    );
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_approvals approval JOIN cp_task_graph_nodes node ON node.task_id=approval.task_id WHERE node.graph_id=?1 AND approval.approval_type='factory_stage_execution' AND approval.status='pending'",
            [&graph_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        materialize_factory_stage_task_graph(&tx, "owner-a", &cycle.id, "discover")
            .unwrap()
            .unwrap(),
        graph_id
    );
    assert!(!activate_approved_factory_stage(&tx, "owner-a", &cycle.id, "discover").unwrap());
    assert!(pause_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
    tx.execute(
        "UPDATE cp_approvals SET status='approved', resolved_at=datetime('now','utc'), resolved_by='owner-a' WHERE task_id IN (SELECT task_id FROM cp_task_graph_nodes WHERE graph_id=?1)",
        [&graph_id],
    )
    .unwrap();
    assert!(!activate_approved_factory_stage(&tx, "owner-a", &cycle.id, "discover").unwrap());
    assert!(resume_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
    assert!(activate_approved_factory_stage(&tx, "owner-a", &cycle.id, "discover").unwrap());
    assert!(pause_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
    assert!(!complete_factory_stage_with_evidence(&tx, "owner-a", &cycle.id, "discover").unwrap());
    assert!(resume_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
    assert!(
        record_factory_stage_evidence(&tx, "owner-a", &cycle.id, "discover", "  ")
            .unwrap()
            .is_none()
    );
    assert!(record_factory_stage_evidence(
        &tx,
        "owner-a",
        &cycle.id,
        "discover",
        "local://discovery-report"
    )
    .unwrap()
    .is_some());
    let (task_id, run_id): (String, String) = tx
        .query_row(
            "SELECT node.task_id, node.agent_run_id FROM cp_task_graph_nodes node WHERE node.graph_id=?1 AND node.node_id='discover'",
            [&graph_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    tx.execute(
        "INSERT INTO cp_artifacts (id, task_id, agent_run_id, kind, status, content_ref, created_at)
         VALUES ('artifact-1', ?1, ?2, 'verification_report', 'submitted', 'sha256:abc123', datetime('now','utc'))",
        params![task_id, run_id],
    )
    .unwrap();
    assert!(record_factory_stage_artifact_evidence(
        &tx,
        "owner-a",
        &cycle.id,
        "discover",
        "artifact-1",
    )
    .unwrap()
    .is_some());
    assert!(complete_factory_stage_with_evidence(&tx, "owner-a", &cycle.id, "discover").unwrap());
    assert_eq!(
        tx.query_row(
            "SELECT status FROM cp_factory_stage_runs WHERE cycle_id=?1 AND stage='discover'",
            [&cycle.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "completed"
    );
    assert!(cancel_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
    assert_eq!(
        tx.query_row(
            "SELECT status FROM cp_factory_stage_runs WHERE cycle_id=?1 AND stage='define'",
            [&cycle.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "cancelled"
    );
    assert!(!resume_factory_cycle(&tx, "owner-a", &cycle.id).unwrap());
}

#[test]
fn required_quality_profile_needs_passing_evidence_for_release_readiness() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    let profile =
        create_factory_quality_profile(&tx, "owner-a", &product.id, "Build verification", true)
            .unwrap()
            .unwrap();
    assert!(
        record_factory_quality_check(&tx, "owner-a", &profile, true, "   ")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        factory_release_ready(&tx, "owner-a", &product.id).unwrap(),
        Some(false)
    );
    assert_eq!(
        factory_release_readiness(&tx, "owner-a", &product.id)
            .unwrap()
            .unwrap()
            .required_quality_blockers,
        1
    );
    record_factory_quality_check(&tx, "owner-a", &profile, false, "test://failed-build").unwrap();
    assert_eq!(
        factory_release_ready(&tx, "owner-a", &product.id).unwrap(),
        Some(false)
    );
    record_factory_quality_check(&tx, "owner-a", &profile, true, "test://verified-build").unwrap();
    assert_eq!(
        factory_release_ready(&tx, "owner-a", &product.id).unwrap(),
        Some(false)
    );
    let plan = create_factory_plan_draft(&tx, "owner-a", &product.id, "Release plan")
        .unwrap()
        .unwrap();
    assert!(approve_factory_plan(&tx, "owner-a", &plan.id).unwrap());
    let cycle = materialize_factory_cycle(&tx, "owner-a", &plan.id)
        .unwrap()
        .unwrap();
    tx.execute(
        "UPDATE cp_factory_stage_runs SET status='completed', completed_at=datetime('now','utc') WHERE cycle_id=?1 AND stage='verify'",
        [&cycle.id],
    )
    .unwrap();
    assert_eq!(
        factory_release_ready(&tx, "owner-a", &product.id).unwrap(),
        Some(true)
    );
    let readiness = factory_release_readiness(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(readiness.required_quality_blockers, 0);
    assert!(readiness.completed_verify_stage);
    assert_eq!(readiness.pending_impact_assessments, 0);
    assert_eq!(readiness.stale_evidence_count, 0);
    assert!(readiness.ready);
    let change_id = submit_factory_change_request(
        &tx,
        "owner-a",
        &product.id,
        "Clarify closed-testing acceptance criteria",
    )
    .unwrap()
    .unwrap();
    let (assessment_id, _) = assess_factory_change_request(&tx, "owner-a", &change_id)
        .unwrap()
        .unwrap();
    let pending_readiness = factory_release_readiness(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(pending_readiness.pending_impact_assessments, 1);
    assert!(!pending_readiness.ready);
    assert!(resolve_factory_impact_assessment(&tx, "owner-a", &assessment_id, false).unwrap());
    assert!(factory_release_ready(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap());
    tx.execute(
        "INSERT INTO cp_factory_evidence_links (id, product_id, subject_kind, subject_id, content_ref, storage_class, staleness_state)
         VALUES ('stale-evidence-1', ?1, 'stage_run', 'verify-run', 'local://superseded-report', 'inline_reference', 'stale')",
        [&product.id],
    )
    .unwrap();
    let stale_readiness = factory_release_readiness(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(stale_readiness.stale_evidence_count, 1);
    assert!(!stale_readiness.ready);
    tx.execute(
        "UPDATE cp_factory_evidence_links SET staleness_state='current' WHERE id='stale-evidence-1'",
        [],
    )
    .unwrap();
    let release_id =
        create_factory_release_draft(&tx, "owner-a", &product.id, "build://closed-testing")
            .unwrap()
            .unwrap();
    assert!(approve_factory_closed_testing_release(&tx, "owner-a", &release_id).unwrap());
    assert_eq!(
        tx.query_row(
            "SELECT status FROM cp_factory_release_channels WHERE release_id=?1",
            [&release_id],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "approved"
    );
}

#[test]
fn react_native_closed_testing_profile_is_owner_bound_and_idempotent() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    assert!(
        ensure_react_native_closed_testing_quality_profile(&tx, "owner-b", &product.id,)
            .unwrap()
            .is_none()
    );
    let first = ensure_react_native_closed_testing_quality_profile(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    let second = ensure_react_native_closed_testing_quality_profile(&tx, "owner-a", &product.id)
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), REACT_NATIVE_CLOSED_TESTING_QUALITY_GATES.len());
    assert_eq!(
        tx.query_row(
            "SELECT COUNT(*) FROM cp_factory_quality_profiles WHERE product_id=?1 AND required=1",
            [&product.id],
            |row| row.get::<_, usize>(0),
        )
        .unwrap(),
        REACT_NATIVE_CLOSED_TESTING_QUALITY_GATES.len(),
    );
}

#[test]
fn support_item_resolution_is_owner_bound_and_keeps_resolution_evidence() {
    let conn = in_memory();
    let tx = conn.unchecked_transaction().unwrap();
    let workspace = create_factory_workspace(&tx, "owner-a", "Workspace").unwrap();
    let product = create_factory_product_draft(&tx, "owner-a", &workspace.id, "Pilot")
        .unwrap()
        .unwrap();
    let item_id = create_factory_support_item(
        &tx,
        "owner-a",
        &product.id,
        "tester_feedback",
        "Small layout issue",
    )
    .unwrap()
    .unwrap();
    assert!(!resolve_factory_support_item(&tx, "owner-b", &item_id, "local://resolution").unwrap());
    assert!(triage_factory_support_item(&tx, "owner-a", &item_id).unwrap());
    assert!(resolve_factory_support_item(&tx, "owner-a", &item_id, "local://resolution").unwrap());
    assert_eq!(
        tx.query_row(
            "SELECT resolution_ref FROM cp_factory_support_items WHERE id=?1",
            [&item_id],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "local://resolution"
    );
}
