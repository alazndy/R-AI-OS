//! Control-plane domain snapshot definitions.

use serde::{Deserialize, Serialize};

use crate::dto::{
    ActiveRunDto, ArtifactDto, AuditSummaryDto, BlockedTaskDto, LogEntryDto, PolicySummaryDto,
    ProjectDto, ProjectHealthDto, ScheduledJobDto, ScoredApprovalDto, SearchResultDto,
    SystemAlertDto, ToolTraceDto, UnifiedTaskDto,
};
use crate::factory::FactoryOverviewSnapshot;

/// Snapshot projection for the Now route (immediate attention items).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NowSnapshot {
    /// Pending approval requests requiring review.
    pub approvals: Vec<ScoredApprovalDto>,
    /// Currently running agent execution sessions.
    pub active_runs: Vec<ActiveRunDto>,
    /// Tasks blocked on dependencies or manual intervention.
    pub blocked_tasks: Vec<BlockedTaskDto>,
    /// Active system alerts.
    pub alerts: Vec<SystemAlertDto>,
}

/// Snapshot projection for the Work route (projects and active tasks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkSnapshot {
    /// List of tracked workspace projects.
    pub projects: Vec<ProjectDto>,
    /// Unified task list entries.
    pub tasks: Vec<UnifiedTaskDto>,
    /// Active agent execution runs.
    pub active_runs: Vec<ActiveRunDto>,
    /// Recently generated artifacts.
    pub recent_artifacts: Vec<ArtifactDto>,
    /// Additive Product Factory projection. Older daemons omit this field.
    #[serde(default)]
    pub factory: FactoryOverviewSnapshot,
}

/// Snapshot projection for the Explore route (search and activity feeds).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ExploreSnapshot {
    /// Active search query string, if any.
    pub active_search_query: Option<String>,
    /// Search result hits matching the query.
    pub search_results: Vec<SearchResultDto>,
    /// Recent tool execution traces.
    pub recent_traces: Vec<ToolTraceDto>,
    /// Recent log records.
    pub recent_logs: Vec<LogEntryDto>,
}

/// Snapshot projection for the Govern route (policy, audit, and health).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GovernSnapshot {
    /// Security policy enforcement summary.
    pub policy_summary: PolicySummaryDto,
    /// Audit log counters.
    pub audit_summary: AuditSummaryDto,
    /// Project health check reports.
    pub health_reports: Vec<ProjectHealthDto>,
    /// Scheduled background cron jobs.
    pub cron_jobs: Vec<ScheduledJobDto>,
}

/// Envelope carrying a complete versioned snapshot across all four routes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotEnvelope {
    /// Monotonically increasing snapshot sequence number.
    pub sequence: u64,
    /// ISO-8601 UTC timestamp when the snapshot was constructed.
    pub timestamp: String,
    /// Projection for the Now route.
    pub now: NowSnapshot,
    /// Projection for the Work route.
    pub work: WorkSnapshot,
    /// Projection for the Explore route.
    pub explore: ExploreSnapshot,
    /// Projection for the Govern route.
    pub govern: GovernSnapshot,
}
