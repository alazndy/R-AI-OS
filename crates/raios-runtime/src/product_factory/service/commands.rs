//! One function per Product Factory command.
//!
//! Each runs inside the dispatcher's transaction (`tx`): returning `Err` rolls
//! everything back, so a refused command leaves no trace and its idempotency
//! key stays usable. Authorization of the principal and validation of the
//! command's fields have already happened in the dispatcher.

use super::*;

pub(super) fn create_workspace(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    name: &str,
) -> Result<serde_json::Value, Problem> {
    let workspace = raios_core::db::create_factory_workspace(tx, actor.subject(), name)
        .map_err(|error| Problem::internal(error.to_string()))?;
    Ok(serde_json::json!({"workspace_id": workspace.id, "name": workspace.name}))
}

pub(super) fn create_product_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    workspace_id: &str,
    title: &str,
) -> Result<serde_json::Value, Problem> {
    let product =
        raios_core::db::create_factory_product_draft(tx, actor.subject(), workspace_id, title)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Workspace is not owned by this principal"))?;
    Ok(serde_json::json!({
        "product_id": product.id,
        "workspace_id": product.workspace_id,
        "title": product.title
    }))
}

pub(super) fn set_product_mode(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    mode: &raios_contracts::FactoryMode,
) -> Result<serde_json::Value, Problem> {
    let mode = match mode {
        raios_contracts::FactoryMode::Quick => "quick",
        raios_contracts::FactoryMode::Governed => "governed",
    };
    let updated = raios_core::db::set_factory_product_mode(tx, actor.subject(), product_id, mode)
        .map_err(|error| Problem::internal(error.to_string()))?;
    if !updated {
        return Err(Problem::forbidden("Product is not owned by this principal"));
    }
    Ok(serde_json::json!({"product_id": product_id, "mode": mode}))
}

pub(super) fn scaffold_project(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    override_config: Option<&raios_core::config::Config>,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let (title, existing_path, platform, mode) =
        raios_core::db::load_factory_product_scaffold_context(tx, actor.subject(), product_id)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(if !existing_path.is_empty() {
        serde_json::json!({"product_id": product_id, "project_path": existing_path, "created": false})
    } else {
        let loaded_cfg;
        let config = match override_config {
            Some(c) => c,
            None => {
                loaded_cfg = raios_core::config::Config::load().unwrap_or_else(|| {
                    raios_core::config::Config::from_detect_result(
                        raios_core::config::Config::auto_detect(),
                    )
                });
                &loaded_cfg
            }
        };
        if !config.dev_ops_path.is_dir() {
            return Err(Problem::internal(
                "Factory workspace root is not configured",
            ));
        }
        let category = factory_project_category(&platform);
        let project_name = factory_project_slug(&title);
        let target_dir = config.dev_ops_path.join(category).join(&project_name);
        if target_dir.exists()
            && std::fs::read_dir(&target_dir)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
        {
            return Err(Problem::invalid_input(format!(
                "Target directory '{}' already exists and is non-empty",
                target_dir.display()
            )));
        }
        let scaffold = crate::new_project::create(&crate::new_project::NewProjectConfig {
            name: &project_name,
            category,
            dev_ops: &config.dev_ops_path,
            github: false,
            no_vault: true,
            vault_path: None,
        });
        if !scaffold.path.join("README.md").is_file() || !scaffold.path.join("memory.md").is_file()
        {
            return Err(Problem::internal(
                "Factory project scaffold did not create required project files",
            ));
        }
        let project_path = scaffold.path.to_string_lossy().to_string();
        let manifest_content = format!(
            "name: \"{}\"\ncategory: \"{}\"\nstack: unknown\ngithub: null\nstatus: active\nproduct_id: \"{}\"\nmode: \"{}\"\nproject_path: \"{}\"\n",
            project_name, category, product_id, mode, project_path
        );
        let _ = std::fs::write(scaffold.path.join(".raios.yaml"), manifest_content);
        raios_core::db::save_factory_product_project_path(
            tx,
            actor.subject(),
            product_id,
            &project_path,
        )
        .map_err(|error| Problem::internal(error.to_string()))?;
        serde_json::json!({"product_id": product_id, "project_path": project_path, "category": category, "created": true})
    })
}

