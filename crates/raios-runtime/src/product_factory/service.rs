use raios_contracts::{FactoryCommand, Problem};
use raios_core::product_factory::{
    FactoryInvariantError, FactoryProduct, FactoryWorkspace, ImpactAssessmentSummary,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

use crate::control_plane::service::{init_idempotency_table, ControlActor};
mod commands;
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

    let payload_hash = payload_hash(command)?;
    let command_name = factory_command_name(command);
    let key = command.idempotency_key();
    if let Some(replayed) = replayed_result(conn, key, &payload_hash)? {
        return Ok(replayed);
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|error| Problem::internal(format!("Failed starting transaction: {error}")))?;
    let result = run_command(&tx, actor, command, override_config)?;
    record_outcome(&tx, actor, key, &payload_hash, command_name, &result)?;
    tx.commit()
        .map_err(|error| Problem::internal(format!("Commit failed: {error}")))?;
    Ok(result)
}

/// SHA-256 of the command's serialized payload: detects one idempotency key
/// being reused for a different command.
fn payload_hash(command: &FactoryCommand) -> Result<String, Problem> {
    let payload = serde_json::to_string(command)
        .map_err(|error| Problem::invalid_input(format!("Failed serializing command: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(payload.as_bytes())))
}

/// Idempotent replay: `Ok(Some(result))` if this key already ran with the same
/// payload, `Ok(None)` if it has not. Reusing the key for a different command
/// is refused.
fn replayed_result(
    conn: &Connection,
    key: &str,
    payload_hash: &str,
) -> Result<Option<serde_json::Value>, Problem> {
    let cached: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT payload_hash, result_json FROM cp_idempotency WHERE idempotency_key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| Problem::internal(error.to_string()))?;
    let Some((stored_hash, cached_result)) = cached else {
        return Ok(None);
    };
    if !stored_hash.is_empty() && stored_hash != payload_hash {
        return Err(Problem::invalid_input(
            "Idempotency key is already bound to a different Product Factory command",
        ));
    }
    cached_result
        .and_then(|json| serde_json::from_str(&json).ok())
        .map(Some)
        .ok_or_else(|| Problem::internal("Idempotency record is missing a valid result"))
}

/// Routes a command to its handler in [`commands`]. Exhaustive on purpose: a new
/// `FactoryCommand` variant does not compile until it is routed here.
fn run_command(
    tx: &Transaction<'_>,
    actor: &ControlActor,
    command: &FactoryCommand,
    override_config: Option<&raios_core::config::Config>,
) -> Result<serde_json::Value, Problem> {
    match command {
        FactoryCommand::CreateWorkspace { name, .. } => commands::create_workspace(tx, actor, name),
        FactoryCommand::CreateProductDraft {
            workspace_id,
            title,
            ..
        } => commands::create_product_draft(tx, actor, workspace_id, title),
        FactoryCommand::SetProductMode {
            product_id, mode, ..
        } => commands::set_product_mode(tx, actor, product_id, mode),
        FactoryCommand::ScaffoldProject { product_id, .. } => {
            commands::scaffold_project(tx, actor, override_config, product_id)
        }
        FactoryCommand::AttachExistingProject {
            product_id,
            project_path,
            ..
        } => commands::attach_existing_project(tx, actor, product_id, project_path),
        FactoryCommand::StartIntake { product_id, .. } => {
            commands::start_intake(tx, actor, product_id)
        }
        FactoryCommand::RecordIntakeAnswer {
            session_id,
            question_key,
            response,
            ..
        } => commands::record_intake_answer(tx, actor, session_id, question_key, response),
        FactoryCommand::CreateCharterDraft {
            product_id,
            content,
            ..
        } => commands::create_charter_draft(tx, actor, product_id, content),
        FactoryCommand::GenerateCharterDraft { product_id, .. } => {
            commands::generate_charter_draft(tx, actor, product_id)
        }
        FactoryCommand::CreateRequirementDraft {
            product_id,
            stable_key,
            content,
            ..
        } => commands::create_requirement_draft(tx, actor, product_id, stable_key, content),
        FactoryCommand::SubmitChangeRequest {
            product_id,
            summary,
            ..
        } => commands::submit_change_request(tx, actor, product_id, summary),
        FactoryCommand::AssessChangeRequest {
            change_request_id, ..
        } => commands::assess_change_request(tx, actor, change_request_id),
        FactoryCommand::ResolveImpactAssessment {
            assessment_id,
            approved,
            ..
        } => commands::resolve_impact_assessment(tx, actor, assessment_id, approved),
        FactoryCommand::ApplyApprovedRequirementChange {
            assessment_id,
            requirement_id,
            content,
            ..
        } => commands::apply_approved_requirement_change(
            tx,
            actor,
            assessment_id,
            requirement_id,
            content,
        ),
        FactoryCommand::CreatePlanDraft {
            product_id, title, ..
        } => commands::create_plan_draft(tx, actor, product_id, title),
        FactoryCommand::ApprovePlan { plan_id, .. } => commands::approve_plan(tx, actor, plan_id),
        FactoryCommand::MaterializePlannedCycle { plan_id, .. } => {
            commands::materialize_planned_cycle(tx, actor, plan_id)
        }
        FactoryCommand::PauseCycle { cycle_id, .. } => commands::pause_cycle(tx, actor, cycle_id),
        FactoryCommand::ResumeCycle { cycle_id, .. } => commands::resume_cycle(tx, actor, cycle_id),
        FactoryCommand::CancelCycle { cycle_id, .. } => commands::cancel_cycle(tx, actor, cycle_id),
        FactoryCommand::MaterializeStageTaskGraph {
            cycle_id, stage, ..
        } => commands::materialize_stage_task_graph(tx, actor, cycle_id, stage),
        FactoryCommand::ActivateApprovedStage {
            cycle_id, stage, ..
        } => commands::activate_approved_stage(tx, actor, cycle_id, stage),
        FactoryCommand::RecordStageEvidence {
            cycle_id,
            stage,
            content_ref,
            ..
        } => commands::record_stage_evidence(tx, actor, cycle_id, stage, content_ref),
        FactoryCommand::LinkStageEvidenceToRequirement {
            evidence_id,
            requirement_id,
            ..
        } => commands::link_stage_evidence_to_requirement(tx, actor, evidence_id, requirement_id),
        FactoryCommand::CompleteStage {
            cycle_id, stage, ..
        } => commands::complete_stage(tx, actor, cycle_id, stage),
        FactoryCommand::InspectReleaseReadiness { product_id, .. } => {
            commands::inspect_release_readiness(tx, actor, product_id)
        }
        FactoryCommand::CreateQualityProfile {
            product_id,
            name,
            required,
            ..
        } => commands::create_quality_profile(tx, actor, product_id, name, required),
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { product_id, .. } => {
            commands::ensure_react_native_closed_testing_quality_profile(tx, actor, product_id)
        }
        FactoryCommand::RecordQualityCheck {
            profile_id,
            passed,
            evidence_ref,
            ..
        } => commands::record_quality_check(tx, actor, profile_id, passed, evidence_ref),
        FactoryCommand::CreateReleaseDraft {
            product_id,
            build_ref,
            ..
        } => commands::create_release_draft(tx, actor, product_id, build_ref),
        FactoryCommand::ApproveClosedTestingRelease { release_id, .. } => {
            commands::approve_closed_testing_release(tx, actor, release_id)
        }
        FactoryCommand::CreateSupportItem {
            product_id,
            source_kind,
            summary,
            ..
        } => commands::create_support_item(tx, actor, product_id, source_kind, summary),
        FactoryCommand::InspectSupportOverview { product_id, .. } => {
            commands::inspect_support_overview(tx, actor, product_id)
        }
        FactoryCommand::TriageSupportItem {
            support_item_id, ..
        } => commands::triage_support_item(tx, actor, support_item_id),
        FactoryCommand::ResolveSupportItem {
            support_item_id,
            resolution_ref,
            ..
        } => commands::resolve_support_item(tx, actor, support_item_id, resolution_ref),
        FactoryCommand::LinkSupportToChangeRequest {
            support_item_id,
            change_request_id,
            ..
        } => {
            commands::link_support_to_change_request(tx, actor, support_item_id, change_request_id)
        }
        FactoryCommand::RequestImpactAssessment { .. } => commands::request_impact_assessment(),
    }
}

/// Writes the idempotency record and the audit entry in the command's own
/// transaction, so a command is never recorded without its audit trail (or the
/// reverse).
fn record_outcome(
    tx: &Connection,
    actor: &ControlActor,
    key: &str,
    payload_hash: &str,
    command_name: &str,
    result: &serde_json::Value,
) -> Result<(), Problem> {
    tx.execute(
        "INSERT INTO cp_idempotency (idempotency_key, payload_hash, command_type, result_json, status)
         VALUES (?1, ?2, ?3, ?4, 'COMPLETED')",
        params![key, payload_hash, command_name, result.to_string()],
    )
    .map_err(|error| Problem::internal(format!("Idempotency record failed: {error}")))?;
    raios_core::security::record_tool_decision(
        tx,
        command_name,
        payload_hash,
        "product_factory",
        "tool_confirm",
        actor.subject(),
    )
    .map_err(|error| Problem::internal(format!("Audit write failed: {error}")))?;
    Ok(())
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
