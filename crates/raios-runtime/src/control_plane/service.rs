use chrono::Utc;
use raios_contracts::{
    ActiveRunDto, AuditSummaryDto, BlockedTaskDto, Command, ExploreSnapshot,
    FactoryOverviewSnapshot, FactoryProductSummaryDto, GovernSnapshot, LogEntryDto, NowSnapshot,
    PolicySummaryDto, Problem, ProjectDto, ScheduledJobDto, ScoredApprovalDto, SnapshotEnvelope,
    ToolTraceDto, UnifiedTaskDto, WorkSnapshot,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Principal derived by an authenticated transport. It is intentionally not
/// serialized or accepted from a client payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlActor {
    subject: String,
    may_mutate_control_plane: bool,
}

impl ControlActor {
    pub fn local_session() -> Self {
        Self {
            subject: "local_control_owner".into(),
            may_mutate_control_plane: true,
        }
    }

    pub fn remote_session(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            may_mutate_control_plane: false,
        }
    }

    #[cfg(test)]
    fn test_local() -> Self {
        Self::local_session()
    }

    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }

    pub(crate) fn may_mutate_control_plane(&self) -> bool {
        self.may_mutate_control_plane
    }
}

/// Create the idempotency cache table if it doesn't already exist.
pub fn init_idempotency_table(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cp_idempotency (
            idempotency_key TEXT PRIMARY KEY,
            payload_hash TEXT NOT NULL DEFAULT '',
            command_type TEXT NOT NULL,
            result_json TEXT,
            status TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Migrate table if missing payload_hash column
    let _ = conn.execute(
        "ALTER TABLE cp_idempotency ADD COLUMN payload_hash TEXT NOT NULL DEFAULT ''",
        [],
    );

    Ok(())
}

pub fn clean_expired_idempotency(conn: &Connection, max_age_hours: u32) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM cp_idempotency WHERE created_at < datetime('now', '-' || ?1 || ' hours')",
        params![max_age_hours],
    )
    .map_err(|e| e.to_string())
}

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
            origin_agent: "agent".into(),
            target_agent: "human".into(),
            project_path: None,
            created_at: a.approval.requested_at,
            score: a.risk_score as i32,
            reason: a.approval.reason,
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
            project_path: t.project_name,
            assignee: t.assignee_id,
            status: t.status,
            priority: 1,
            created_at: t.created_at,
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
fn compact_memory_preview(contents: &str) -> Option<String> {
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

fn command_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::ApproveHandoff { .. } => "approve_handoff",
        Command::RejectHandoff { .. } => "reject_handoff",
        Command::ApproveFileChange { .. } => "approve_file_change",
        Command::RejectFileChange { .. } => "reject_file_change",
        Command::LaunchAgent { .. } => "launch_agent",
        Command::CancelAgentRun { .. } => "cancel_agent_run",
        Command::CreateTask { .. } => "create_task",
        Command::UpdateTaskStatus { .. } => "update_task_status",
        Command::TriggerCronJob { .. } => "trigger_cron_job",
        Command::ToggleCronJob { .. } => "toggle_cron_job",
        Command::ExecuteSearch { .. } => "execute_search",
        Command::UpdatePolicyRule { .. } => "update_policy_rule",
    }
}