pub(super) fn attach_existing_project(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    project_path: &str,
) -> Result<serde_json::Value, Problem> {
    let source = inspect_existing_git_project(project_path)?;
    let attached = raios_core::db::attach_factory_product_existing_project(
        tx,
        actor.subject(),
        product_id,
        &source.project_path,
        &source.remote,
        &source.revision,
    )
    .map_err(|error| Problem::internal(error.to_string()))?;
    if !attached {
        return Err(Problem::forbidden("Product is not owned by this principal"));
    }
    Ok(serde_json::json!({
        "product_id": product_id,
        "project_path": source.project_path,
        "source_remote": source.remote,
        "source_revision": source.revision,
        "attached": true
    }))
}

pub(super) fn start_intake(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let session = raios_core::db::start_factory_intake(tx, actor.subject(), product_id)
        .map_err(|error| Problem::internal(error.to_string()))?
        .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(serde_json::json!({"session_id": session.id, "product_id": session.product_id}))
}

pub(super) fn record_intake_answer(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    session_id: &str,
    question_key: &str,
    response: &str,
) -> Result<serde_json::Value, Problem> {
    let recorded = raios_core::db::record_factory_intake_answer(
        tx,
        actor.subject(),
        session_id,
        question_key,
        response,
    )
    .map_err(|error| Problem::internal(error.to_string()))?;
    if !recorded {
        return Err(Problem::forbidden(
            "Intake session is not open and owned by this principal",
        ));
    }
    Ok(
        serde_json::json!({"session_id": session_id, "question_key": question_key, "status": "answered"}),
    )
}

pub(super) fn create_charter_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    content: &str,
) -> Result<serde_json::Value, Problem> {
    let missing = raios_core::db::missing_required_intake_prompt_keys(tx, product_id)
        .map_err(|error| Problem::internal(error.to_string()))?;
    if !missing.is_empty() {
        return Err(Problem::invalid_input(format!(
            "Complete required intake prompts before drafting a Charter: {}",
            missing.join(", ")
        )));
    }
    let charter =
        raios_core::db::create_factory_charter_draft(tx, actor.subject(), product_id, content)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(serde_json::json!({
        "charter_revision_id": charter.id,
        "product_id": charter.product_id,
        "revision": charter.revision
    }))
}

pub(super) fn generate_charter_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let missing = raios_core::db::missing_required_intake_prompt_keys(tx, product_id)
        .map_err(|error| Problem::internal(error.to_string()))?;
    if !missing.is_empty() {
        return Err(Problem::invalid_input(format!(
            "Complete required intake prompts before generating a Charter: {}",
            missing.join(", ")
        )));
    }
    let title =
        raios_core::db::load_factory_product_title_for_owner(tx, product_id, actor.subject())
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    let answers = raios_core::db::load_required_intake_answers(tx, product_id)
        .map_err(|error| Problem::internal(error.to_string()))?;
    let content = raios_core::product_factory::compose_discovery_charter(&title, &answers);
    let charter =
        raios_core::db::create_factory_charter_draft(tx, actor.subject(), product_id, &content)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(serde_json::json!({
        "charter_revision_id": charter.id,
        "product_id": charter.product_id,
        "revision": charter.revision,
        "generated": true
    }))
}

pub(super) fn create_requirement_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    stable_key: &str,
    content: &str,
) -> Result<serde_json::Value, Problem> {
    let requirement = raios_core::db::create_factory_requirement_draft(
        tx,
        actor.subject(),
        product_id,
        stable_key,
        content,
    )
    .map_err(|error| Problem::internal(error.to_string()))?
    .ok_or_else(|| Problem::forbidden("Product has no owned Charter revision"))?;
    Ok(serde_json::json!({
        "requirement_id": requirement.id,
        "requirement_revision_id": requirement.revision_id,
        "product_id": product_id,
        "stable_key": stable_key
    }))
}

