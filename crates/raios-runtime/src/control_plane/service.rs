use chrono::Utc;
use raios_contracts::{
    ActiveRunDto, AuditSummaryDto, BlockedTaskDto, Command, Event, ExploreSnapshot,
    FactoryOverviewSnapshot, FactoryProductSummaryDto, GovernSnapshot, LogEntryDto, NowSnapshot,
    PolicySummaryDto, Problem, ProjectDto, ScheduledJobDto, ScoredApprovalDto, SnapshotEnvelope,
    ToolTraceDto, UnifiedTaskDto, WorkSnapshot,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const MAX_TASK_TITLE_CHARS: usize = 240;
const MAX_PROJECT_PATH_CHARS: usize = 4_096;
const MAX_AGENT_PROMPT_CHARS: usize = 600;
const OPERATOR_TASK_STATUSES: &[&str] =
    &["queued", "in_progress", "blocked", "completed", "cancelled"];

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
            project_path: t.project_path,
            project_name: t.registered_name.or(t.project_name),
            assignee: t.assignee_id,
            status: t.status,
            priority: t.priority.clamp(0, i64::from(u8::MAX)) as u8,
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
        Command::ValidateAgentLaunch { .. } => "validate_agent_launch",
        Command::CancelAgentRun { .. } => "cancel_agent_run",
        Command::CreateTask { .. } => "create_task",
        Command::UpdateTaskStatus { .. } => "update_task_status",
        Command::TriggerCronJob { .. } => "trigger_cron_job",
        Command::ToggleCronJob { .. } => "toggle_cron_job",
        Command::ExecuteSearch { .. } => "execute_search",
        Command::UpdatePolicyRule { .. } => "update_policy_rule",
        Command::RegisterProject { .. } => "register_project",
        Command::CreateHandoff { .. } => "create_handoff",
    }
}

/// Name under which the security policy is consulted. Validating a launch is governed by the
/// same `launch_agent` rule as launching, so a policy that denies launches also denies it.
fn policy_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::ValidateAgentLaunch { .. } => "launch_agent",
        other => command_name(other),
    }
}

fn validate_task_title(title: &str) -> Result<String, Problem> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TASK_TITLE_CHARS {
        return Err(Problem::invalid_input(format!(
            "Task title must contain 1 to {MAX_TASK_TITLE_CHARS} characters"
        )));
    }
    if raios_core::security::looks_like_secret(title).is_some() {
        return Err(Problem::invalid_input(
            "Task title appears to contain a secret and was not stored",
        ));
    }
    Ok(title.to_owned())
}

