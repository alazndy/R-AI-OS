use serde::{Deserialize, Serialize};

use crate::dto::{
    ActiveRunDto, ArtifactDto, AuditSummaryDto, BlockedTaskDto, LogEntryDto, PolicySummaryDto,
    ProjectDto, ProjectHealthDto, ScheduledJobDto, ScoredApprovalDto, SearchResultDto,
    SystemAlertDto, ToolTraceDto, UnifiedTaskDto,
};
use crate::factory::FactoryOverviewSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NowSnapshot {
    pub approvals: Vec<ScoredApprovalDto>,
    pub active_runs: Vec<ActiveRunDto>,
    pub blocked_tasks: Vec<BlockedTaskDto>,
    pub alerts: Vec<SystemAlertDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkSnapshot {
    pub projects: Vec<ProjectDto>,
    pub tasks: Vec<UnifiedTaskDto>,
    pub active_runs: Vec<ActiveRunDto>,
    pub recent_artifacts: Vec<ArtifactDto>,
    /// Additive Product Factory projection. Older daemons omit this field.
    #[serde(default)]
    pub factory: FactoryOverviewSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ExploreSnapshot {
    pub active_search_query: Option<String>,
    pub search_results: Vec<SearchResultDto>,
    pub recent_traces: Vec<ToolTraceDto>,
    pub recent_logs: Vec<LogEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GovernSnapshot {
    pub policy_summary: PolicySummaryDto,
    pub audit_summary: AuditSummaryDto,
    pub health_reports: Vec<ProjectHealthDto>,
    pub cron_jobs: Vec<ScheduledJobDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotEnvelope {
    pub sequence: u64,
    pub timestamp: String,
    pub now: NowSnapshot,
    pub work: WorkSnapshot,
    pub explore: ExploreSnapshot,
    pub govern: GovernSnapshot,
}
