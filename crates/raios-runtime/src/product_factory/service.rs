use raios_contracts::{FactoryCommand, Problem};
use raios_core::product_factory::{
    FactoryInvariantError, FactoryProduct, FactoryWorkspace, ImpactAssessmentSummary,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

use crate::control_plane::service::{init_idempotency_table, ControlActor};

/// Product Factory service boundary. Implementations are intentionally absent
/// until the skeleton review approves lifecycle mutation semantics.
pub trait ProductFactoryService {
    fn workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<FactoryWorkspace>, FactoryInvariantError>;
    fn product(&self, product_id: &str) -> Result<Option<FactoryProduct>, FactoryInvariantError>;
    fn impact_assessment(
        &self,
        change_request_id: &str,
    ) -> Result<Option<ImpactAssessmentSummary>, FactoryInvariantError>;
}

/// Dispatch a narrow, local-only Product Factory mutation command.
///
/// This is deliberately separate from the general control-plane command enum
/// until a reviewed transport route and UI gesture model exist. The caller
/// supplies the feature gate from trusted configuration; commands never turn
/// the Factory on themselves.
pub fn dispatch_factory_command(
    conn: &mut Connection,
    actor: &ControlActor,
    factory_enabled: bool,
    command: &FactoryCommand,
) -> Result<serde_json::Value, Problem> {
    dispatch_factory_command_with_config(conn, actor, factory_enabled, command, None)
}

pub fn dispatch_factory_command_with_config(
    conn: &mut Connection,
    actor: &ControlActor,
    factory_enabled: bool,
    command: &FactoryCommand,
    override_config: Option<&raios_core::config::Config>,
) -> Result<serde_json::Value, Problem> {
    if !factory_enabled {
        return Err(Problem::forbidden(
            "Product Factory mutations are disabled by configuration",
        ));
    }
    if !actor.may_mutate_control_plane() {
        return Err(Problem::unauthorized(
            "This authenticated principal is not authorized to mutate Product Factory state",
        ));
    }

    validate_command(command)?;
    init_idempotency_table(conn).map_err(Problem::internal)?;

    let payload = serde_json::to_string(command)
        .map_err(|error| Problem::invalid_input(format!("Failed serializing command: {error}")))?;
    let payload_hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let command_name = factory_command_name(command);
    let key = command.idempotency_key();
    let cached: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT payload_hash, result_json FROM cp_idempotency WHERE idempotency_key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| Problem::internal(error.to_string()))?;
    if let Some((stored_hash, cached_result)) = cached {
        if !stored_hash.is_empty() && stored_hash != payload_hash {
            return Err(Problem::invalid_input(
                "Idempotency key is already bound to a different Product Factory command",
            ));
        }
        return cached_result
            .and_then(|json| serde_json::from_str(&json).ok())
            .ok_or_else(|| Problem::internal("Idempotency record is missing a valid result"));
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|error| Problem::internal(format!("Failed starting transaction: {error}")))?;
    let result = match command {
        FactoryCommand::CreateWorkspace { name, .. } => {
            let workspace = raios_core::db::create_factory_workspace(&tx, actor.subject(), name)
                .map_err(|error| Problem::internal(error.to_string()))?;
            serde_json::json!({"workspace_id": workspace.id, "name": workspace.name})
        }
        FactoryCommand::CreateProductDraft {
            workspace_id,
            title,
            ..
        } => {
            let product = raios_core::db::create_factory_product_draft(
                &tx,
                actor.subject(),
                workspace_id,
                title,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Workspace is not owned by this principal"))?;
            serde_json::json!({
                "product_id": product.id,
                "workspace_id": product.workspace_id,
                "title": product.title
            })
        }
        FactoryCommand::SetProductMode {
            product_id, mode, ..
        } => {
            let mode = match mode {
                raios_contracts::FactoryMode::Quick => "quick",
                raios_contracts::FactoryMode::Governed => "governed",
            };
            let updated =
                raios_core::db::set_factory_product_mode(&tx, actor.subject(), product_id, mode)
                    .map_err(|error| Problem::internal(error.to_string()))?;
            if !updated {
                return Err(Problem::forbidden("Product is not owned by this principal"));
            }
            serde_json::json!({"product_id": product_id, "mode": mode})
        }
        FactoryCommand::ScaffoldProject { product_id, .. } => {
            let (title, existing_path, platform, mode) =
                raios_core::db::load_factory_product_scaffold_context(
                    &tx,
                    actor.subject(),
                    product_id,
                )
                .map_err(|error| Problem::internal(error.to_string()))?
                .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            if !existing_path.is_empty() {
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
                });
                if !scaffold.path.join("README.md").is_file()
                    || !scaffold.path.join("memory.md").is_file()
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
                    &tx,
                    actor.subject(),
                    product_id,
                    &project_path,
                )
                .map_err(|error| Problem::internal(error.to_string()))?;
                serde_json::json!({"product_id": product_id, "project_path": project_path, "category": category, "created": true})
            }
        }
        FactoryCommand::AttachExistingProject {
            product_id,
            project_path,
            ..
        } => {
            let source = inspect_existing_git_project(project_path)?;
            let attached = raios_core::db::attach_factory_product_existing_project(
                &tx,
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
            serde_json::json!({
                "product_id": product_id,
                "project_path": source.project_path,
                "source_remote": source.remote,
                "source_revision": source.revision,
                "attached": true
            })
        }
        FactoryCommand::StartIntake { product_id, .. } => {
            let session = raios_core::db::start_factory_intake(&tx, actor.subject(), product_id)
                .map_err(|error| Problem::internal(error.to_string()))?
                .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"session_id": session.id, "product_id": session.product_id})
        }
        FactoryCommand::RecordIntakeAnswer {
            session_id,
            question_key,
            response,
            ..
        } => {
            let recorded = raios_core::db::record_factory_intake_answer(
                &tx,
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
            serde_json::json!({"session_id": session_id, "question_key": question_key, "status": "answered"})
        }
        FactoryCommand::CreateCharterDraft {
            product_id,
            content,
            ..
        } => {
            let missing = raios_core::db::missing_required_intake_prompt_keys(&tx, product_id)
                .map_err(|error| Problem::internal(error.to_string()))?;
            if !missing.is_empty() {
                return Err(Problem::invalid_input(format!(
                    "Complete required intake prompts before drafting a Charter: {}",
                    missing.join(", ")
                )));
            }
            let charter = raios_core::db::create_factory_charter_draft(
                &tx,
                actor.subject(),
                product_id,
                content,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({
                "charter_revision_id": charter.id,
                "product_id": charter.product_id,
                "revision": charter.revision
            })
        }
        FactoryCommand::GenerateCharterDraft { product_id, .. } => {
            let missing = raios_core::db::missing_required_intake_prompt_keys(&tx, product_id)
                .map_err(|error| Problem::internal(error.to_string()))?;
            if !missing.is_empty() {
                return Err(Problem::invalid_input(format!(
                    "Complete required intake prompts before generating a Charter: {}",
                    missing.join(", ")
                )));
            }
            let title = raios_core::db::load_factory_product_title_for_owner(
                &tx,
                product_id,
                actor.subject(),
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            let answers = raios_core::db::load_required_intake_answers(&tx, product_id)
                .map_err(|error| Problem::internal(error.to_string()))?;
            let content = raios_core::product_factory::compose_discovery_charter(&title, &answers);
            let charter = raios_core::db::create_factory_charter_draft(
                &tx,
                actor.subject(),
                product_id,
                &content,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({
                "charter_revision_id": charter.id,
                "product_id": charter.product_id,
                "revision": charter.revision,
                "generated": true
            })
        }
        FactoryCommand::CreateRequirementDraft {
            product_id,
            stable_key,
            content,
            ..
        } => {
            let requirement = raios_core::db::create_factory_requirement_draft(
                &tx,
                actor.subject(),
                product_id,
                stable_key,
                content,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product has no owned Charter revision"))?;
            serde_json::json!({
                "requirement_id": requirement.id,
                "requirement_revision_id": requirement.revision_id,
                "product_id": product_id,
                "stable_key": stable_key
            })
        }
        FactoryCommand::SubmitChangeRequest {
            product_id,
            summary,
            ..
        } => {
            let id = raios_core::db::submit_factory_change_request(
                &tx,
                actor.subject(),
                product_id,
                summary,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"change_request_id": id, "product_id": product_id, "status": "proposed"})
        }
        FactoryCommand::AssessChangeRequest {
            change_request_id, ..
        } => {
            let (assessment_id, affected_count) = raios_core::db::assess_factory_change_request(
                &tx,
                actor.subject(),
                change_request_id,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| {
                Problem::forbidden("Change request is not owned or cannot be assessed")
            })?;
            serde_json::json!({"assessment_id": assessment_id, "change_request_id": change_request_id, "affected_count": affected_count, "status": "awaiting_approval"})
        }
        FactoryCommand::ResolveImpactAssessment {
            assessment_id,
            approved,
            ..
        } => {
            if !raios_core::db::resolve_factory_impact_assessment(
                &tx,
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
            serde_json::json!({"assessment_id": assessment_id, "approved": approved})
        }
        FactoryCommand::ApplyApprovedRequirementChange {
            assessment_id,
            requirement_id,
            content,
            ..
        } => {
            let revision = raios_core::db::apply_approved_requirement_change(
                &tx,
                actor.subject(),
                assessment_id,
                requirement_id,
                content,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            .ok_or_else(|| {
                Problem::forbidden("Approved assessment and owned requirement are required")
            })?;
            serde_json::json!({"requirement_id": revision.id, "requirement_revision_id": revision.revision_id, "assessment_id": assessment_id})
        }
        FactoryCommand::CreatePlanDraft {
            product_id, title, ..
        } => {
            let plan =
                raios_core::db::create_factory_plan_draft(&tx, actor.subject(), product_id, title)
                    .map_err(|error| Problem::internal(error.to_string()))?
                    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"plan_id": plan.id, "product_id": plan.product_id, "title": plan.title, "status": "planned"})
        }
        FactoryCommand::ApprovePlan { plan_id, .. } => {
            if !raios_core::db::approve_factory_plan(&tx, actor.subject(), plan_id)
                .map_err(|error| Problem::internal(error.to_string()))?
            {
                return Err(Problem::forbidden(
                    "Plan is not owned or is not awaiting approval",
                ));
            }
            serde_json::json!({"plan_id": plan_id, "status": "approved"})
        }
        FactoryCommand::MaterializePlannedCycle { plan_id, .. } => {
            let cycle = raios_core::db::materialize_factory_cycle(&tx, actor.subject(), plan_id)
                .map_err(|error| Problem::internal(error.to_string()))?
                .ok_or_else(|| Problem::forbidden("Plan is not owned or has not been approved"))?;
            serde_json::json!({"cycle_id": cycle.id, "plan_id": cycle.plan_id, "product_id": cycle.product_id, "created": cycle.created, "status": "planned"})
        }
        FactoryCommand::PauseCycle { cycle_id, .. } => {
            if !raios_core::db::pause_factory_cycle(&tx, actor.subject(), cycle_id)
                .map_err(|error| Problem::internal(error.to_string()))?
            {
                return Err(Problem::forbidden("Cycle is not owned or cannot be paused"));
            }
            serde_json::json!({"cycle_id": cycle_id, "status": "paused"})
        }
        FactoryCommand::ResumeCycle { cycle_id, .. } => {
            if !raios_core::db::resume_factory_cycle(&tx, actor.subject(), cycle_id)
                .map_err(|error| Problem::internal(error.to_string()))?
            {
                return Err(Problem::forbidden("Cycle is not owned or is not paused"));
            }
            serde_json::json!({"cycle_id": cycle_id, "status": "resumed"})
        }
        FactoryCommand::CancelCycle { cycle_id, .. } => {
            if !raios_core::db::cancel_factory_cycle(&tx, actor.subject(), cycle_id)
                .map_err(|error| Problem::internal(error.to_string()))?
            {
                return Err(Problem::forbidden(
                    "Cycle is not owned or cannot be cancelled",
                ));
            }
            serde_json::json!({"cycle_id": cycle_id, "status": "cancelled"})
        }
        FactoryCommand::MaterializeStageTaskGraph {
            cycle_id, stage, ..
        } => {
            let graph_id = raios_core::db::materialize_factory_stage_task_graph(
                &tx,
                actor.subject(),
                cycle_id,
                stage,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            .ok_or_else(|| Problem::forbidden("Cycle stage is not owned or does not exist"))?;
            serde_json::json!({"cycle_id": cycle_id, "stage": stage, "task_graph_id": graph_id, "execution": "disabled"})
        }
        FactoryCommand::ActivateApprovedStage {
            cycle_id, stage, ..
        } => {
            if !raios_core::db::activate_approved_factory_stage(
                &tx,
                actor.subject(),
                cycle_id,
                stage,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::forbidden(
                    "Stage requires an owned approved execution approval",
                ));
            }
            serde_json::json!({"cycle_id": cycle_id, "stage": stage, "status":"active", "execution":"operator-controlled"})
        }
        FactoryCommand::RecordStageEvidence {
            cycle_id,
            stage,
            content_ref,
            ..
        } => {
            let evidence_id = raios_core::db::record_factory_stage_evidence(
                &tx,
                actor.subject(),
                cycle_id,
                stage,
                content_ref,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            .ok_or_else(|| Problem::forbidden("Stage is not owned or does not exist"))?;
            serde_json::json!({"evidence_id": evidence_id, "cycle_id": cycle_id, "stage": stage})
        }
        FactoryCommand::LinkStageEvidenceToRequirement {
            evidence_id,
            requirement_id,
            ..
        } => {
            if !raios_core::db::link_factory_stage_evidence_to_requirement(
                &tx,
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
            serde_json::json!({"evidence_id": evidence_id, "requirement_id": requirement_id, "status": "linked"})
        }
        FactoryCommand::CompleteStage {
            cycle_id, stage, ..
        } => {
            if !raios_core::db::complete_factory_stage_with_evidence(
                &tx,
                actor.subject(),
                cycle_id,
                stage,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::forbidden(
                    "Active owned stage requires evidence before completion",
                ));
            }
            serde_json::json!({"cycle_id": cycle_id, "stage": stage, "status":"completed"})
        }
        FactoryCommand::InspectReleaseReadiness { product_id, .. } => {
            let readiness =
                raios_core::db::factory_release_readiness(&tx, actor.subject(), product_id)
                    .map_err(|e| Problem::internal(e.to_string()))?
                    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"product_id": product_id, "ready": readiness.ready, "required_quality_blockers": readiness.required_quality_blockers, "completed_verify_stage": readiness.completed_verify_stage, "pending_impact_assessments": readiness.pending_impact_assessments, "stale_evidence_count": readiness.stale_evidence_count})
        }
        FactoryCommand::CreateQualityProfile {
            product_id,
            name,
            required,
            ..
        } => {
            let profile_id = raios_core::db::create_factory_quality_profile(
                &tx,
                actor.subject(),
                product_id,
                name,
                *required,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"profile_id": profile_id, "product_id": product_id, "required": required})
        }
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { product_id, .. } => {
            let profile_ids = raios_core::db::ensure_react_native_closed_testing_quality_profile(
                &tx,
                actor.subject(),
                product_id,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"product_id": product_id, "profile_ids": profile_ids, "profile": "react_native_closed_testing"})
        }
        FactoryCommand::RecordQualityCheck {
            profile_id,
            passed,
            evidence_ref,
            ..
        } => {
            let check_id = raios_core::db::record_factory_quality_check(
                &tx,
                actor.subject(),
                profile_id,
                *passed,
                evidence_ref,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| Problem::forbidden("Quality profile is not owned by this principal"))?;
            serde_json::json!({"check_id": check_id, "profile_id": profile_id, "passed": passed, "evidence_ref": evidence_ref})
        }
        FactoryCommand::CreateReleaseDraft {
            product_id,
            build_ref,
            ..
        } => {
            let release_id = raios_core::db::create_factory_release_draft(
                &tx,
                actor.subject(),
                product_id,
                build_ref,
            )
            .map_err(|error| Problem::internal(error.to_string()))?
            .ok_or_else(|| {
                Problem::forbidden(
                    "Owned product is not release-ready; required quality evidence is missing",
                )
            })?;
            serde_json::json!({"release_id": release_id, "product_id": product_id, "status": "draft", "build_ref": build_ref})
        }
        FactoryCommand::ApproveClosedTestingRelease { release_id, .. } => {
            if !raios_core::db::approve_factory_closed_testing_release(
                &tx,
                actor.subject(),
                release_id,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::forbidden("Release is not an owned draft"));
            }
            serde_json::json!({"release_id": release_id, "status": "approved", "channel": "closed_testing"})
        }
        FactoryCommand::CreateSupportItem {
            product_id,
            source_kind,
            summary,
            ..
        } => {
            let id = raios_core::db::create_factory_support_item(
                &tx,
                actor.subject(),
                product_id,
                source_kind,
                summary,
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"support_item_id": id, "status":"new"})
        }
        FactoryCommand::InspectSupportOverview { product_id, .. } => {
            let overview =
                raios_core::db::factory_support_overview(&tx, actor.subject(), product_id)
                    .map_err(|error| Problem::internal(error.to_string()))?
                    .ok_or_else(|| Problem::forbidden("Product is not owned by this principal"))?;
            serde_json::json!({"product_id": product_id, "open_count": overview.open_count, "resolved_count": overview.resolved_count, "linked_change_count": overview.linked_change_count, "oldest_open_created_at": overview.oldest_open_created_at})
        }
        FactoryCommand::TriageSupportItem {
            support_item_id, ..
        } => {
            if !raios_core::db::triage_factory_support_item(&tx, actor.subject(), support_item_id)
                .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::forbidden(
                    "Support item is not owned or cannot be triaged",
                ));
            }
            serde_json::json!({"support_item_id": support_item_id, "status":"triaged"})
        }
        FactoryCommand::ResolveSupportItem {
            support_item_id,
            resolution_ref,
            ..
        } => {
            if !raios_core::db::resolve_factory_support_item(
                &tx,
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
            serde_json::json!({"support_item_id": support_item_id, "status":"resolved", "resolution_ref": resolution_ref})
        }
        FactoryCommand::LinkSupportToChangeRequest {
            support_item_id,
            change_request_id,
            ..
        } => {
            if !raios_core::db::link_support_to_change_request(
                &tx,
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
            serde_json::json!({"support_item_id": support_item_id, "change_request_id": change_request_id, "relation":"raised_by"})
        }
        FactoryCommand::RequestImpactAssessment { .. } => {
            return Err(Problem::not_implemented(
                "Impact assessment mutation is not implemented in Phase 3",
            ));
        }
    };

    tx.execute(
        "INSERT INTO cp_idempotency (idempotency_key, payload_hash, command_type, result_json, status)
         VALUES (?1, ?2, ?3, ?4, 'COMPLETED')",
        params![key, payload_hash, command_name, result.to_string()],
    )
    .map_err(|error| Problem::internal(format!("Idempotency record failed: {error}")))?;
    raios_core::security::record_tool_decision(
        &tx,
        command_name,
        &payload_hash,
        "product_factory",
        "tool_confirm",
        actor.subject(),
    )
    .map_err(|error| Problem::internal(format!("Audit write failed: {error}")))?;
    tx.commit()
        .map_err(|error| Problem::internal(format!("Commit failed: {error}")))?;
    Ok(result)
}