pub(super) fn submit_change_request(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    summary: &str,
) -> Result<serde_json::Value, Problem> {
    let id =
        raios_core::db::submit_factory_change_request(tx, actor.subject(), product_id, summary)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(serde_json::json!({"change_request_id": id, "product_id": product_id, "status": "proposed"}))
}

pub(super) fn assess_change_request(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    change_request_id: &str,
) -> Result<serde_json::Value, Problem> {
    let (assessment_id, affected_count) =
        raios_core::db::assess_factory_change_request(tx, actor.subject(), change_request_id)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| {
                Problem::forbidden("Change request is not owned or cannot be assessed")
            })?;
    Ok(
        serde_json::json!({"assessment_id": assessment_id, "change_request_id": change_request_id, "affected_count": affected_count, "status": "awaiting_approval"}),
    )
}

pub(super) fn resolve_impact_assessment(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    assessment_id: &str,
    approved: &bool,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::resolve_factory_impact_assessment(
        tx,
        actor.subject(),
        assessment_id,
        *approved,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Impact assessment is not awaiting owner approval",
        ));
    }
    Ok(serde_json::json!({"assessment_id": assessment_id, "approved": approved}))
}

pub(super) fn apply_approved_requirement_change(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    assessment_id: &str,
    requirement_id: &str,
    content: &str,
) -> Result<serde_json::Value, Problem> {
    let revision = raios_core::db::apply_approved_requirement_change(
        tx,
        actor.subject(),
        assessment_id,
        requirement_id,
        content,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    .ok_or_else(|| Problem::forbidden("Approved assessment and owned requirement are required"))?;
    Ok(
        serde_json::json!({"requirement_id": revision.id, "requirement_revision_id": revision.revision_id, "assessment_id": assessment_id}),
    )
}

pub(super) fn create_plan_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    title: &str,
) -> Result<serde_json::Value, Problem> {
    let plan = raios_core::db::create_factory_plan_draft(tx, actor.subject(), product_id, title)
        .map_err(|error| Problem::internal(error.to_string()))?
        .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(
        serde_json::json!({"plan_id": plan.id, "product_id": plan.product_id, "title": plan.title, "status": "planned"}),
    )
}

pub(super) fn approve_plan(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    plan_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::approve_factory_plan(tx, actor.subject(), plan_id)
        .map_err(|error| Problem::internal(error.to_string()))?
    {
        return Err(Problem::forbidden(
            "Plan is not owned or is not awaiting approval",
        ));
    }
    Ok(serde_json::json!({"plan_id": plan_id, "status": "approved"}))
}

pub(super) fn materialize_planned_cycle(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    plan_id: &str,
) -> Result<serde_json::Value, Problem> {
    let cycle = raios_core::db::materialize_factory_cycle(tx, actor.subject(), plan_id)
        .map_err(|error| Problem::internal(error.to_string()))?
        .ok_or_else(|| Problem::forbidden("Plan is not owned or has not been approved"))?;
    Ok(
        serde_json::json!({"cycle_id": cycle.id, "plan_id": cycle.plan_id, "product_id": cycle.product_id, "created": cycle.created, "status": "planned"}),
    )
}

pub(super) fn pause_cycle(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::pause_factory_cycle(tx, actor.subject(), cycle_id)
        .map_err(|error| Problem::internal(error.to_string()))?
    {
        return Err(Problem::forbidden("Cycle is not owned or cannot be paused"));
    }
    Ok(serde_json::json!({"cycle_id": cycle_id, "status": "paused"}))
}

pub(super) fn resume_cycle(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::resume_factory_cycle(tx, actor.subject(), cycle_id)
        .map_err(|error| Problem::internal(error.to_string()))?
    {
        return Err(Problem::forbidden("Cycle is not owned or is not paused"));
    }
    Ok(serde_json::json!({"cycle_id": cycle_id, "status": "resumed"}))
}