/// Dispatches a control-plane command with payload-bound idempotency, atomic DB
/// execution, explicit unsupported-command errors, and a transactional audit entry.
///
/// `actor` must originate from a transport-authenticated session. This service
/// deliberately does not trust environment variables or client-provided actor data.
pub fn dispatch_control_command(
    conn: &mut Connection,
    actor: &ControlActor,
    cmd: &Command,
) -> Result<Option<serde_json::Value>, Problem> {
    if !actor.may_mutate_control_plane {
        return Err(Problem::unauthorized(
            "This authenticated principal is not authorized to mutate the control plane",
        ));
    }

    init_idempotency_table(conn).map_err(Problem::internal)?;

    let command_name = command_name(cmd);
    let audit_event_type = match raios_core::security::PolicyConfig::try_load_default()
        .map(|policy| policy.tool_action(command_name).clone())
    {
        Some(raios_core::security::policy::PolicyAction::Deny) => {
            let _ = raios_core::security::record_tool_decision(
                conn,
                command_name,
                &format!("{:x}", Sha256::digest(command_name.as_bytes())),
                "control_plane",
                "tool_deny",
                actor.subject(),
            );
            return Err(Problem::forbidden(format!(
                "Security policy denies control command '{command_name}'"
            )));
        }
        Some(raios_core::security::policy::PolicyAction::Allow) => "tool_allow",
        // The authenticated human gesture that submitted this command is the
        // confirmation required by the policy; it remains distinct in audit.
        Some(raios_core::security::policy::PolicyAction::Confirm) | None => "tool_confirm",
    };

    let key = cmd.idempotency_key();
    let payload_json = serde_json::to_string(cmd).map_err(|e| {
        Problem::invalid_input(format!("Failed serializing command payload: {}", e))
    })?;
    let payload_hash = format!("{:x}", Sha256::digest(payload_json.as_bytes()));

    let existing: Result<Option<(String, Option<String>)>, _> = conn
        .query_row(
            "SELECT payload_hash, result_json FROM cp_idempotency WHERE idempotency_key = ?1",
            params![key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional();

    if let Ok(Some((stored_hash, cached_json_opt))) = existing {
        if !stored_hash.is_empty() && stored_hash != payload_hash {
            return Err(Problem::invalid_input(format!(
                "Idempotency collision: key '{}' used with different command payload",
                key
            )));
        }
        let val: Option<serde_json::Value> =
            cached_json_opt.and_then(|j| serde_json::from_str(&j).ok());
        return Ok(val);
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| Problem::internal(format!("Failed starting transaction: {}", e)))?;

    let result_val: Option<serde_json::Value> = match cmd {
        Command::ApproveHandoff { approval_id, .. } => {
            let handoff = tx
                .query_row(
                    "SELECT task_id, agent_run_id, artifact_id
                     FROM cp_approvals
                     WHERE id = ?1 AND approval_type = 'handover' AND status = 'pending'
                       AND owner_subject = ?2",
                    params![approval_id, actor.subject()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| Problem::internal(e.to_string()))?
                .ok_or_else(|| Problem::not_found("Pending handoff approval not found"))?;
            let now = Utc::now().to_rfc3339();
            tx.execute(
                "UPDATE cp_approvals
                 SET status = 'approved', resolved_at = ?1, resolved_by = ?2
                 WHERE id = ?3 AND status = 'pending' AND owner_subject = ?4",
                params![now, actor.subject(), approval_id, actor.subject()],
            )
            .map_err(|e| Problem::internal(e.to_string()))?;
            tx.execute(
                "UPDATE cp_artifacts SET status = 'applied' WHERE id = ?1",
                params![handoff.2],
            )
            .map_err(|e| Problem::internal(e.to_string()))?;
            tx.execute(
                "UPDATE cp_agent_runs
                 SET status = 'succeeded', ended_at = ?1, exit_reason = 'handover_delivered'
                 WHERE id = ?2",
                params![now, handoff.1],
            )
            .map_err(|e| Problem::internal(e.to_string()))?;
            tx.execute(
                "UPDATE cp_tasks SET status = 'completed', updated_at = ?1 WHERE id = ?2",
                params![now, handoff.0],
            )
            .map_err(|e| Problem::internal(e.to_string()))?;
            Some(serde_json::json!({"approval_id": approval_id, "status": "approved"}))
        }
        Command::RejectHandoff {
            approval_id,
            reason,
            ..
        } => {
            if reason.trim().is_empty() || reason.len() > 2_000 {
                return Err(Problem::invalid_input(
                    "Rejection reason must contain at most 2000 characters",
                ));
            }
            let now = Utc::now().to_rfc3339();
            let changed = tx
                .execute(
                    "UPDATE cp_approvals
                 SET status = 'rejected', resolved_at = ?1, resolved_by = ?2,
                     reason = reason || char(10) || 'Rejected: ' || ?3
                 WHERE id = ?4 AND approval_type = 'handover' AND status = 'pending'
                   AND owner_subject = ?5",
                    params![now, actor.subject(), reason, approval_id, actor.subject()],
                )
                .map_err(|e| Problem::internal(e.to_string()))?;
            if changed != 1 {
                return Err(Problem::not_found("Pending handoff approval not found"));
            }
            Some(serde_json::json!({"approval_id": approval_id, "status": "rejected"}))
        }
        Command::TriggerCronJob { job_id, .. } => {
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM cp_scheduled_jobs WHERE id = ?1 AND status != 'deleted'",
                    params![job_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| Problem::internal(e.to_string()))?;
            if exists.is_none() {
                return Err(Problem::not_found("Scheduled job not found"));
            }
            raios_core::db::cp_scheduled_job_trigger_now(&tx, job_id)
                .map_err(|e| Problem::internal(e.to_string()))?;
            Some(serde_json::json!({"job_id": job_id, "status": "scheduled_now"}))
        }
        Command::ToggleCronJob { job_id, paused, .. } => {
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM cp_scheduled_jobs WHERE id = ?1 AND status != 'deleted'",
                    params![job_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| Problem::internal(e.to_string()))?;
            if exists.is_none() {
                return Err(Problem::not_found("Scheduled job not found"));
            }
            let status_str = if *paused { "paused" } else { "active" };
            raios_core::db::cp_scheduled_job_set_status(&tx, job_id, status_str)
                .map_err(|e| Problem::internal(e.to_string()))?;
            Some(serde_json::json!({"job_id": job_id, "status": status_str}))
        }
        unhandled => {
            return Err(Problem::not_implemented(format!(
                "Command '{:?}' is not implemented",
                unhandled
            )));
        }
    };

    let cached_str = result_val.as_ref().map(|v| v.to_string());
    tx.execute(
        "INSERT INTO cp_idempotency (idempotency_key, payload_hash, command_type, result_json, status) VALUES (?1, ?2, ?3, ?4, 'COMPLETED')",
        params![key, payload_hash, command_name, cached_str],
    )
    .map_err(|e| Problem::internal(format!("Idempotency record failed: {}", e)))?;

    raios_core::security::record_tool_decision(
        &tx,
        command_name,
        &payload_hash,
        "control_plane",
        audit_event_type,
        actor.subject(),
    )
    .map_err(|e| Problem::internal(format!("Audit write failed: {}", e)))?;

    tx.commit()
        .map_err(|e| Problem::internal(format!("Commit failed: {}", e)))?;

    Ok(result_val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_generation_on_in_memory_db() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let snap = load_system_snapshot(&conn).unwrap();
        assert!(snap.sequence >= 1);
        assert!(!snap.timestamp.is_empty());
        assert_eq!(snap.now.active_runs.len(), 0);
        assert_eq!(snap.work.factory.product_count, 0);
        assert!(!snap.work.factory.enabled);
    }

    #[test]
    fn work_snapshot_projects_read_only_factory_overview() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn.execute(
            "INSERT INTO cp_workspaces (id, name, owner_subject) VALUES ('workspace-1', 'Workspace', 'local_control_owner')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_factory_products (id, workspace_id, owner_subject, title, status) VALUES ('product-1', 'workspace-1', 'local_control_owner', 'Pilot', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_plans (id, workspace_id, product_id, title, status) VALUES ('plan-1', 'workspace-1', 'product-1', 'Pilot plan', 'planned')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_factory_cycles (id, product_id, plan_id, status, current_stage) VALUES ('cycle-1', 'product-1', 'plan-1', 'active', 'discover')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_factory_change_requests (id, product_id, requested_by, status, summary) VALUES ('change-1', 'product-1', 'local_control_owner', 'awaiting_approval', 'Change')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_factory_support_items (id, product_id, source_kind, status) VALUES ('support-1', 'product-1', 'manual', 'triaged')",
            [],
        )
        .unwrap();

        let snapshot = load_work_snapshot(&conn).unwrap();
        assert_eq!(snapshot.factory.product_count, 1);
        assert_eq!(snapshot.factory.active_cycle_count, 1);
        assert_eq!(snapshot.factory.pending_change_request_count, 1);
        assert_eq!(snapshot.factory.open_support_items, 1);
        assert_eq!(
            snapshot
                .factory
                .latest_product
                .as_ref()
                .map(|product| product.title.as_str()),
            Some("Pilot")
        );
    }

    #[test]
    fn memory_preview_is_bounded_and_skips_blank_lines() {
        let contents = "\n# Project Memory\n\nStatus: Active\n\nObjective: Improve the TUI\n";

        assert_eq!(
            compact_memory_preview(contents).as_deref(),
            Some("# Project Memory\nStatus: Active\nObjective: Improve the TUI")
        );
        assert_eq!(compact_memory_preview("\n \n"), None);
    }

    #[test]
    fn command_dispatch_idempotency_and_payload_check() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let workflow = raios_core::db::create_handoff_workflow(
            &conn,
            raios_core::db::HandoffWorkflowInput {
                project_path: "/tmp/control-plane-test",
                from_agent: "codex_kaira",
                to_agent: "antigravity_kaira",
                status: "blocker",
                msg: "Needs explicit rejection",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();

        let cmd1 = Command::RejectHandoff {
            approval_id: workflow.approval_id,
            reason: "Not needed".into(),
            idempotency_key: "idem-key-1".into(),
        };

        let actor = ControlActor::test_local();
        let res1 = dispatch_control_command(&mut conn, &actor, &cmd1);
        assert!(res1.is_ok());

        // Same key + same payload -> OK cached
        let res2 = dispatch_control_command(&mut conn, &actor, &cmd1);
        assert!(res2.is_ok());

        // Same key + different payload -> Error (invalid input / collision)
        let cmd2 = Command::RejectHandoff {
            approval_id: "app-different".into(),
            reason: "Different reason".into(),
            idempotency_key: "idem-key-1".into(),
        };
        let res3 = dispatch_control_command(&mut conn, &actor, &cmd2);
        assert!(res3.is_err());
        assert_eq!(res3.unwrap_err().code, "INVALID_INPUT");
    }

    #[test]
    fn handoff_command_updates_snapshot_and_audit_atomically() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let workflow = raios_core::db::create_handoff_workflow(
            &conn,
            raios_core::db::HandoffWorkflowInput {
                project_path: "/tmp/control-plane-flow",
                from_agent: "codex_kaira",
                to_agent: "antigravity_kaira",
                status: "blocker",
                msg: "Ready for human approval",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();

        let before = load_system_snapshot(&conn).unwrap();
        assert!(before
            .now
            .approvals
            .iter()
            .any(|approval| approval.id == workflow.approval_id));

        let command = Command::ApproveHandoff {
            approval_id: workflow.approval_id.clone(),
            idempotency_key: "approve-control-plane-flow".into(),
        };
        let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &command)
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "approved");

        let after = load_system_snapshot(&conn).unwrap();
        assert!(!after
            .now
            .approvals
            .iter()
            .any(|approval| approval.id == workflow.approval_id));
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_tasks WHERE id = ?1",
                params![workflow.task_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "completed"
        );
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
    }

    #[test]
    fn remote_principal_cannot_resolve_local_owned_approval() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let workflow = raios_core::db::create_handoff_workflow(
            &conn,
            raios_core::db::HandoffWorkflowInput {
                project_path: "/tmp/control-plane-owner",
                from_agent: "codex_kaira",
                to_agent: "antigravity_kaira",
                status: "blocker",
                msg: "Owner-bound approval",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();

        let command = Command::ApproveHandoff {
            approval_id: workflow.approval_id.clone(),
            idempotency_key: "remote-owner-check".into(),
        };
        let error = dispatch_control_command(
            &mut conn,
            &ControlActor::remote_session("remote_api_key"),
            &command,
        )
        .unwrap_err();
        assert_eq!(error.code, "UNAUTHORIZED");
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_approvals WHERE id = ?1",
                params![workflow.approval_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "pending"
        );
    }

    #[test]
    fn command_unsupported_returns_not_implemented() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let unsupported = Command::LaunchAgent {
            agent_name: "codex".into(),
            prompt: Some("Do something".into()),
            project_path: "/tmp".into(),
            idempotency_key: "idem-unsupported".into(),
        };

        let res = dispatch_control_command(&mut conn, &ControlActor::test_local(), &unsupported);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, "NOT_IMPLEMENTED");
    }
}