fn factory_command_name(command: &FactoryCommand) -> &'static str {
    match command {
        FactoryCommand::CreateWorkspace { .. } => "factory_create_workspace",
        FactoryCommand::CreateProductDraft { .. } => "factory_create_product_draft",
        FactoryCommand::SetProductMode { .. } => "factory_set_product_mode",
        FactoryCommand::ScaffoldProject { .. } => "factory_scaffold_project",
        FactoryCommand::AttachExistingProject { .. } => "factory_attach_existing_project",
        FactoryCommand::StartIntake { .. } => "factory_start_intake",
        FactoryCommand::RecordIntakeAnswer { .. } => "factory_record_intake_answer",
        FactoryCommand::CreateCharterDraft { .. } => "factory_create_charter_draft",
        FactoryCommand::GenerateCharterDraft { .. } => "factory_generate_charter_draft",
        FactoryCommand::CreateRequirementDraft { .. } => "factory_create_requirement_draft",
        FactoryCommand::SubmitChangeRequest { .. } => "factory_submit_change_request",
        FactoryCommand::AssessChangeRequest { .. } => "factory_assess_change_request",
        FactoryCommand::ResolveImpactAssessment { .. } => "factory_resolve_impact_assessment",
        FactoryCommand::ApplyApprovedRequirementChange { .. } => "factory_apply_requirement_change",
        FactoryCommand::CreatePlanDraft { .. } => "factory_create_plan_draft",
        FactoryCommand::ApprovePlan { .. } => "factory_approve_plan",
        FactoryCommand::MaterializePlannedCycle { .. } => "factory_materialize_planned_cycle",
        FactoryCommand::PauseCycle { .. } => "factory_pause_cycle",
        FactoryCommand::ResumeCycle { .. } => "factory_resume_cycle",
        FactoryCommand::CancelCycle { .. } => "factory_cancel_cycle",
        FactoryCommand::MaterializeStageTaskGraph { .. } => "factory_materialize_stage_task_graph",
        FactoryCommand::ActivateApprovedStage { .. } => "factory_activate_approved_stage",
        FactoryCommand::RecordStageEvidence { .. } => "factory_record_stage_evidence",
        FactoryCommand::LinkStageEvidenceToRequirement { .. } => {
            "factory_link_evidence_requirement"
        }
        FactoryCommand::CompleteStage { .. } => "factory_complete_stage",
        FactoryCommand::InspectReleaseReadiness { .. } => "factory_inspect_release_readiness",
        FactoryCommand::CreateQualityProfile { .. } => "factory_create_quality_profile",
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { .. } => {
            "factory_ensure_react_native_closed_testing_quality_profile"
        }
        FactoryCommand::RecordQualityCheck { .. } => "factory_record_quality_check",
        FactoryCommand::CreateReleaseDraft { .. } => "factory_create_release_draft",
        FactoryCommand::ApproveClosedTestingRelease { .. } => {
            "factory_approve_closed_testing_release"
        }
        FactoryCommand::CreateSupportItem { .. } => "factory_create_support_item",
        FactoryCommand::InspectSupportOverview { .. } => "factory_inspect_support_overview",
        FactoryCommand::TriageSupportItem { .. } => "factory_triage_support_item",
        FactoryCommand::ResolveSupportItem { .. } => "factory_resolve_support_item",
        FactoryCommand::LinkSupportToChangeRequest { .. } => "factory_link_support_change",
        FactoryCommand::RequestImpactAssessment { .. } => "factory_request_impact_assessment",
    }
}