pub(super) fn cancel_cycle(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::cancel_factory_cycle(tx, actor.subject(), cycle_id)
        .map_err(|error| Problem::internal(error.to_string()))?
    {
        return Err(Problem::forbidden(
            "Cycle is not owned or cannot be cancelled",
        ));
    }
    Ok(serde_json::json!({"cycle_id": cycle_id, "status": "cancelled"}))
}

pub(super) fn materialize_stage_task_graph(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
    stage: &str,
) -> Result<serde_json::Value, Problem> {
    let graph_id =
        raios_core::db::materialize_factory_stage_task_graph(tx, actor.subject(), cycle_id, stage)
            .map_err(|e| Problem::internal(e.to_string()))?
            .ok_or_else(|| Problem::forbidden("Cycle stage is not owned or does not exist"))?;
    Ok(
        serde_json::json!({"cycle_id": cycle_id, "stage": stage, "task_graph_id": graph_id, "execution": "disabled"}),
    )
}

pub(super) fn activate_approved_stage(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
    stage: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::activate_approved_factory_stage(tx, actor.subject(), cycle_id, stage)
        .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Stage requires an owned approved execution approval",
        ));
    }
    Ok(
        serde_json::json!({"cycle_id": cycle_id, "stage": stage, "status":"active", "execution":"operator-controlled"}),
    )
}

pub(super) fn record_stage_evidence(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
    stage: &str,
    content_ref: &str,
) -> Result<serde_json::Value, Problem> {
    let evidence_id = raios_core::db::record_factory_stage_evidence(
        tx,
        actor.subject(),
        cycle_id,
        stage,
        content_ref,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    .ok_or_else(|| Problem::forbidden("Stage is not owned or does not exist"))?;
    Ok(serde_json::json!({"evidence_id": evidence_id, "cycle_id": cycle_id, "stage": stage}))
}

pub(super) fn link_stage_evidence_to_requirement(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    evidence_id: &str,
    requirement_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::link_factory_stage_evidence_to_requirement(
        tx,
        actor.subject(),
        evidence_id,
        requirement_id,
    )
    .map_err(|error| Problem::internal(error.to_string()))?
    {
        return Err(Problem::forbidden(
            "Stage evidence and requirement must belong to the owned product",
        ));
    }
    Ok(
        serde_json::json!({"evidence_id": evidence_id, "requirement_id": requirement_id, "status": "linked"}),
    )
}

pub(super) fn complete_stage(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    cycle_id: &str,
    stage: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::complete_factory_stage_with_evidence(tx, actor.subject(), cycle_id, stage)
        .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Active owned stage requires evidence before completion",
        ));
    }
    Ok(serde_json::json!({"cycle_id": cycle_id, "stage": stage, "status":"completed"}))
}

pub(super) fn inspect_release_readiness(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let readiness = raios_core::db::factory_release_readiness(tx, actor.subject(), product_id)
        .map_err(|e| Problem::internal(e.to_string()))?
        .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(
        serde_json::json!({"product_id": product_id, "ready": readiness.ready, "required_quality_blockers": readiness.required_quality_blockers, "completed_verify_stage": readiness.completed_verify_stage, "pending_impact_assessments": readiness.pending_impact_assessments, "stale_evidence_count": readiness.stale_evidence_count}),
    )
}

pub(super) fn create_quality_profile(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    name: &str,
    required: &bool,
) -> Result<serde_json::Value, Problem> {
    let profile_id = raios_core::db::create_factory_quality_profile(
        tx,
        actor.subject(),
        product_id,
        name,
        *required,
    )
    .map_err(|error| Problem::internal(error.to_string()))?
    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(
        serde_json::json!({"profile_id": profile_id, "product_id": product_id, "required": required}),
    )
}

pub(super) fn ensure_react_native_closed_testing_quality_profile(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let profile_ids = raios_core::db::ensure_react_native_closed_testing_quality_profile(
        tx,
        actor.subject(),
        product_id,
    )
    .map_err(|error| Problem::internal(error.to_string()))?
    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(
        serde_json::json!({"product_id": product_id, "profile_ids": profile_ids, "profile": "react_native_closed_testing"}),
    )
}

