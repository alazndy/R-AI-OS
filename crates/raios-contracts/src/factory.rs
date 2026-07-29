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
    /// Accelerated intake mode with minimal discovery questions.
    Quick,
    /// Full governed mode with explicit multi-step approval cycles.
    Governed,
}

/// Read-only queries for the Product Factory domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_query_type", content = "payload")]
pub enum FactoryQuery {
    /// Retrieve product overview summary.
    GetOverview {
        /// Target product identifier.
        product_id: String,
    },
    /// Retrieve impact assessment report for a change request.
    GetImpactAssessment {
        /// Target change request identifier.
        change_request_id: String,
    },
}

/// State-modifying commands for the Product Factory domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_command_type", content = "payload")]
pub enum FactoryCommand {
    /// Create a new workspace.
    CreateWorkspace {
        /// Name of the new workspace.
        name: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a new product draft in a workspace.
    CreateProductDraft {
        /// Parent workspace identifier.
        workspace_id: String,
        /// Product title.
        title: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Change the operating mode of a product.
    SetProductMode {
        /// Target product identifier.
        product_id: String,
        /// New operating posture mode.
        mode: FactoryMode,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Scaffold a new project repository structure for a product.
    ScaffoldProject {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Bind an already-existing local Git worktree to a Factory product.
    /// The runtime canonicalizes and verifies the path before persisting it.
    AttachExistingProject {
        /// Target product identifier.
        product_id: String,
        /// Absolute local path to the existing Git repository.
        project_path: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Start a new product intake session.
    StartIntake {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Record an answer to an intake questionnaire item.
    RecordIntakeAnswer {
        /// Active intake session identifier.
        session_id: String,
        /// Key identifying the answered question.
        question_key: String,
        /// Response value provided.
        response: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a product charter draft manually.
    CreateCharterDraft {
        /// Target product identifier.
        product_id: String,
        /// Markdown content of the charter.
        content: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Auto-generate a product charter draft from completed intake answers.
    GenerateCharterDraft {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a draft requirement entry.
    CreateRequirementDraft {
        /// Target product identifier.
        product_id: String,
        /// Stable requirement identifier key (e.g. `"REQ-001"`).
        stable_key: String,
        /// Requirement description content.
        content: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Submit a new change request against a product.
    SubmitChangeRequest {
        /// Target product identifier.
        product_id: String,
        /// Summary description of the proposed change.
        summary: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Trigger an automated impact assessment for a change request.
    AssessChangeRequest {
        /// Target change request identifier.
        change_request_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Resolve (approve or reject) an impact assessment.
    ResolveImpactAssessment {
        /// Target assessment identifier.
        assessment_id: String,
        /// `true` if approved, `false` if rejected.
        approved: bool,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Apply an approved requirement change.
    ApplyApprovedRequirementChange {
        /// Resolved impact assessment identifier.
        assessment_id: String,
        /// Target requirement identifier to update.
        requirement_id: String,
        /// Updated requirement content.
        content: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a new draft execution plan.
    CreatePlanDraft {
        /// Target product identifier.
        product_id: String,
        /// Plan title.
        title: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Approve an execution plan.
    ApprovePlan {
        /// Target plan identifier.
        plan_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Materialize a planned execution cycle from an approved plan.
    MaterializePlannedCycle {
        /// Target plan identifier.
        plan_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Pause an active execution cycle.
    PauseCycle {
        /// Target cycle identifier.
        cycle_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Resume a paused execution cycle.
    ResumeCycle {
        /// Target cycle identifier.
        cycle_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Cancel an execution cycle.
    CancelCycle {
        /// Target cycle identifier.
        cycle_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Materialize a task graph for a cycle stage.
    MaterializeStageTaskGraph {
        /// Target cycle identifier.
        cycle_id: String,
        /// Stage identifier name.
        stage: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Activate an approved stage in a cycle.
    ActivateApprovedStage {
        /// Target cycle identifier.
        cycle_id: String,
        /// Stage identifier name.
        stage: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Record execution evidence artifact for a stage.
    RecordStageEvidence {
        /// Target cycle identifier.
        cycle_id: String,
        /// Stage identifier name.
        stage: String,
        /// Content reference string (e.g. SHA-256 hash or file path).
        content_ref: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Link recorded stage evidence to a requirement.
    LinkStageEvidenceToRequirement {
        /// Target evidence identifier.
        evidence_id: String,
        /// Target requirement identifier.
        requirement_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Mark a cycle stage as completed.
    CompleteStage {
        /// Target cycle identifier.
        cycle_id: String,
        /// Stage identifier name.
        stage: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Inspect release readiness requirements for a product.
    InspectReleaseReadiness {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a quality verification profile.
    CreateQualityProfile {
        /// Target product identifier.
        product_id: String,
        /// Display name for the quality profile.
        name: String,
        /// Whether passing this quality profile is mandatory for release.
        required: bool,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Ensure closed-testing quality profile defaults exist for React Native projects.
    EnsureReactNativeClosedTestingQualityProfile {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Record the result of a quality verification check.
    RecordQualityCheck {
        /// Target profile identifier.
        profile_id: String,
        /// `true` if check passed, `false` otherwise.
        passed: bool,
        /// Reference to supporting test/verification evidence.
        evidence_ref: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a release draft build.
    CreateReleaseDraft {
        /// Target product identifier.
        product_id: String,
        /// Reference to build artifact or SHA.
        build_ref: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Approve a release for closed-testing deployment.
    ApproveClosedTestingRelease {
        /// Target release identifier.
        release_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Create a support ticket/item.
    CreateSupportItem {
        /// Target product identifier.
        product_id: String,
        /// Source kind (e.g., `"user_feedback"`, `"bug"`).
        source_kind: String,
        /// Short summary of the item.
        summary: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Inspect overall support item posture for a product.
    InspectSupportOverview {
        /// Target product identifier.
        product_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Triage an open support item.
    TriageSupportItem {
        /// Target support item identifier.
        support_item_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Resolve an open support item.
    ResolveSupportItem {
        /// Target support item identifier.
        support_item_id: String,
        /// Reference to resolution commit, PR, or artifact.
        resolution_ref: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Link a support item to a change request.
    LinkSupportToChangeRequest {
        /// Target support item identifier.
        support_item_id: String,
        /// Target change request identifier.
        change_request_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
    /// Request an impact assessment for a change request.
    RequestImpactAssessment {
        /// Target change request identifier.
        change_request_id: String,
        /// Unique key to prevent duplicate execution.
        idempotency_key: String,
    },
}

impl FactoryCommand {
    /// Returns the idempotency key string carried by this factory command variant.
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
            | Self::AttachExistingProject {
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

/// Product Factory domain events emitted by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "factory_event_type", content = "payload")]
pub enum FactoryEvent {
    /// A new workspace was created.
    WorkspaceCreated {
        /// Identifier of the created workspace.
        workspace_id: String,
    },
    /// A new product draft was created.
    ProductDraftCreated {
        /// Identifier of the created product draft.
        product_id: String,
    },
    /// A product intake session was started.
    IntakeStarted {
        /// Identifier of the active intake session.
        session_id: String,
    },
    /// An answer was recorded for an intake session.
    IntakeAnswerRecorded {
        /// Session identifier.
        session_id: String,
        /// Question key answered.
        question_key: String,
    },
    /// A product charter draft was created.
    CharterDraftCreated {
        /// Identifier of the charter revision.
        charter_revision_id: String,
    },
    /// A product charter draft was auto-generated.
    CharterDraftGenerated {
        /// Identifier of the generated charter revision.
        charter_revision_id: String,
    },
    /// A requirement draft was created.
    RequirementDraftCreated {
        /// Identifier of the created requirement.
        requirement_id: String,
    },
    /// A change request was submitted.
    ChangeRequestSubmitted {
        /// Identifier of the submitted change request.
        change_request_id: String,
    },
    /// An impact assessment completed and is ready for review.
    ImpactAssessmentReady {
        /// Identifier of the change request.
        change_request_id: String,
        /// Identifier of the completed assessment.
        assessment_id: String,
    },
    /// Human approval is required to proceed.
    ApprovalRequired {
        /// Target product identifier.
        product_id: String,
        /// Category kind of approval required.
        approval_kind: String,
    },
}

/// Overview projection snapshot of all Product Factory metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FactoryOverviewSnapshot {
    /// Whether the Product Factory feature is enabled in configuration.
    #[serde(default)]
    pub enabled: bool,
    /// Total number of tracked products.
    #[serde(default)]
    pub product_count: u32,
    /// Total active execution cycles.
    #[serde(default)]
    pub active_cycle_count: u32,
    /// Total pending change requests awaiting assessment or approval.
    #[serde(default)]
    pub pending_change_request_count: u32,
    /// Total open support items.
    #[serde(default)]
    pub open_support_items: u32,
    /// Number of quality profiles currently blocking release.
    #[serde(default)]
    pub blocking_quality_profiles: u32,
    /// Number of draft releases awaiting approval.
    #[serde(default)]
    pub release_drafts: u32,
    /// Number of completed verification stages.
    #[serde(default)]
    pub completed_verify_stages: u32,
    /// Number of releases approved for closed testing.
    #[serde(default)]
    pub approved_closed_testing_releases: u32,
    /// Summary of the most recently updated product, if any.
    #[serde(default)]
    pub latest_product: Option<FactoryProductSummaryDto>,
}

/// Summary metadata DTO for a Product Factory product.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FactoryProductSummaryDto {
    /// Unique product identifier.
    pub id: String,
    /// Product title.
    pub title: String,
    /// Product lifecycle status string.
    pub status: String,
    /// Operating posture mode string (`"quick"` or `"governed"`).
    #[serde(default)]
    pub mode: String,
    /// Optional absolute local path to the bound project directory.
    #[serde(default)]
    pub project_path: Option<String>,
    /// Optional remote Git repository URL.
    #[serde(default)]
    pub source_remote: Option<String>,
    /// Optional verified HEAD commit SHA.
    #[serde(default)]
    pub source_revision: Option<String>,
    /// Optional detected technology stack name (e.g. `"react_native"`).
    #[serde(default)]
    pub stack: Option<String>,
    /// Repository scaffolding or attachment state.
    #[serde(default)]
    pub scaffold_state: String,
    /// Number of open quality issues blocking progress.
    #[serde(default)]
    pub quality_blockers: u32,
    /// Number of issues blocking release.
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
