//! Read-only snapshot loaders (now / work / explore / govern) for the control plane.

use super::*;

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn load_now_snapshot(conn: &Connection) -> Result<NowSnapshot, String> {
    let raw_approvals = raios_core::db::cp_query_pending_approvals_scored(conn)
        .map_err(|e| format!("Failed loading pending approvals: {}", e))?;
    let approvals: Vec<ScoredApprovalDto> = raw_approvals
        .into_iter()
        .map(|a| ScoredApprovalDto {
            id: a.approval.id,
            task_id: a.approval.task_id.unwrap_or_default(),
            kind: a.approval.approval_type,
            title: a.approval.task_title.unwrap_or_default(),
            origin_agent: a.approval.origin_agent.unwrap_or_else(|| "agent".into()),
            target_agent: a.approval.target_agent.unwrap_or_else(|| "human".into()),
            project_path: a.approval.project_path,
            created_at: a.approval.requested_at,
            score: a.risk_score as i32,
            reason: a.approval.reason,
            revision: a.approval.revision,
        })
        .collect();

    let raw_runs = raios_core::db::cp_query_active_runs(conn)
        .map_err(|e| format!("Failed loading active runs: {}", e))?;
    let active_runs: Vec<ActiveRunDto> = raw_runs
        .into_iter()
        .map(|r| ActiveRunDto {
            run_id: r.id,
            task_id: r.task_id,
            agent_name: r.agent_name,
            project_name: r.task_title.unwrap_or_else(|| "Global".into()),
            status: r.status,
            duration_secs: 0,
        })
        .collect();

    let raw_blocked = raios_core::db::cp_query_blocked_tasks(conn)
        .map_err(|e| format!("Failed loading blocked tasks: {}", e))?;
    let blocked_tasks: Vec<BlockedTaskDto> = raw_blocked
        .into_iter()
        .map(|t| BlockedTaskDto {
            task_id: t.id,
            title: t.title,
            project_name: t.project_name.unwrap_or_else(|| "Global".into()),
            reason: t.origin,
            created_at: t.created_at,
        })
        .collect();

    let alerts = Vec::new(); // Populated dynamically by health worker/alert monitor

    Ok(NowSnapshot {
        approvals,
        active_runs,
        blocked_tasks,
        alerts,
    })
}

pub fn load_work_snapshot(conn: &Connection) -> Result<WorkSnapshot, String> {
    let dev_ops = raios_core::config::Config::load()
        .map(|c| c.dev_ops_path)
        .unwrap_or_default();
    let entity_projects = raios_core::entities::load_entities(&dev_ops);
    let projects: Vec<ProjectDto> = entity_projects
        .into_iter()
        .map(|p| {
            let memory_path = p.local_path.join("memory.md");
            let memory_preview = std::fs::read_to_string(&memory_path)
                .ok()
                .and_then(|contents| compact_memory_preview(&contents));

            ProjectDto {
                path: p.local_path.to_string_lossy().to_string(),
                name: p.name,
                status: p.status,
                git_branch: None,
                dirty_files: 0,
                last_active: p.last_commit,
                has_memory: memory_path.is_file(),
                memory_preview,
            }
        })
        .collect();

    let raw_tasks = raios_core::db::cp_query_active_tasks(conn)
        .map_err(|e| format!("Failed loading tasks: {}", e))?;
    let tasks: Vec<UnifiedTaskDto> = raw_tasks
        .into_iter()
        .map(|t| UnifiedTaskDto {
            id: t.id,
            title: t.title,
            project_path: t.project_path,
            project_name: t.registered_name.or(t.project_name),
            assignee: t.assignee_id,
            status: t.status,
            priority: t.priority.clamp(0, i64::from(u8::MAX)) as u8,
            created_at: t.created_at,
            updated_at: t.updated_at,
        })
        .collect();

    let now_snap = load_now_snapshot(conn)?;
    let active_runs = now_snap.active_runs;
    let recent_artifacts = Vec::new();
    let factory_row = raios_core::db::load_factory_overview(conn)
        .map_err(|e| format!("Failed loading Product Factory overview: {e}"))?;
    let factory_enabled = raios_core::config::Config::load()
        .map(|config| config.factory.enabled)
        .unwrap_or(false);
    let factory = FactoryOverviewSnapshot {
        enabled: factory_enabled,
        product_count: factory_row.product_count,
        active_cycle_count: factory_row.active_cycle_count,
        pending_change_request_count: factory_row.pending_change_request_count,
        open_support_items: factory_row.open_support_item_count,
        blocking_quality_profiles: factory_row.blocking_quality_profile_count,
        release_drafts: factory_row.release_draft_count,
        completed_verify_stages: factory_row.completed_verify_stage_count,
        approved_closed_testing_releases: factory_row.approved_closed_testing_release_count,
        latest_product: factory_row
            .latest_product
            .map(|product| FactoryProductSummaryDto {
                id: product.id,
                title: product.title,
                status: product.status,
                mode: product.mode,
                project_path: product.project_path,
                source_remote: product.source_remote,
                source_revision: product.source_revision,
                stack: product.stack,
                scaffold_state: product.scaffold_state,
                quality_blockers: product.quality_blockers,
                release_blockers: product.release_blockers,
            }),
    };

    Ok(WorkSnapshot {
        projects,
        tasks,
        active_runs,
        recent_artifacts,
        factory,
    })
}