fn validate_command(command: &FactoryCommand) -> Result<(), Problem> {
    let valid = match command {
        FactoryCommand::CreateWorkspace {
            name,
            idempotency_key,
        } => valid_text(name, 120) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateProductDraft {
            workspace_id,
            title,
            idempotency_key,
        } => {
            valid_identifier(workspace_id)
                && valid_text(title, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::SetProductMode {
            product_id,
            idempotency_key,
            ..
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ScaffoldProject {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::AttachExistingProject {
            product_id,
            project_path,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_absolute_project_path(project_path)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::StartIntake {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RecordIntakeAnswer {
            session_id,
            question_key,
            response,
            idempotency_key,
        } => {
            valid_identifier(session_id)
                && valid_identifier(question_key)
                && valid_text(response, 8_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateCharterDraft {
            product_id,
            content,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(content, 32_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::GenerateCharterDraft {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateRequirementDraft {
            product_id,
            stable_key,
            content,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_identifier(stable_key)
                && valid_text(content, 16_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::SubmitChangeRequest {
            product_id,
            summary,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(summary, 4_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::AssessChangeRequest {
            change_request_id,
            idempotency_key,
        } => valid_identifier(change_request_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ResolveImpactAssessment {
            assessment_id,
            idempotency_key,
            ..
        } => valid_identifier(assessment_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ApplyApprovedRequirementChange {
            assessment_id,
            requirement_id,
            content,
            idempotency_key,
        } => {
            valid_identifier(assessment_id)
                && valid_identifier(requirement_id)
                && valid_text(content, 16_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreatePlanDraft {
            product_id,
            title,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(title, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ApprovePlan {
            plan_id,
            idempotency_key,
        }
        | FactoryCommand::MaterializePlannedCycle {
            plan_id,
            idempotency_key,
        } => valid_identifier(plan_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::PauseCycle {
            cycle_id,
            idempotency_key,
        }
        | FactoryCommand::ResumeCycle {
            cycle_id,
            idempotency_key,
        }
        | FactoryCommand::CancelCycle {
            cycle_id,
            idempotency_key,
        } => valid_identifier(cycle_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::MaterializeStageTaskGraph {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ActivateApprovedStage {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::RecordStageEvidence {
            cycle_id,
            stage,
            content_ref,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_text(content_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::LinkStageEvidenceToRequirement {
            evidence_id,
            requirement_id,
            idempotency_key,
        } => {
            valid_identifier(evidence_id)
                && valid_identifier(requirement_id)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CompleteStage {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::InspectReleaseReadiness {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateQualityProfile {
            product_id,
            name,
            idempotency_key,
            ..
        } => {
            valid_identifier(product_id)
                && valid_text(name, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RecordQualityCheck {
            profile_id,
            evidence_ref,
            idempotency_key,
            ..
        } => {
            valid_identifier(profile_id)
                && valid_text(evidence_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateSupportItem {
            product_id,
            source_kind,
            summary,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_identifier(source_kind)
                && valid_text(summary, 4_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::InspectSupportOverview {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::TriageSupportItem {
            support_item_id,
            idempotency_key,
        } => valid_identifier(support_item_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ResolveSupportItem {
            support_item_id,
            resolution_ref,
            idempotency_key,
        } => {
            valid_identifier(support_item_id)
                && valid_text(resolution_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::LinkSupportToChangeRequest {
            support_item_id,
            change_request_id,
            idempotency_key,
        } => {
            valid_identifier(support_item_id)
                && valid_identifier(change_request_id)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateReleaseDraft {
            product_id,
            build_ref,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(build_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ApproveClosedTestingRelease {
            release_id,
            idempotency_key,
        } => valid_identifier(release_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RequestImpactAssessment {
            change_request_id,
            idempotency_key,
        } => valid_identifier(change_request_id) && valid_idempotency_key(idempotency_key),
    };
    if valid {
        Ok(())
    } else {
        Err(Problem::invalid_input(
            "Factory command contains an empty, oversized, or invalid field",
        ))
    }
}

fn valid_text(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_len
        && !value.contains('\0')
        && raios_core::security::looks_like_secret(value).is_none()
}

fn valid_identifier(value: &str) -> bool {
    valid_text(value, 128)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_absolute_project_path(value: &str) -> bool {
    valid_text(value, 4_096) && Path::new(value).is_absolute()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExistingGitProject {
    project_path: String,
    remote: String,
    revision: String,
}

/// Inspect an existing Git worktree without mutating it. The command accepts
/// only repository roots (not arbitrary subdirectories), captures a stable
/// origin and HEAD SHA, and never persists a credential-bearing HTTP remote.
fn inspect_existing_git_project(project_path: &str) -> Result<ExistingGitProject, Problem> {
    let canonical_path = std::fs::canonicalize(project_path)
        .map_err(|_| Problem::invalid_input("Project path does not exist or is not accessible"))?;
    if !canonical_path.is_dir() || !canonical_path.join(".git").exists() {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }

    let canonical = canonical_path.to_string_lossy().to_string();
    let inside_worktree = git_output(&canonical, &["rev-parse", "--is-inside-work-tree"])?;
    if inside_worktree != "true" {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }
    let top_level = git_output(&canonical, &["rev-parse", "--show-toplevel"])?;
    if top_level != canonical {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }
    let remote = git_output(&canonical, &["config", "--get", "remote.origin.url"])?;
    if remote.is_empty() || remote.len() > 2_000 || remote_has_embedded_http_credentials(&remote) {
        return Err(Problem::invalid_input(
            "Project origin remote is missing or contains embedded credentials",
        ));
    }
    let revision = git_output(&canonical, &["rev-parse", "HEAD"])?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Problem::invalid_input(
            "Project HEAD must resolve to a full Git commit SHA",
        ));
    }

    Ok(ExistingGitProject {
        project_path: canonical,
        remote,
        revision,
    })
}

fn git_output(project_path: &str, args: &[&str]) -> Result<String, Problem> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map_err(|_| Problem::invalid_input("Git is required to attach an existing project"))?;
    if !output.status.success() {
        return Err(Problem::invalid_input(
            "Project path is not a valid Git worktree with the required metadata",
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| Problem::invalid_input("Git metadata must be valid UTF-8"))
}

fn remote_has_embedded_http_credentials(remote: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .filter_map(|scheme| remote.strip_prefix(scheme))
        .any(|rest| {
            rest.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
}

fn is_factory_stage(value: &str) -> bool {
    raios_core::db::FACTORY_LIFECYCLE_STAGES.contains(&value)
}

fn valid_idempotency_key(value: &str) -> bool {
    valid_identifier(value) && value.len() >= 8
}

fn factory_project_category(platform: &str) -> &'static str {
    let platform = platform.to_ascii_lowercase();
    if platform.contains("flutter")
        || platform.contains("react native")
        || platform.contains("android")
        || platform.contains("ios")
    {
        "mobile"
    } else if platform.contains("react") || platform.contains("next") || platform.contains("web") {
        "web"
    } else if platform.contains("esp") || platform.contains("iot") || platform.contains("embedded")
    {
        "embedded"
    } else if platform.contains("ai") || platform.contains("model") || platform.contains("data") {
        "ai"
    } else {
        "tools"
    }
}

fn factory_project_slug(title: &str) -> String {
    let slug: String = title
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "factory-project".into()
    } else {
        slug.into()
    }
}

#[cfg(test)]
mod tests {
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
}
