//! Product Factory MCP tools.
//!
//! Agents share the same typed Factory contract as the TUI. This module is a
//! policy boundary: it deliberately exposes only non-approval lifecycle work
//! and never accepts shell commands, secrets, or arbitrary database input.

use raios_contracts::FactoryCommand;
use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn tool_factory_overview(&self) -> Result<Value, String> {
        let conn = raios_core::db::open_db().map_err(|error| error.to_string())?;
        let snapshot = raios_runtime::control_plane::service::load_work_snapshot(&conn)
            .map_err(|error| format!("Failed loading Product Factory overview: {error}"))?;
        Ok(tool_response(json!(snapshot.factory)))
    }

    pub(super) fn tool_factory_execute(&self, args: &Value) -> Result<Value, String> {
        let command_value = args.get("command").ok_or("missing command")?;
        let command: FactoryCommand = serde_json::from_value(command_value.clone())
            .map_err(|error| format!("invalid FactoryCommand envelope: {error}"))?;

        if !agent_may_execute(&command) {
            return Err(format!(
                "factory_approval_required:{} must be approved by the human owner in the Factory UI",
                command_kind(&command)
            ));
        }

        let factory_enabled = raios_core::config::Config::load()
            .map(|config| config.factory.enabled)
            .unwrap_or(false);
        let actor = raios_runtime::control_plane::service::ControlActor::local_session();
        let mut conn = raios_core::db::open_db().map_err(|error| error.to_string())?;
        let result = raios_runtime::product_factory::dispatch_factory_command(
            &mut conn,
            &actor,
            factory_enabled,
            &command,
        )
        .map_err(|problem| format!("factory_command_failed:{}", problem.message))?;

        Ok(tool_response(json!({
            "command": command_kind(&command),
            "result": result,
        })))
    }
}

fn tool_response(value: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
        }]
    })
}

/// Human approval is mandatory for actions that change an approved plan,
/// destroy/stop work, or make an external-release decision. Stage execution
/// itself remains protected by the Factory task-graph approval gate.
fn agent_may_execute(command: &FactoryCommand) -> bool {
    matches!(
        command,
        FactoryCommand::CreateWorkspace { .. }
            | FactoryCommand::CreateProductDraft { .. }
            | FactoryCommand::SetProductMode { .. }
            | FactoryCommand::StartIntake { .. }
            | FactoryCommand::RecordIntakeAnswer { .. }
            | FactoryCommand::CreateCharterDraft { .. }
            | FactoryCommand::GenerateCharterDraft { .. }
            | FactoryCommand::CreateRequirementDraft { .. }
            | FactoryCommand::SubmitChangeRequest { .. }
            | FactoryCommand::AssessChangeRequest { .. }
            | FactoryCommand::CreatePlanDraft { .. }
            | FactoryCommand::MaterializePlannedCycle { .. }
            | FactoryCommand::PauseCycle { .. }
            | FactoryCommand::ResumeCycle { .. }
            | FactoryCommand::MaterializeStageTaskGraph { .. }
            | FactoryCommand::RecordStageEvidence { .. }
            | FactoryCommand::LinkStageEvidenceToRequirement { .. }
            | FactoryCommand::InspectReleaseReadiness { .. }
            | FactoryCommand::CreateQualityProfile { .. }
            | FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { .. }
            | FactoryCommand::RecordQualityCheck { .. }
            | FactoryCommand::CreateReleaseDraft { .. }
            | FactoryCommand::CreateSupportItem { .. }
            | FactoryCommand::InspectSupportOverview { .. }
            | FactoryCommand::TriageSupportItem { .. }
            | FactoryCommand::ResolveSupportItem { .. }
            | FactoryCommand::LinkSupportToChangeRequest { .. }
    )
}

fn command_kind(command: &FactoryCommand) -> &'static str {
    match command {
        FactoryCommand::CreateWorkspace { .. } => "create_workspace",
        FactoryCommand::CreateProductDraft { .. } => "create_product_draft",
        FactoryCommand::SetProductMode { .. } => "set_product_mode",
        FactoryCommand::ScaffoldProject { .. } => "scaffold_project",
        FactoryCommand::StartIntake { .. } => "start_intake",
        FactoryCommand::RecordIntakeAnswer { .. } => "record_intake_answer",
        FactoryCommand::CreateCharterDraft { .. } => "create_charter_draft",
        FactoryCommand::GenerateCharterDraft { .. } => "generate_charter_draft",
        FactoryCommand::CreateRequirementDraft { .. } => "create_requirement_draft",
        FactoryCommand::SubmitChangeRequest { .. } => "submit_change_request",
        FactoryCommand::AssessChangeRequest { .. } => "assess_change_request",
        FactoryCommand::ResolveImpactAssessment { .. } => "resolve_impact_assessment",
        FactoryCommand::ApplyApprovedRequirementChange { .. } => "apply_requirement_change",
        FactoryCommand::CreatePlanDraft { .. } => "create_plan_draft",
        FactoryCommand::ApprovePlan { .. } => "approve_plan",
        FactoryCommand::MaterializePlannedCycle { .. } => "materialize_planned_cycle",
        FactoryCommand::PauseCycle { .. } => "pause_cycle",
        FactoryCommand::ResumeCycle { .. } => "resume_cycle",
        FactoryCommand::CancelCycle { .. } => "cancel_cycle",
        FactoryCommand::MaterializeStageTaskGraph { .. } => "materialize_stage_task_graph",
        FactoryCommand::ActivateApprovedStage { .. } => "activate_approved_stage",
        FactoryCommand::RecordStageEvidence { .. } => "record_stage_evidence",
        FactoryCommand::LinkStageEvidenceToRequirement { .. } => "link_evidence_to_requirement",
        FactoryCommand::CompleteStage { .. } => "complete_stage",
        FactoryCommand::InspectReleaseReadiness { .. } => "inspect_release_readiness",
        FactoryCommand::CreateQualityProfile { .. } => "create_quality_profile",
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { .. } => {
            "ensure_react_native_quality_profile"
        }
        FactoryCommand::RecordQualityCheck { .. } => "record_quality_check",
        FactoryCommand::CreateReleaseDraft { .. } => "create_release_draft",
        FactoryCommand::ApproveClosedTestingRelease { .. } => "approve_closed_testing_release",
        FactoryCommand::CreateSupportItem { .. } => "create_support_item",
        FactoryCommand::InspectSupportOverview { .. } => "inspect_support_overview",
        FactoryCommand::TriageSupportItem { .. } => "triage_support_item",
        FactoryCommand::ResolveSupportItem { .. } => "resolve_support_item",
        FactoryCommand::LinkSupportToChangeRequest { .. } => "link_support_to_change_request",
        FactoryCommand::RequestImpactAssessment { .. } => "request_impact_assessment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_policy_allows_drafts_but_blocks_human_decisions() {
        assert!(agent_may_execute(&FactoryCommand::CreateWorkspace {
            name: "Mobile products".into(),
            idempotency_key: "factory-agent-001".into(),
        }));
        assert!(!agent_may_execute(&FactoryCommand::ApprovePlan {
            plan_id: "plan-1".into(),
            idempotency_key: "factory-agent-002".into(),
        }));
        assert!(!agent_may_execute(
            &FactoryCommand::ApproveClosedTestingRelease {
                release_id: "release-1".into(),
                idempotency_key: "factory-agent-003".into(),
            }
        ));
    }
}
