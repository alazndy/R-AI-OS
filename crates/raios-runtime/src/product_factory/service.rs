use raios_contracts::{FactoryCommand, Problem};
use raios_core::product_factory::{
    FactoryInvariantError, FactoryProduct, FactoryWorkspace, ImpactAssessmentSummary,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

use crate::control_plane::service::{init_idempotency_table, ControlActor};
mod validate;
use validate::*;

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
                    vault_path: None,
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

#[cfg(test)]
mod tests;
