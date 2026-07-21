use serde::{Deserialize, Serialize};

/// Storage classes are explicit because durable control state must never be
/// mixed with rebuildable search indexes or large artifacts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageClass {
    DurableControl,
    RebuildableSearchCache,
    ContentAddressedArtifact,
    RecoverySnapshot,
}

/// A project runtime family detected from durable project files. The Factory
/// uses this to select a bounded adapter before falling back to generic tools.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRuntimeKind {
    ReactNative,
    Flutter,
    Web,
    Rust,
}

/// The React Native project shape changes which local and hosted build
/// providers can safely be proposed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReactNativeWorkflow {
    ExpoManaged,
    ExpoPrebuild,
    Bare,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReactNativeCapabilities {
    pub workflow: ReactNativeWorkflow,
    pub has_android_project: bool,
    pub has_ios_project: bool,
    pub package_manager: String,
    pub typescript: bool,
    pub eas_configured: bool,
    pub local_android_toolchain: bool,
    pub local_macos_capability: bool,
    pub signing_readiness: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlutterCapabilities {
    pub has_pubspec: bool,
    pub flutter_toolchain: bool,
    pub dart_toolchain: bool,
    pub has_android_project: bool,
    pub has_ios_project: bool,
    pub has_web_project: bool,
    pub signing_readiness: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebCapabilities {
    pub framework: String,
    pub package_manager: String,
    pub typescript: bool,
    pub has_build_script: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustCapabilities {
    pub has_cargo_toml: bool,
    pub cargo_toolchain: bool,
    pub edition: String,
    pub is_workspace: bool,
    pub has_clippy: bool,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectInspection {
    pub project_ref: String,
    pub runtime_kind: Option<ProjectRuntimeKind>,
    pub react_native: Option<ReactNativeCapabilities>,
    pub flutter: Option<FlutterCapabilities>,
    pub web: Option<WebCapabilities>,
    pub rust: Option<RustCapabilities>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductStatus {
    Draft,
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactoryMode {
    Quick,
    Governed,
}

impl FactoryMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Governed => "governed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactoryStage {
    Discover,
    Define,
    Design,
    Build,
    Verify,
    Release,
    Support,
    Maintain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntakeStatus {
    Open,
    ReadyForCharter,
    Closed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Proposed,
    Approved,
    Implemented,
    Verified,
    Superseded,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Superseded,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CycleStatus {
    Planned,
    Active,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageRunStatus {
    Pending,
    InProgress,
    AwaitingApproval,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeRequestStatus {
    Proposed,
    Assessing,
    AwaitingApproval,
    Accepted,
    Rejected,
    Applied,
    Withdrawn,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactStatus {
    Pending,
    Ready,
    Stale,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityCheckStatus {
    Pending,
    Passed,
    Failed,
    Waived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    Draft,
    Candidate,
    AwaitingApproval,
    Released,
    RolledBack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupportItemStatus {
    New,
    Triaged,
    InProgress,
    Resolved,
    Closed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceMode {
    Observe,
    Prepare,
    GuardedExecute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryWorkspace {
    pub id: String,
    pub name: String,
    pub owner_subject: String,
    pub status: ProductStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryPlan {
    pub id: String,
    pub workspace_id: String,
    pub product_id: String,
    pub title: String,
    pub status: CycleStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryProduct {
    pub id: String,
    pub workspace_id: String,
    pub owner_subject: String,
    pub title: String,
    pub status: ProductStatus,
    pub current_charter_revision_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CharterRevision {
    pub id: String,
    pub product_id: String,
    pub revision: u32,
    pub status: DecisionStatus,
    pub content_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequirementRecord {
    pub id: String,
    pub product_id: String,
    pub stable_key: String,
    pub status: RequirementStatus,
    pub current_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CycleRecord {
    pub id: String,
    pub product_id: String,
    pub plan_id: String,
    pub status: CycleStatus,
    pub current_stage: FactoryStage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeRequestRecord {
    pub id: String,
    pub product_id: String,
    pub requested_by: String,
    pub status: ChangeRequestStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactAssessmentSummary {
    pub id: String,
    pub change_request_id: String,
    pub status: ImpactStatus,
    pub affected_count: u32,
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityCheckRecord {
    pub id: String,
    pub product_id: String,
    pub status: QualityCheckStatus,
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseRecord {
    pub id: String,
    pub product_id: String,
    pub status: ReleaseStatus,
    pub build_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupportItemRecord {
    pub id: String,
    pub product_id: String,
    pub status: SupportItemStatus,
    pub source_kind: String,
}