/// Keeps the control-plane snapshot compact while still making project memory
/// actionable in the TUI. It never writes or logs the memory contents.
pub(super) fn compact_memory_preview(contents: &str) -> Option<String> {
    const MAX_LINES: usize = 8;
    const MAX_CHARS_PER_LINE: usize = 140;

    let lines: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .map(|line| {
            let mut preview: String = line.chars().take(MAX_CHARS_PER_LINE).collect();
            if line.chars().count() > MAX_CHARS_PER_LINE {
                preview.push_str("...");
            }
            preview
        })
        .collect();

    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub fn load_explore_snapshot(
    conn: &Connection,
    search_query: Option<&str>,
) -> Result<ExploreSnapshot, String> {
    let active_search_query = search_query.map(|s| s.to_string());
    let search_results = Vec::new();

    let query = raios_core::db::ToolTraceQuery {
        text: "",
        project: None,
        preferred_project: None,
        success_only: false,
        tag: None,
        limit: 50,
    };

    let raw_traces = raios_core::db::tool_trace_search(conn, query).unwrap_or_default();

    let recent_traces: Vec<ToolTraceDto> = raw_traces
        .into_iter()
        .map(|t| ToolTraceDto {
            id: t.id,
            tool_name: t.command,
            project_path: Some(t.project),
            status: if t.success {
                "SUCCESS".into()
            } else {
                "FAILED".into()
            },
            duration_ms: 0,
            timestamp: t.created_at,
        })
        .collect();

    let raw_logs = raios_core::db::cp_logs_replay(conn, 100).unwrap_or_default();
    let recent_logs: Vec<LogEntryDto> = raw_logs
        .into_iter()
        .map(|(ts, sender, content)| LogEntryDto {
            timestamp: ts,
            category: sender,
            message: content,
        })
        .collect();

    Ok(ExploreSnapshot {
        active_search_query,
        search_results,
        recent_traces,
        recent_logs,
    })
}

pub fn load_govern_snapshot(conn: &Connection) -> Result<GovernSnapshot, String> {
    let policy_config =
        raios_core::security::PolicyConfig::try_load_default().unwrap_or_else(|| {
            raios_core::security::PolicyConfig {
                server: None,
                filesystem: raios_core::security::policy::FilesystemPolicy {
                    enforce_sandbox: true,
                    allowed_paths: vec![],
                    blocked_paths: vec![],
                },
                tools: raios_core::security::policy::ToolsPolicy {
                    default_action: raios_core::security::policy::PolicyAction::Confirm,
                    rules: vec![],
                },
                preflight: None,
                egress: None,
                rate_limits: None,
                quarantine: None,
                hooks: None,
            }
        });

    let policy_summary = PolicySummaryDto {
        enforce_sandbox: policy_config.filesystem.enforce_sandbox,
        egress_enabled: policy_config.egress.as_ref().is_some_and(|e| e.enabled),
        default_action: format!("{:?}", policy_config.tools.default_action),
        total_rules: policy_config.tools.rules.len(),
    };

    let total_records: usize = conn
        .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
        .unwrap_or(0);
    let allowed_records: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type LIKE '%allow%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let denied_records: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type LIKE '%deny%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let confirmed_records: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type LIKE '%confirm%'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let audit_summary = AuditSummaryDto {
        total_records,
        allowed_records,
        denied_records,
        confirmed_records,
    };

    let raw_jobs = raios_core::db::cp_scheduled_jobs_list(conn).unwrap_or_default();
    let cron_jobs: Vec<ScheduledJobDto> = raw_jobs
        .into_iter()
        .map(|j| ScheduledJobDto {
            id: j.id,
            name: j.title,
            schedule: format!("{}s", j.interval_secs),
            command: j.task_description,
            status: j.status,
            last_run: j.last_run_at,
            next_run: Some(j.next_run_at),
        })
        .collect();

    let health_reports = Vec::new();

    Ok(GovernSnapshot {
        policy_summary,
        audit_summary,
        health_reports,
        cron_jobs,
    })
}

pub fn load_system_snapshot(conn: &Connection) -> Result<SnapshotEnvelope, String> {
    let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let timestamp = Utc::now().to_rfc3339();

    let now = load_now_snapshot(conn)?;
    let work = load_work_snapshot(conn)?;
    let explore = load_explore_snapshot(conn, None)?;
    let govern = load_govern_snapshot(conn)?;

    Ok(SnapshotEnvelope {
        sequence,
        timestamp,
        now,
        work,
        explore,
        govern,
    })
}