pub(super) fn validate_project_path(project_path: Option<&str>) -> Result<Option<String>, Problem> {
    let Some(project_path) = project_path else {
        return Ok(None);
    };
    let project_path = project_path.trim();
    if project_path.is_empty() || project_path.chars().count() > MAX_PROJECT_PATH_CHARS {
        return Err(Problem::invalid_input(format!(
            "Project path must contain 1 to {MAX_PROJECT_PATH_CHARS} characters"
        )));
    }
    if !std::path::Path::new(project_path).is_absolute() {
        return Err(Problem::invalid_input("Project path must be absolute"));
    }
    Ok(Some(project_path.to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentLaunchRequest {
    pub(super) agent_name: String,
    pub(super) project_path: String,
    pub(super) prompt: Option<String>,
}

pub(super) fn validate_agent_name(agent_name: &str) -> Result<String, Problem> {
    match agent_name.trim().to_ascii_lowercase().as_str() {
        "claude" => Ok("claude".into()),
        "codex" => Ok("codex".into()),
        "opencode" => Ok("opencode".into()),
        "antigravity" | "agy" => Ok("antigravity".into()),
        _ => Err(Problem::invalid_input(
            "Agent must be one of: claude, codex, opencode, antigravity",
        )),
    }
}

pub(super) fn validate_agent_prompt(prompt: Option<&str>) -> Result<Option<String>, Problem> {
    let Some(prompt) = prompt else {
        return Ok(None);
    };
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Ok(None);
    }
    if prompt.starts_with('-') || prompt.chars().count() > MAX_AGENT_PROMPT_CHARS {
        return Err(Problem::invalid_input(format!(
            "Agent prompt must contain 1 to {MAX_AGENT_PROMPT_CHARS} characters and cannot begin with '- '"
        )));
    }
    if raios_core::security::looks_like_secret(prompt).is_some() {
        return Err(Problem::invalid_input(
            "Agent prompt appears to contain a secret and was not sent",
        ));
    }
    Ok(Some(prompt.to_owned()))
}

/// Opens the trusted wrapper in a separate terminal, preserving the agent's
/// interactive TTY and all existing `raios run` session tracking.
fn launch_agent_in_terminal(request: &AgentLaunchRequest) -> Result<(), Problem> {
    let mut args = vec![
        "run".into(),
        request.agent_name.clone(),
        "--project".into(),
        request.project_path.clone(),
    ];
    if let Some(prompt) = &request.prompt {
        args.push(prompt.clone());
    }
    if raios_core::core::process::launch_in_terminal_argv(
        "raios",
        &args,
        Path::new(&request.project_path),
    ) {
        Ok(())
    } else {
        Err(Problem::internal(
            "No supported terminal launcher was available for the requested agent session",
        ))
    }
}

/// Reads the run linked to a handoff approval, if any, while the approval row
/// is still observable inside the dispatch transaction.
fn handoff_run_id(tx: &Connection, approval_id: &str) -> Result<Option<String>, Problem> {
    tx.query_row(
        "SELECT agent_run_id FROM cp_approvals WHERE id = ?1",
        params![approval_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map_err(|e| Problem::internal(e.to_string()))
    .map(Option::flatten)
}

/// Refuses a decision made on a stale read. When the caller names the revision it saw and the
/// stored revision has moved on, the answer is `CONFLICT` — never a silent decision on newer
/// state. A named revision that matches no approval at all stays `NOT_FOUND`. Callers that
/// name no revision (older surfaces) are unaffected.
fn require_expected_revision(
    conn: &Connection,
    approval_id: &str,
    expected: Option<i64>,
) -> Result<(), Problem> {
    let Some(expected) = expected else {
        return Ok(());
    };
    match raios_core::db::cp_approval_revision(conn, approval_id)
        .map_err(|e| Problem::internal(e.to_string()))?
    {
        Some(current) if current != expected => Err(Problem::conflict(format!(
            "Approval '{}' changed since it was read (revision {} is stale, current is {})",
            approval_id, expected, current
        ))),
        Some(_) => Ok(()),
        None => Err(Problem::not_found("Pending handoff approval not found")),
    }
}

/// Appends the typed events for a resolved handoff approval: the resolution
/// itself plus the linked run's *observed* post-resolution state. The status
/// string is read back from the database rather than assumed, so the event
/// always mirrors the durable transition.
fn push_handoff_resolution_events(
    tx: &Connection,
    approval_id: &str,
    run_id: Option<&str>,
    resolution: &str,
    pending: &mut Vec<Event>,
) -> Result<(), Problem> {
    pending.push(Event::ApprovalResolved {
        approval_id: approval_id.to_owned(),
        status: resolution.to_owned(),
    });
    let Some(run_id) = run_id else {
        return Ok(());
    };
    let run = tx
        .query_row(
            "SELECT agent_name, status FROM cp_agent_runs WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| Problem::internal(e.to_string()))?;
    if let Some((agent_name, status)) = run {
        pending.push(Event::AgentRunStateChanged {
            run_id: run_id.to_owned(),
            agent_name,
            status,
        });
    }
    Ok(())
}

/// Appends the typed events for a queued handoff: the new approval request
/// plus the run's observed creation state. `result` is the `CreateHandoff`
/// result payload; its ids come from the same transaction.
fn push_handoff_created_events(
    tx: &Connection,
    result: &serde_json::Value,
    target: &str,
    pending: &mut Vec<Event>,
) -> Result<(), Problem> {
    let Some(approval_id) = result["approval_id"].as_str() else {
        return Ok(());
    };
    let title = match result["task_id"].as_str() {
        Some(task_id) => tx
            .query_row(
                "SELECT title FROM cp_tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| Problem::internal(e.to_string()))?
            .unwrap_or_else(|| format!("Handoff to {target}")),
        None => format!("Handoff to {target}"),
    };
    pending.push(Event::ApprovalRequested {
        approval_id: approval_id.to_owned(),
        // Canonical `cp_approvals.approval_type` value for handoffs.
        kind: "handover".to_owned(),
        title,
        target: target.to_owned(),
    });
    if let Some(run_id) = result["agent_run_id"].as_str() {
        let run = tx
            .query_row(
                "SELECT agent_name, status FROM cp_agent_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| Problem::internal(e.to_string()))?;
        if let Some((agent_name, status)) = run {
            pending.push(Event::AgentRunStateChanged {
                run_id: run_id.to_owned(),
                agent_name,
                status,
            });
        }
    }
    Ok(())
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
    dispatch_with_seams(
        conn,
        actor,
        cmd,
        &launch_agent_in_terminal,
        &raios_core::security::PolicyConfig::try_load_default,
    )
}

/// [`dispatch_control_command`] with its two outside effects injected: the terminal launcher and
/// the security policy loader. Tests use this to prove which commands start a process and which
/// never do, and to apply a policy without touching process-wide state (the working directory or
/// `XDG_CONFIG_HOME`), which other tests share.
pub(super) fn dispatch_with_seams(
    conn: &mut Connection,
    actor: &ControlActor,
    cmd: &Command,
    launcher: &dyn Fn(&AgentLaunchRequest) -> Result<(), Problem>,
    load_policy: &dyn Fn() -> Option<raios_core::security::PolicyConfig>,
) -> Result<Option<serde_json::Value>, Problem> {
    if !actor.may_mutate_control_plane {
        return Err(Problem::unauthorized(
            "This authenticated principal is not authorized to mutate the control plane",
        ));
    }

    init_idempotency_table(conn).map_err(Problem::internal)?;

    let command_name = command_name(cmd);
    let audit_event_type =
        match load_policy().map(|policy| policy.tool_action(policy_name(cmd)).clone()) {
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

    let mut launch_request: Option<AgentLaunchRequest> = None;
    let mut pending_events: Vec<Event> = Vec::new();
    let result_val: Option<serde_json::Value> = match cmd {
        Command::ApproveHandoff {
            approval_id,
            expected_revision,
            ..
        } => {
            require_expected_revision(&tx, approval_id, *expected_revision)?;
            let run_id = handoff_run_id(&tx, approval_id)?;
            // The owner's consent: the approval resolves as approved and the run leaves the
            // queue, but the task stays open. Only the addressed agent's own wrapper marks
            // delivery (`handover_delivered`).
            if !raios_core::db::cp_approve_handoff_by_owner(
                &tx,
                approval_id,
                actor.subject(),
                actor.subject(),
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::not_found("Pending handoff approval not found"));
            }
            push_handoff_resolution_events(
                &tx,
                approval_id,
                run_id.as_deref(),
                "approved",
                &mut pending_events,
            )?;
            Some(serde_json::json!({"approval_id": approval_id, "status": "approved"}))
        }
        Command::RejectHandoff {
            approval_id,
            reason,
            expected_revision,
            ..
        } => {
            require_expected_revision(&tx, approval_id, *expected_revision)?;
            if reason.trim().is_empty() || reason.len() > 2_000 {
                return Err(Problem::invalid_input(
                    "Rejection reason must contain at most 2000 characters",
                ));
            }
            let run_id = handoff_run_id(&tx, approval_id)?;
            if !raios_core::db::cp_resolve_handoff(
                &tx,
                approval_id,
                actor.subject(),
                actor.subject(),
                Some(reason),
            )
            .map_err(|e| Problem::internal(e.to_string()))?
            {
                return Err(Problem::not_found("Pending handoff approval not found"));
            }
            push_handoff_resolution_events(
                &tx,
                approval_id,
                run_id.as_deref(),
                "rejected",
                &mut pending_events,
            )?;
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
        Command::LaunchAgent {
            agent_name,
            project_path,
            prompt,
            ..
        } => {
            let request = super::agent_launch::validate_launch(
                &tx,
                agent_name,
                project_path,
                prompt.as_deref(),
            )?;
            let result = super::agent_launch::launch_result(&request, "launching");
            launch_request = Some(request);
            Some(result)
        }
        Command::ValidateAgentLaunch {
            agent_name,
            project_path,
            prompt,
            ..
        } => {
            let request = super::agent_launch::validate_launch(
                &tx,
                agent_name,
                project_path,
                prompt.as_deref(),
            )?;
            Some(super::agent_launch::launch_result(&request, "validated"))
        }
        Command::CreateTask {
            title,
            project_path,
            priority,
            ..
        } => {
            let title = validate_task_title(title)?;
            let project_path = validate_project_path(project_path.as_deref())?;
            let project_id = project_path
                .as_deref()
                .map(|path| {
                    tx.query_row(
                        "SELECT id FROM projects WHERE path = ?1",
                        params![path],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                })
                .transpose()
                .map_err(|error| Problem::internal(error.to_string()))?
                .flatten();
            let task_id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            let display_order: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(display_order), -1) + 1 FROM cp_task_list_items",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| Problem::internal(error.to_string()))?;

            tx.execute(
                "INSERT INTO cp_tasks
                 (id, project_id, plan_id, parent_task_id, title, description, priority, status,
                  assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at)
                 VALUES (?1, ?2, NULL, NULL, ?3, '', ?4, 'queued',
                         NULL, NULL, '', ?5, ?5)",
                params![task_id, project_id, title, i64::from(*priority), now],
            )
            .map_err(|error| Problem::internal(error.to_string()))?;
            tx.execute(
                "INSERT INTO cp_task_list_items
                 (task_id, source_kind, source_path, display_order, project_name, created_at)
                 VALUES (?1, 'tui', 'control-plane', ?2, ?3, ?4)",
                params![task_id, display_order, project_path, now],
            )
            .map_err(|error| Problem::internal(error.to_string()))?;

            Some(serde_json::json!({
                "task_id": task_id,
                "status": "queued",
            }))
        }
        Command::UpdateTaskStatus {
            task_id, status, ..
        } => {
            let status = status.trim();
            if task_id.trim().is_empty() || !OPERATOR_TASK_STATUSES.contains(&status) {
                return Err(Problem::invalid_input(
                    "Task id and an allowed operator task status are required",
                ));
            }
            let now = Utc::now().to_rfc3339();
            let changed = tx
                .execute(
                    "UPDATE cp_tasks
                     SET status = ?1, updated_at = ?2
                     WHERE id = ?3
                       AND plan_id IS NULL
                       AND parent_task_id IS NULL
                       AND EXISTS (
                         SELECT 1 FROM cp_task_list_items li
                         WHERE li.task_id = cp_tasks.id
                       )
                       AND NOT EXISTS (
                         SELECT 1 FROM cp_approvals ap WHERE ap.task_id = cp_tasks.id
                       )
                       AND NOT EXISTS (
                         SELECT 1 FROM cp_agent_runs ar WHERE ar.task_id = cp_tasks.id
                       )",
                    params![status, now, task_id],
                )
                .map_err(|error| Problem::internal(error.to_string()))?;
            if changed != 1 {
                return Err(Problem::not_found(
                    "Operator-managed personal task was not found or cannot be updated",
                ));
            }
            Some(serde_json::json!({"task_id": task_id, "status": status}))
        }
        Command::RegisterProject { project_path, .. } => Some(
            super::project_registry::register_project(&tx, project_path)?,
        ),
        Command::CreateHandoff {
            project_path,
            from_agent,
            to_agent,
            summary,
            ..
        } => {
            let result =
                super::handoff::create_handoff(&tx, project_path, from_agent, to_agent, summary)?;
            push_handoff_created_events(&tx, &result, to_agent, &mut pending_events)?;
            Some(result)
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

    // Typed events are published only after the transaction is durable, so a
    // subscriber that reacts by querying a snapshot always observes the
    // committed state. A replayed command (idempotency cache hit) returned
    // earlier and deliberately emits nothing a second time.
    for event in pending_events {
        crate::events::emit(event);
    }

    if let Some(request) = launch_request {
        if let Err(problem) = launcher(&request) {
            let failed_result = serde_json::json!({
                "agent_name": request.agent_name,
                "project_path": request.project_path,
                "status": "failed_to_launch",
            });
            let _ = conn.execute(
                "UPDATE cp_idempotency SET result_json = ?1, status = 'FAILED' WHERE idempotency_key = ?2",
                params![failed_result.to_string(), key],
            );
            return Err(problem);
        }
    }

    Ok(result_val)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points `dirs::config_dir()` at an empty tempdir for the duration of a
    /// test, so `Config::load()` inside `load_work_snapshot` never reads the
    /// live `~/.config/raios/config.toml`. Without this, the
    /// `factory.enabled` assertion in `snapshot_generation_on_in_memory_db`
    /// depends on the machine running the tests. Serialized on the crate-wide
    /// `TEST_ENV_LOCK` (see lib.rs) with the other env-redirecting tests.
    struct IsolatedConfigDir {
        _lock: std::sync::MutexGuard<'static, ()>,
        original_xdg_config_home: Option<String>,
        _tmp: tempfile::TempDir,
    }

    impl IsolatedConfigDir {
        fn new() -> Self {
            let lock = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tmp = tempfile::TempDir::new().unwrap();
            let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
            // dirs::config_dir() prefers XDG_CONFIG_HOME over HOME and only
            // accepts absolute paths; TempDir paths are absolute.
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
            Self {
                _lock: lock,
                original_xdg_config_home,
                _tmp: tmp,
            }
        }
    }

    impl Drop for IsolatedConfigDir {
        fn drop(&mut self) {
            match &self.original_xdg_config_home {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn snapshot_generation_on_in_memory_db() {
        let _isolated_config = IsolatedConfigDir::new();
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let snap = load_system_snapshot(&conn).unwrap();
        assert!(snap.sequence >= 1);
        assert!(!snap.timestamp.is_empty());
        assert_eq!(snap.now.active_runs.len(), 0);
        assert_eq!(snap.work.factory.product_count, 0);
        // An empty database does not imply the user's Factory feature is off.
        // Default-off policy is tested using Config::default in raios-core.
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
            approval_id: workflow.approval_id.clone(),
            expected_revision: None,
            reason: "Not needed".into(),
            idempotency_key: "idem-key-1".into(),
        };

        let actor = ControlActor::test_local();
        let res1 = dispatch_control_command(&mut conn, &actor, &cmd1);
        assert!(res1.is_ok());
        // Rejection must close the entire delivery workflow, not just hide the
        // approval while its task/run remain active forever.
        let snapshot = load_system_snapshot(&conn).unwrap();
        assert!(!snapshot
            .work
            .tasks
            .iter()
            .any(|task| task.id == workflow.task_id));
        assert!(!snapshot
            .now
            .active_runs
            .iter()
            .any(|run| run.run_id == workflow.agent_run_id));
        let states: (String, String, String, String) = conn
            .query_row(
                "SELECT ap.status, a.status, r.status, t.status
             FROM cp_approvals ap JOIN cp_artifacts a ON a.id = ap.artifact_id
             JOIN cp_agent_runs r ON r.id = ap.agent_run_id JOIN cp_tasks t ON t.id = ap.task_id
             WHERE ap.id = ?1",
                params![workflow.approval_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            states,
            (
                "rejected".into(),
                "rejected".into(),
                "cancelled".into(),
                "cancelled".into()
            )
        );

        // Same key + same payload -> OK cached
        let res2 = dispatch_control_command(&mut conn, &actor, &cmd1);
        assert!(res2.is_ok());

        // Same key + different payload -> Error (invalid input / collision)
        let cmd2 = Command::RejectHandoff {
            approval_id: "app-different".into(),
            expected_revision: None,
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
            expected_revision: None,
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

        // The owner's approval is consent, not delivery: the run leaves the queue, but the
        // task stays open and the receipt must not read "delivered by the target".
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_tasks WHERE id = ?1",
                params![workflow.task_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "awaiting_approval"
        );
        assert_eq!(
            conn.query_row(
                "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
                params![workflow.agent_run_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "handover_approved_by_owner"
        );
        let receipt = raios_core::db::cp_handoff_receipt(
            &conn,
            &workflow.approval_id,
            "/tmp/control-plane-flow",
        )
        .unwrap()
        .expect("receipt");
        assert_eq!(receipt.status, "approved");
        assert!(!receipt.delivered_by_target());
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 2);

        // Only the addressed agent's own delivery closes the task and flips the receipt.
        assert!(raios_core::db::cp_mark_handoff_delivered(
            &conn,
            &workflow.approval_id,
            "antigravity_kaira"
        )
        .unwrap());
        let delivered = raios_core::db::cp_handoff_receipt(
            &conn,
            &workflow.approval_id,
            "/tmp/control-plane-flow",
        )
        .unwrap()
        .expect("receipt");
        assert_eq!(delivered.status, "approved");
        assert!(delivered.delivered_by_target());
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_tasks WHERE id = ?1",
                params![workflow.task_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "completed"
        );
        // A run that already ended by owner approval keeps that reason: delivery is recorded
        // on the approval, artifact and task, never by re-labelling the run.
        assert_eq!(
            conn.query_row(
                "SELECT exit_reason FROM cp_agent_runs WHERE id = ?1",
                params![workflow.agent_run_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "handover_approved_by_owner"
        );
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 3);
    }

    #[test]
    fn stale_expected_revision_is_refused_with_conflict() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let workflow = raios_core::db::create_handoff_workflow(
            &conn,
            raios_core::db::HandoffWorkflowInput {
                project_path: "/tmp/control-plane-conflict",
                from_agent: "codex_kaira",
                to_agent: "antigravity_kaira",
                status: "blocker",
                msg: "Conflict coverage",
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();
        let actor = ControlActor::test_local();

        // A decision on a revision the row never had is refused without touching the row.
        let stale = Command::RejectHandoff {
            approval_id: workflow.approval_id.clone(),
            expected_revision: Some(99),
            reason: "stale decision".into(),
            idempotency_key: "conflict-stale".into(),
        };
        let error = dispatch_control_command(&mut conn, &actor, &stale).unwrap_err();
        assert_eq!(error.code, "CONFLICT");
        let status: String = conn
            .query_row(
                "SELECT status FROM cp_approvals WHERE id = ?1",
                params![workflow.approval_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");

        // The revision read before the decision is accepted once.
        let fresh = Command::ApproveHandoff {
            approval_id: workflow.approval_id.clone(),
            expected_revision: Some(1),
            idempotency_key: "conflict-fresh".into(),
        };
        let result = dispatch_control_command(&mut conn, &actor, &fresh)
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "approved");

        // Re-deciding on the same, now stale revision is a conflict, not a silent success.
        let again = Command::ApproveHandoff {
            approval_id: workflow.approval_id.clone(),
            expected_revision: Some(1),
            idempotency_key: "conflict-again".into(),
        };
        let error = dispatch_control_command(&mut conn, &actor, &again).unwrap_err();
        assert_eq!(error.code, "CONFLICT");

        // A named revision that matches no approval at all stays `NOT_FOUND`.
        let missing = Command::RejectHandoff {
            approval_id: "no-such-approval".into(),
            expected_revision: Some(1),
            reason: "nowhere".into(),
            idempotency_key: "conflict-missing".into(),
        };
        let error = dispatch_control_command(&mut conn, &actor, &missing).unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
    }

    /// Drains the typed-event bus for `expected` messages matching `matching`.
    ///
    /// `emit` sends synchronously inside dispatch, so by the time a dispatch
    /// call returns its events are already queued. Other tests may dispatch
    /// control-plane commands concurrently on the same process-wide bus, hence
    /// the filter.
    fn collect_typed_events(
        rx: &mut tokio::sync::broadcast::Receiver<String>,
        expected: usize,
        matching: impl Fn(&serde_json::Value) -> bool,
    ) -> Vec<serde_json::Value> {
        let mut found = Vec::new();
        while found.len() < expected {
            match rx.try_recv() {
                Ok(msg) => {
                    let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
                    if matching(&value) {
                        found.push(value);
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    panic!(
                        "expected {expected} matching typed events, got {}: {found:?}",
                        found.len()
                    );
                }
                Err(error) => panic!("typed event bus receive failed: {error}"),
            }
        }
        found
    }

    #[test]
    fn handoff_lifecycle_emits_typed_events_only_after_commit() {
        // The typed bus is process-wide; tests that install it serialize on
        // the crate's shared global-state lock.
        let _lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(64);
        crate::events::install(&tx);

        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // 1. A queued handoff publishes the approval request plus the run's
        // observed creation state. The dispatcher requires an absolute path;
        // a POSIX literal like `/workspace/...` is not `Path::is_absolute` on
        // Windows, so the fixture comes from the platform temp root.
        let fixture_path = std::env::temp_dir()
            .join("raios-typed-events-fixture")
            .to_string_lossy()
            .into_owned();
        let create = Command::CreateHandoff {
            project_path: fixture_path,
            from_agent: "claude_kaira".into(),
            to_agent: "codex_kaira".into(),
            summary: "Typed event coverage".into(),
            idempotency_key: "typed-events-create".into(),
        };
        let created = dispatch_control_command(&mut conn, &ControlActor::test_local(), &create)
            .unwrap()
            .unwrap();
        let approval_id = created["approval_id"].as_str().unwrap().to_owned();
        let run_id = created["agent_run_id"].as_str().unwrap().to_owned();

        let own_ids = |value: &serde_json::Value| {
            value["payload"]["approval_id"].as_str() == Some(approval_id.as_str())
                || value["payload"]["run_id"].as_str() == Some(run_id.as_str())
        };
        let created_events = collect_typed_events(&mut rx, 2, own_ids);
        let requested = created_events
            .iter()
            .find(|v| v["event_type"] == "ApprovalRequested")
            .expect("ApprovalRequested must be emitted");
        assert_eq!(requested["payload"]["kind"], "handover");
        assert_eq!(requested["payload"]["target"], "codex_kaira");
        assert!(requested["payload"]["title"]
            .as_str()
            .unwrap()
            .contains("Handoff"));
        let run_created = created_events
            .iter()
            .find(|v| v["event_type"] == "AgentRunStateChanged")
            .expect("run creation must be emitted");
        assert_eq!(run_created["payload"]["agent_name"], "claude_kaira");
        let db_status: String = conn
            .query_row(
                "SELECT status FROM cp_agent_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            run_created["payload"]["status"].as_str(),
            Some(db_status.as_str()),
            "the emitted run status must mirror the committed row"
        );

        // 2. Approving publishes the resolution plus the run's observed end
        // state (never an assumed constant).
        let approve = Command::ApproveHandoff {
            approval_id: approval_id.clone(),
            expected_revision: None,
            idempotency_key: "typed-events-approve".into(),
        };
        dispatch_control_command(&mut conn, &ControlActor::test_local(), &approve)
            .unwrap()
            .unwrap();
        let resolved_events = collect_typed_events(&mut rx, 2, own_ids);
        let resolved = resolved_events
            .iter()
            .find(|v| v["event_type"] == "ApprovalResolved")
            .expect("ApprovalResolved must be emitted");
        assert_eq!(resolved["payload"]["approval_id"], approval_id);
        assert_eq!(resolved["payload"]["status"], "approved");
        let run_ended = resolved_events
            .iter()
            .find(|v| v["event_type"] == "AgentRunStateChanged")
            .expect("run end must be emitted");
        let db_status: String = conn
            .query_row(
                "SELECT status FROM cp_agent_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            run_ended["payload"]["status"].as_str(),
            Some(db_status.as_str())
        );

        // 3. An idempotent replay returns the cached result and must not emit
        // the same transition a second time.
        dispatch_control_command(&mut conn, &ControlActor::test_local(), &approve)
            .unwrap()
            .unwrap();
        while let Ok(msg) = rx.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&msg).unwrap();
            assert!(
                !own_ids(&value),
                "a replayed command must not re-emit its events: {value:?}"
            );
        }
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
            expected_revision: None,
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
    fn launch_agent_rejects_unknown_identity_before_terminal_spawn() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let unsupported = Command::LaunchAgent {
            agent_name: "codex; touch /tmp/pwned".into(),
            prompt: Some("Do something".into()),
            project_path: "/tmp".into(),
            idempotency_key: "idem-invalid-agent".into(),
        };

        let res = dispatch_control_command(&mut conn, &ControlActor::test_local(), &unsupported);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, "INVALID_INPUT");
    }

    #[test]
    fn launch_validation_bounds_identity_and_prompt_without_spawning() {
        assert_eq!(validate_agent_name(" AGY ").unwrap(), "antigravity");
        assert!(validate_agent_name("codex; touch /tmp/pwned").is_err());
        assert_eq!(
            validate_agent_prompt(Some("  inspect the project ")).unwrap(),
            Some("inspect the project".into())
        );
        assert!(validate_agent_prompt(Some("--dangerous-extra-flag")).is_err());
        assert!(validate_agent_prompt(Some(&"x".repeat(MAX_AGENT_PROMPT_CHARS + 1))).is_err());
    }

    #[test]
    fn tui_personal_task_create_and_status_update_are_transactional() {
        let project_path = if cfg!(windows) {
            r"C:\workspace\raios"
        } else {
            "/workspace/raios"
        };
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (name, path) VALUES ('R-AI-OS', ?1)",
            params![project_path],
        )
        .unwrap();

        let create = Command::CreateTask {
            title: "Build WORK task composer".into(),
            project_path: Some(project_path.into()),
            priority: 180,
            idempotency_key: "create-tui-task".into(),
        };
        let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &create)
            .unwrap()
            .unwrap();
        let task_id = result["task_id"].as_str().unwrap().to_owned();
        assert_eq!(result["status"], "queued");

        let snapshot = load_work_snapshot(&conn).unwrap();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(task.project_path.as_deref(), Some(project_path));
        assert_eq!(task.priority, 180);

        let update = Command::UpdateTaskStatus {
            task_id: task_id.clone(),
            status: "in_progress".into(),
            idempotency_key: "update-tui-task".into(),
        };
        let result = dispatch_control_command(&mut conn, &ControlActor::test_local(), &update)
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "in_progress");
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "in_progress"
        );
    }

    #[test]
    fn task_commands_reject_invalid_but_allow_canonical_personal_task_mutations() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let invalid_create = Command::CreateTask {
            title: " ".into(),
            project_path: None,
            priority: 50,
            idempotency_key: "empty-tui-task".into(),
        };
        assert_eq!(
            dispatch_control_command(&mut conn, &ControlActor::test_local(), &invalid_create)
                .unwrap_err()
                .code,
            "INVALID_INPUT"
        );

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cp_tasks
             (id, plan_id, parent_task_id, title, description, priority, status,
              acceptance_criteria, created_at, updated_at)
             VALUES ('markdown-task', NULL, NULL, 'Legacy task', '', 50, 'queued', '', ?1, ?1)",
            params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cp_task_list_items
             (task_id, source_kind, source_path, display_order, created_at)
             VALUES ('markdown-task', 'markdown', 'tasks.md', 0, ?1)",
            params![now],
        )
        .unwrap();
        let markdown_update = Command::UpdateTaskStatus {
            task_id: "markdown-task".into(),
            status: "completed".into(),
            idempotency_key: "complete-markdown-task".into(),
        };
        assert_eq!(
            dispatch_control_command(&mut conn, &ControlActor::test_local(), &markdown_update)
                .unwrap()
                .unwrap()["status"],
            "completed"
        );
    }
}
