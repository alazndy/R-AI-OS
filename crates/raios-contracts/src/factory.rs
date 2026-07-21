//! Serialization-only contracts for the Product Factory domain.
//!
//! These contracts are intentionally separate from the live control-plane
//! `Query` and `Command` enums until a reviewed transport integration exists.

use serde::{Deserialize, Serialize};

/// Product Factory operating posture. Quick reduces discovery friction; it
/// never bypasses the human approval and release controls of Governed mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactoryMode {
    Quick,
    Governed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_query_type", content = "payload")]
pub enum FactoryQuery {
    GetOverview { product_id: String },
    GetImpactAssessment { change_request_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_command_type", content = "payload")]
pub enum FactoryCommand {
    CreateWorkspace {
        name: String,
        idempotency_key: String,
    },
    CreateProductDraft {
        workspace_id: String,
        title: String,
        idempotency_key: String,
    },
    SetProductMode {
        product_id: String,
        mode: FactoryMode,
        idempotency_key: String,
    },
    ScaffoldProject {
        product_id: String,
        idempotency_key: String,
    },
    StartIntake {
        product_id: String,
        idempotency_key: String,
    },
    RecordIntakeAnswer {
        session_id: String,
        question_key: String,
        response: String,
        idempotency_key: String,
    },
    CreateCharterDraft {
        product_id: String,
        content: String,
        idempotency_key: String,
    },
    GenerateCharterDraft {
        product_id: String,
        idempotency_key: String,
    },
    CreateRequirementDraft {
        product_id: String,
        stable_key: String,
        content: String,
        idempotency_key: String,
    },
    SubmitChangeRequest {
        product_id: String,
        summary: String,
        idempotency_key: String,
    },
    AssessChangeRequest {
        change_request_id: String,
        idempotency_key: String,
    },
    ResolveImpactAssessment {
        assessment_id: String,
        approved: bool,
        idempotency_key: String,
    },
    ApplyApprovedRequirementChange {
        assessment_id: String,
        requirement_id: String,
        content: String,
        idempotency_key: String,
    },
    CreatePlanDraft {
        product_id: String,
        title: String,
        idempotency_key: String,
    },
    ApprovePlan {
        plan_id: String,
        idempotency_key: String,
    },
    MaterializePlannedCycle {
        plan_id: String,
        idempotency_key: String,
    },
    PauseCycle {
        cycle_id: String,
        idempotency_key: String,
    },
    ResumeCycle {
        cycle_id: String,
        idempotency_key: String,
    },
    CancelCycle {
        cycle_id: String,
        idempotency_key: String,
    },
    MaterializeStageTaskGraph {
        cycle_id: String,
        stage: String,
        idempotency_key: String,
    },
    ActivateApprovedStage {
        cycle_id: String,
        stage: String,
        idempotency_key: String,
    },
    RecordStageEvidence {
        cycle_id: String,
        stage: String,
        content_ref: String,
        idempotency_key: String,
    },
    LinkStageEvidenceToRequirement {
        evidence_id: String,
        requirement_id: String,
        idempotency_key: String,
    },
    CompleteStage {
        cycle_id: String,
        stage: String,
        idempotency_key: String,
    },
    InspectReleaseReadiness {
        product_id: String,
        idempotency_key: String,
    },
    CreateQualityProfile {
        product_id: String,
        name: String,
        required: bool,
        idempotency_key: String,
    },
    EnsureReactNativeClosedTestingQualityProfile {
        product_id: String,
        idempotency_key: String,
    },
    RecordQualityCheck {
        profile_id: String,
        passed: bool,
        evidence_ref: String,
        idempotency_key: String,
    },
    CreateReleaseDraft {
        product_id: String,
        build_ref: String,
        idempotency_key: String,
    },
    ApproveClosedTestingRelease {
        release_id: String,
        idempotency_key: String,
    },
    CreateSupportItem {
        product_id: String,
        source_kind: String,
        summary: String,
        idempotency_key: String,
    },
    InspectSupportOverview {
        product_id: String,
        idempotency_key: String,
    },
    TriageSupportItem {
        support_item_id: String,
        idempotency_key: String,
    },
    ResolveSupportItem {
        support_item_id: String,
        resolution_ref: String,
        idempotency_key: String,
    },
    LinkSupportToChangeRequest {
        support_item_id: String,
        change_request_id: String,
        idempotency_key: String,
    },
    RequestImpactAssessment {
        change_request_id: String,
        idempotency_key: String,
    },
}

impl FactoryCommand {
    pub fn idempotency_key(&self) -> &str {
        match self {
            Self::CreateWorkspace {
                idempotency_key, ..
            }
            | Self::CreateProductDraft {
                idempotency_key, ..
            }
            | Self::SetProductMode {
                idempotency_key, ..
            }
            | Self::ScaffoldProject {
                idempotency_key, ..
            }
            | Self::StartIntake {
                idempotency_key, ..
            }
            | Self::RecordIntakeAnswer {
                idempotency_key, ..
            }
            | Self::CreateCharterDraft {
                idempotency_key, ..
            }
            | Self::GenerateCharterDraft {
                idempotency_key, ..
            }
            | Self::CreateRequirementDraft {
                idempotency_key, ..
            }
            | Self::SubmitChangeRequest {
                idempotency_key, ..
            }
            | Self::AssessChangeRequest {
                idempotency_key, ..
            }
            | Self::ResolveImpactAssessment {
                idempotency_key, ..
            }
            | Self::ApplyApprovedRequirementChange {
                idempotency_key, ..
            }
            | Self::CreatePlanDraft {
                idempotency_key, ..
            }
            | Self::ApprovePlan {
                idempotency_key, ..
            }
            | Self::MaterializePlannedCycle {
                idempotency_key, ..
            }
            | Self::PauseCycle {
                idempotency_key, ..
            }
            | Self::ResumeCycle {
                idempotency_key, ..
            }
            | Self::CancelCycle {
                idempotency_key, ..
            }
            | Self::MaterializeStageTaskGraph {
                idempotency_key, ..
            }
            | Self::ActivateApprovedStage {
                idempotency_key, ..
            }
            | Self::RecordStageEvidence {
                idempotency_key, ..
            }
            | Self::LinkStageEvidenceToRequirement {
                idempotency_key, ..
            }
            | Self::CompleteStage {
                idempotency_key, ..
            }
            | Self::InspectReleaseReadiness {
                idempotency_key, ..
            }
            | Self::CreateQualityProfile {
                idempotency_key, ..
            }
            | Self::EnsureReactNativeClosedTestingQualityProfile {
                idempotency_key, ..
            }
            | Self::RecordQualityCheck {
                idempotency_key, ..
            }
            | Self::CreateReleaseDraft {
                idempotency_key, ..
            }
            | Self::ApproveClosedTestingRelease {
                idempotency_key, ..
            }
            | Self::CreateSupportItem {
                idempotency_key, ..
            }
            | Self::InspectSupportOverview {
                idempotency_key, ..
            }
            | Self::TriageSupportItem {
                idempotency_key, ..
            }
            | Self::ResolveSupportItem {
                idempotency_key, ..
            }
            | Self::LinkSupportToChangeRequest {
                idempotency_key, ..
            }
            | Self::RequestImpactAssessment {
                idempotency_key, ..
            } => idempotency_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_event_type", content = "payload")]
pub enum FactoryEvent {
    WorkspaceCreated {
        workspace_id: String,
    },
    ProductDraftCreated {
        product_id: String,
    },
    IntakeStarted {
        session_id: String,
    },
    IntakeAnswerRecorded {
        session_id: String,
        question_key: String,
    },
    CharterDraftCreated {
        charter_revision_id: String,
    },
    CharterDraftGenerated {
        charter_revision_id: String,
    },
    RequirementDraftCreated {
        requirement_id: String,
    },
    ChangeRequestSubmitted {
        change_request_id: String,
    },
    ImpactAssessmentReady {
        change_request_id: String,
        assessment_id: String,
    },
    ApprovalRequired {
        product_id: String,
        approval_kind: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FactoryOverviewSnapshot {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub product_count: u32,
    #[serde(default)]
    pub active_cycle_count: u32,
    #[serde(default)]
    pub pending_change_request_count: u32,
    #[serde(default)]
    pub open_support_items: u32,
    #[serde(default)]
    pub blocking_quality_profiles: u32,
    #[serde(default)]
    pub release_drafts: u32,
    #[serde(default)]
    pub completed_verify_stages: u32,
    #[serde(default)]
    pub approved_closed_testing_releases: u32,
    #[serde(default)]
    pub latest_product: Option<FactoryProductSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FactoryProductSummaryDto {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub stack: Option<String>,
    #[serde(default)]
    pub scaffold_state: String,
    #[serde(default)]
    pub quality_blockers: u32,
    #[serde(default)]
    pub release_blockers: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_contracts_round_trip_through_serde() {
        let command = FactoryCommand::CreateProductDraft {
            workspace_id: "workspace-1".into(),
            title: "Pilot".into(),
            idempotency_key: "idem-1".into(),
        };
        let encoded = serde_json::to_string(&command).unwrap();
        let decoded: FactoryCommand = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, command);
        assert_eq!(command.idempotency_key(), "idem-1");
    }
}