pub(super) fn record_quality_check(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    profile_id: &str,
    passed: &bool,
    evidence_ref: &str,
) -> Result<serde_json::Value, Problem> {
    let check_id = raios_core::db::record_factory_quality_check(
        tx,
        actor.subject(),
        profile_id,
        *passed,
        evidence_ref,
    )
    .map_err(|error| Problem::internal(error.to_string()))?
    .ok_or_else(|| Problem::forbidden("Quality profile is not owned by this principal"))?;
    Ok(
        serde_json::json!({"check_id": check_id, "profile_id": profile_id, "passed": passed, "evidence_ref": evidence_ref}),
    )
}

pub(super) fn create_release_draft(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    build_ref: &str,
) -> Result<serde_json::Value, Problem> {
    let release_id =
        raios_core::db::create_factory_release_draft(tx, actor.subject(), product_id, build_ref)
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| {
                Problem::forbidden(
                    "Owned product is not release-ready; required quality evidence is missing",
                )
            })?;
    Ok(
        serde_json::json!({"release_id": release_id, "product_id": product_id, "status": "draft", "build_ref": build_ref}),
    )
}

pub(super) fn approve_closed_testing_release(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    release_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::approve_factory_closed_testing_release(tx, actor.subject(), release_id)
        .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden("Release is not an owned draft"));
    }
    Ok(
        serde_json::json!({"release_id": release_id, "status": "approved", "channel": "closed_testing"}),
    )
}

pub(super) fn create_support_item(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
    source_kind: &str,
    summary: &str,
) -> Result<serde_json::Value, Problem> {
    let id = raios_core::db::create_factory_support_item(
        tx,
        actor.subject(),
        product_id,
        source_kind,
        summary,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(serde_json::json!({"support_item_id": id, "status":"new"}))
}

pub(super) fn inspect_support_overview(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    product_id: &str,
) -> Result<serde_json::Value, Problem> {
    let overview = raios_core::db::factory_support_overview(tx, actor.subject(), product_id)
        .map_err(|error| Problem::internal(error.to_string()))?
        .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
    Ok(
        serde_json::json!({"product_id": product_id, "open_count": overview.open_count, "resolved_count": overview.resolved_count, "linked_change_count": overview.linked_change_count, "oldest_open_created_at": overview.oldest_open_created_at}),
    )
}

pub(super) fn triage_support_item(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    support_item_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::triage_factory_support_item(tx, actor.subject(), support_item_id)
        .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Support item is not owned or cannot be triaged",
        ));
    }
    Ok(serde_json::json!({"support_item_id": support_item_id, "status":"triaged"}))
}

pub(super) fn resolve_support_item(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    support_item_id: &str,
    resolution_ref: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::resolve_factory_support_item(
        tx,
        actor.subject(),
        support_item_id,
        resolution_ref,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Support item is not owned or cannot be resolved",
        ));
    }
    Ok(
        serde_json::json!({"support_item_id": support_item_id, "status":"resolved", "resolution_ref": resolution_ref}),
    )
}

pub(super) fn link_support_to_change_request(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    support_item_id: &str,
    change_request_id: &str,
) -> Result<serde_json::Value, Problem> {
    if !raios_core::db::link_support_to_change_request(
        tx,
        actor.subject(),
        support_item_id,
        change_request_id,
    )
    .map_err(|e| Problem::internal(e.to_string()))?
    {
        return Err(Problem::forbidden(
            "Support item and change request must be owned by the same product",
        ));
    }
    Ok(
        serde_json::json!({"support_item_id": support_item_id, "change_request_id": change_request_id, "relation":"raised_by"}),
    )
}

pub(super) fn request_impact_assessment() -> Result<serde_json::Value, Problem> {
    Err(Problem::not_implemented(
        "Impact assessment mutation is not implemented in Phase 3",
    ))
}
