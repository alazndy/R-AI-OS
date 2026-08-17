use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use raios_core::config::Config;

use super::{plans, AppState};

pub(super) async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.daemon_state.read().await;
    let payload = json!({
        "status": "ok",
        "handover_count": s.handover_count,
        "needs_human_approval": s.needs_human_approval,
        "active_agents": s.active_agents,
    });
    Json(payload)
}

pub(super) async fn handle_projects(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.daemon_state.read().await;
    Json(s.projects.clone())
}

pub(super) async fn handle_tasks() -> impl IntoResponse {
    let config =
        Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));

    match crate::tasks::load_tasks(&config.dev_ops_path) {
        Ok(tasks) => Json(json!({ "status": "ok", "tasks": tasks })),
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

pub(super) async fn handle_inbox() -> impl IntoResponse {
    match raios_core::db::open_db() {
        Ok(conn) => {
            let tasks = raios_core::db::cp_query_active_tasks(&conn).unwrap_or_default();
            let approvals =
                raios_core::db::cp_query_pending_approvals_scored(&conn).unwrap_or_default();
            let runs = raios_core::db::cp_query_active_runs(&conn).unwrap_or_default();
            let blocked = raios_core::db::cp_query_blocked_tasks(&conn).unwrap_or_default();
            Json(json!({
                "status": "ok",
                "active_tasks": tasks,
                "pending_approvals": approvals,
                "active_runs": runs,
                "blocked_tasks": blocked,
            }))
        }
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

pub(super) async fn handle_usage() -> impl IntoResponse {
    let report = crate::system_scan::scan_system();
    Json(json!({ "status": "ok", "usage": report.usage }))
}

#[derive(Deserialize)]
pub(super) struct ApprovePayload {
    task_id: String,
}

pub(super) async fn handle_approve(
    State(state): State<AppState>,
    Json(payload): Json<ApprovePayload>,
) -> impl IntoResponse {
    let swarm_store = Arc::new(crate::swarm::store::SwarmStore::new(
        crate::swarm::store::SwarmStore::default_path(),
    ));

    if let Some(task) = swarm_store.get(&payload.task_id) {
        let msg = format!("swarm merge: {}", task.task_description);
        match crate::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg) {
            Ok(_) => {
                let _ = crate::swarm::worktree::remove_worktree(
                    &task.project_path,
                    &task.worktree_path,
                );
                swarm_store.set_status(&payload.task_id, crate::swarm::SwarmStatus::Merged);
                return Json(
                    json!({ "status": "ok", "message": format!("Swarm task {} approved and merged", payload.task_id) }),
                );
            }
            Err(e) => {
                return Json(json!({ "status": "error", "message": e.to_string() }));
            }
        }
    }

    let mut s = state.daemon_state.write().await;
    if let Some(pos) = s.pending_diffs.iter().position(|d| d.id == payload.task_id) {
        let Some(diff) = s.pending_diffs.remove(pos) else {
            return Json(
                json!({ "status": "error", "message": "Pending diff disappeared before approval" }),
            );
        };
        drop(s);

        if let Ok(content) = decode_base64(&diff.proposed) {
            let file_path = Path::new(&diff.file_path);
            if let Some(config) = Config::load() {
                if let Ok(allowed_base) = config.dev_ops_path.canonicalize() {
                    if resolve_pending_diff_target(file_path, &allowed_base).is_some()
                        && std::fs::write(file_path, content).is_ok()
                    {
                        return Json(
                            json!({ "status": "ok", "message": format!("File diff {} approved and written", payload.task_id) }),
                        );
                    }
                }
            }
        }
        return Json(json!({ "status": "error", "message": "Failed to apply proposed changes" }));
    }

    Json(json!({ "status": "error", "message": "Task or diff ID not found" }))
}

#[derive(Deserialize)]
pub(super) struct SteerPayload {
    agent_id: String,
    message: String,
    sender: String,
}

pub(super) async fn handle_steer(
    Extension(actor): Extension<crate::control_plane::service::ControlActor>,
    State(state): State<AppState>,
    Json(payload): Json<SteerPayload>,
) -> impl IntoResponse {
    // Same gate the sibling mutating routes apply: `handle_cp_command` and
    // `handle_factory_command` pass their `ControlActor` into a dispatcher
    // whose first act is to reject any principal without
    // `may_mutate_control_plane`. Steering has no dispatcher of its own, so
    // the check lands here — but it is the identical rule, not a new scheme.
    // `auth.rs` builds a remote-API-key principal without that grant on
    // purpose: authentication is not an ownership grant. Without this, under
    // a non-`localhost` `bind_mode` (`tailscale`/`all`) any remote key holder
    // could inject keystrokes into local agent sessions.
    if !actor.may_mutate_control_plane() {
        return Json(json!({
            "status": "error",
            "message": "This authenticated principal is not authorized to steer local agent sessions",
        }));
    }

    let id = match uuid::Uuid::parse_str(&payload.agent_id) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "status": "error",
                "message": format!("'{}' is not a valid agent id", payload.agent_id),
            }));
        }
    };

    // `.with_event_tx(...)` is what makes `push_event`'s `AgentSteered`
    // broadcast actually reach connected TUI/dashboard clients — a bare
    // `ExecutionProxy::new(...)` has `event_tx: None` and drops every event
    // silently, so the design spec's "a live TUI reflects a steer
    // immediately" requirement was unmet on this, the only production steer
    // path. Mirrors `daemon/server.rs`'s existing
    // `execution_proxy.clone().with_event_tx(tx.clone())` call site.
    let proxy = crate::daemon::proxy::ExecutionProxy::new(state.daemon_state.clone())
        .with_event_tx(state.tx.clone());
    match proxy
        .steer_agent(id, &payload.message, &payload.sender)
        .await
    {
        Ok(_) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

pub(super) async fn handle_plans() -> impl IntoResponse {
    let plans_dir = plans::locate_plans_dir();
    let plans = match plans_dir {
        Some(dir) => plans::scan_plans(&dir),
        None => vec![],
    };
    Json(json!({ "plans": plans }))
}

#[derive(Deserialize)]
pub(super) struct PathQuery {
    path: Option<String>,
}

pub(super) async fn handle_git_status(Query(params): Query<PathQuery>) -> impl IntoResponse {
    let path = params
        .path
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".to_string());

    let out = std::process::Command::new("git")
        .args(["-C", &path, "status", "--porcelain=v1", "-b"])
        .output();

    match out {
        Err(_) => Json(json!({ "error": "git not available" })),
        Ok(output) if !output.status.success() && output.stdout.is_empty() => {
            Json(json!({ "error": "not a git repo" }))
        }
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut branch = "unknown".to_string();
            let mut staged: u32 = 0;
            let mut modified: u32 = 0;
            let mut untracked: u32 = 0;

            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("## ") {
                    branch = rest.split("...").next().unwrap_or(rest).to_string();
                } else if line.len() >= 2 {
                    let bytes = line.as_bytes();
                    let x = bytes[0] as char;
                    let y = bytes[1] as char;
                    if x == '?' && y == '?' {
                        untracked += 1;
                    } else {
                        if x != ' ' {
                            staged += 1;
                        }
                        if y != ' ' {
                            modified += 1;
                        }
                    }
                }
            }

            let dirty = staged + modified + untracked > 0;
            Json(json!({
                "branch": branch,
                "dirty": dirty,
                "staged": staged,
                "modified": modified,
                "untracked": untracked,
            }))
        }
    }
}

pub(super) async fn handle_swarm() -> impl IntoResponse {
    let store =
        crate::swarm::store::SwarmStore::new(crate::swarm::store::SwarmStore::default_path());
    let tasks: Vec<_> = store
        .list_active()
        .iter()
        .map(|t| {
            let status = match &t.status {
                crate::swarm::SwarmStatus::Initializing => "initializing",
                crate::swarm::SwarmStatus::Running => "running",
                crate::swarm::SwarmStatus::AwaitingReview => "awaiting_review",
                crate::swarm::SwarmStatus::Merged => "merged",
                crate::swarm::SwarmStatus::Rejected => "rejected",
                crate::swarm::SwarmStatus::Failed(_) => "failed",
            };
            json!({
                "id": t.id.to_string(),
                "project": t.project_name,
                "description": t.task_description,
                "agent": t.agent,
                "status": status,
                "created_at": t.created_at,
            })
        })
        .collect();
    Json(json!({ "tasks": tasks }))
}

fn resolve_pending_diff_target(file_path: &Path, allowed_base: &Path) -> Option<PathBuf> {
    let resolved = if file_path.exists() {
        file_path.canonicalize().ok()
    } else {
        let file_name = file_path.file_name()?;
        file_path
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|parent| parent.join(file_name))
    }?;

    // Compare against the canonicalized base too — otherwise this check spuriously
    // rejects legitimate paths (and could as easily hide a real escape) whenever
    // `allowed_base` isn't already fully resolved, e.g. macOS's /tmp -> /private/tmp
    // symlink or Windows's \\?\ canonical prefix.
    let allowed_base = allowed_base
        .canonicalize()
        .unwrap_or_else(|_| allowed_base.to_path_buf());
    resolved.starts_with(&allowed_base).then_some(resolved)
}

pub(super) async fn handle_cp_query(
    Json(query): Json<raios_contracts::Query>,
) -> impl IntoResponse {
    let Ok(conn) = raios_core::db::open_db() else {
        return Json(
            json!({"status": "error", "problem": raios_contracts::Problem::internal("Failed to open database")}),
        );
    };
    match query {
        raios_contracts::Query::GetSystemSnapshot => {
            match crate::control_plane::service::load_system_snapshot(&conn) {
                Ok(snap) => Json(json!({"status": "ok", "snapshot": snap})),
                Err(e) => Json(
                    json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}),
                ),
            }
        }
        raios_contracts::Query::GetNowSnapshot => {
            match crate::control_plane::service::load_now_snapshot(&conn) {
                Ok(snap) => Json(json!({"status": "ok", "now": snap})),
                Err(e) => Json(
                    json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}),
                ),
            }
        }
        raios_contracts::Query::GetWorkSnapshot => {
            match crate::control_plane::service::load_work_snapshot(&conn) {
                Ok(snap) => Json(json!({"status": "ok", "work": snap})),
                Err(e) => Json(
                    json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}),
                ),
            }
        }
        raios_contracts::Query::GetExploreSnapshot { search_query, .. } => {
            match crate::control_plane::service::load_explore_snapshot(
                &conn,
                search_query.as_deref(),
            ) {
                Ok(snap) => Json(json!({"status": "ok", "explore": snap})),
                Err(e) => Json(
                    json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}),
                ),
            }
        }
        raios_contracts::Query::GetGovernSnapshot => {
            match crate::control_plane::service::load_govern_snapshot(&conn) {
                Ok(snap) => Json(json!({"status": "ok", "govern": snap})),
                Err(e) => Json(
                    json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}),
                ),
            }
        }
        _ => Json(
            json!({"status": "error", "problem": raios_contracts::Problem::not_implemented("Query variant not supported via HTTP API")}),
        ),
    }
}

pub(super) async fn handle_cp_command(
    Extension(actor): Extension<crate::control_plane::service::ControlActor>,
    Json(cmd): Json<raios_contracts::Command>,
) -> impl IntoResponse {
    let Ok(mut conn) = raios_core::db::open_db() else {
        return Json(
            json!({"status": "error", "problem": raios_contracts::Problem::internal("Failed to open database")}),
        );
    };
    match crate::control_plane::service::dispatch_control_command(&mut conn, &actor, &cmd) {
        Ok(val) => Json(json!({"status": "ok", "result": val})),
        Err(problem) => Json(json!({"status": "error", "problem": problem})),
    }
}

pub(super) async fn handle_factory_overview() -> impl IntoResponse {
    let Ok(conn) = raios_core::db::open_db() else {
        return Json(
            json!({"status": "error", "problem": raios_contracts::Problem::internal("Failed to open database")}),
        );
    };

    match crate::control_plane::service::load_work_snapshot(&conn) {
        Ok(snap) => Json(json!({"status": "ok", "overview": snap.factory})),
        Err(e) => {
            Json(json!({"status": "error", "problem": raios_contracts::Problem::internal(e)}))
        }
    }
}

pub(super) async fn handle_factory_command(
    Extension(actor): Extension<crate::control_plane::service::ControlActor>,
    Json(cmd): Json<raios_contracts::FactoryCommand>,
) -> impl IntoResponse {
    if !http_may_execute(&cmd) {
        return Json(
            json!({"status": "error", "problem": raios_contracts::Problem::forbidden("factory_approval_required: this command must be approved by the human owner in the Product Factory UI")}),
        );
    }

    let factory_enabled = raios_core::config::Config::load()
        .map(|config| config.factory.enabled)
        .unwrap_or(false);

    let Ok(mut conn) = raios_core::db::open_db() else {
        return Json(
            json!({"status": "error", "problem": raios_contracts::Problem::internal("Failed to open database")}),
        );
    };

    match crate::product_factory::dispatch_factory_command(&mut conn, &actor, factory_enabled, &cmd)
    {
        Ok(val) => Json(json!({"status": "ok", "result": val})),
        Err(problem) => Json(json!({"status": "error", "problem": problem})),
    }
}

fn http_may_execute(command: &raios_contracts::FactoryCommand) -> bool {
    matches!(
        command,
        raios_contracts::FactoryCommand::CreateWorkspace { .. }
            | raios_contracts::FactoryCommand::CreateProductDraft { .. }
            | raios_contracts::FactoryCommand::SetProductMode { .. }
            | raios_contracts::FactoryCommand::AttachExistingProject { .. }
            | raios_contracts::FactoryCommand::StartIntake { .. }
            | raios_contracts::FactoryCommand::RecordIntakeAnswer { .. }
            | raios_contracts::FactoryCommand::CreateCharterDraft { .. }
            | raios_contracts::FactoryCommand::GenerateCharterDraft { .. }
            | raios_contracts::FactoryCommand::CreateRequirementDraft { .. }
            | raios_contracts::FactoryCommand::SubmitChangeRequest { .. }
            | raios_contracts::FactoryCommand::AssessChangeRequest { .. }
            | raios_contracts::FactoryCommand::CreatePlanDraft { .. }
            | raios_contracts::FactoryCommand::MaterializePlannedCycle { .. }
            | raios_contracts::FactoryCommand::PauseCycle { .. }
            | raios_contracts::FactoryCommand::ResumeCycle { .. }
            | raios_contracts::FactoryCommand::MaterializeStageTaskGraph { .. }
            | raios_contracts::FactoryCommand::RecordStageEvidence { .. }
            | raios_contracts::FactoryCommand::LinkStageEvidenceToRequirement { .. }
            | raios_contracts::FactoryCommand::InspectReleaseReadiness { .. }
            | raios_contracts::FactoryCommand::CreateQualityProfile { .. }
            | raios_contracts::FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { .. }
            | raios_contracts::FactoryCommand::RecordQualityCheck { .. }
            | raios_contracts::FactoryCommand::CreateReleaseDraft { .. }
            | raios_contracts::FactoryCommand::CreateSupportItem { .. }
            | raios_contracts::FactoryCommand::InspectSupportOverview { .. }
            | raios_contracts::FactoryCommand::TriageSupportItem { .. }
            | raios_contracts::FactoryCommand::ResolveSupportItem { .. }
            | raios_contracts::FactoryCommand::LinkSupportToChangeRequest { .. }
    )
}

fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(Into::into)
}

#[derive(Deserialize)]
pub(super) struct ClientIdQuery {
    client_id: String,
}

pub(super) async fn handle_notifications_important(
    Query(params): Query<ClientIdQuery>,
) -> impl IntoResponse {
    match raios_core::db::open_db() {
        Ok(conn) => match raios_core::db::poll_important_events(&conn, &params.client_id) {
            Ok(events) => Json(json!({ "status": "ok", "events": events })),
            Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
        },
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

pub(super) async fn handle_notifications_digest(
    Query(params): Query<ClientIdQuery>,
) -> impl IntoResponse {
    let config =
        Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => return Json(json!({ "status": "error", "message": e.to_string() })),
    };

    let window = match raios_core::db::poll_digest_window(
        &conn,
        &params.client_id,
        config.daemon.digest_interval_secs as i64,
    ) {
        Ok(w) => w,
        Err(e) => return Json(json!({ "status": "error", "message": e.to_string() })),
    };

    let Some(window) = window else {
        return Json(json!({ "status": "ok", "digest": null }));
    };

    let projects = raios_core::entities::discover_entities(&config.dev_ops_path);
    let snapshots: Vec<_> = projects
        .iter()
        .map(raios_runtime::reflect_scoring::snapshot)
        .collect();
    let top_recommendation = raios_runtime::reflect_scoring::build_recommendations(&snapshots)
        .into_iter()
        .next();

    let summary = build_digest_summary(&window.events);

    Json(json!({
        "status": "ok",
        "digest": {
            "since_ts": window.since_ts,
            "until_ts": window.until_ts,
            "summary": summary,
            "top_recommendation": top_recommendation,
            "event_count": window.events.len(),
        }
    }))
}

fn build_digest_summary(events: &[raios_core::db::ActivityEvent]) -> String {
    if events.is_empty() {
        return "No background activity.".to_string();
    }

    let mut by_source: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for e in events {
        *by_source.entry(e.source.as_str()).or_insert(0) += 1;
    }

    let order = ["git", "health", "scheduler", "agent_run"];
    let clauses: Vec<String> = order
        .iter()
        .filter_map(|source| {
            by_source.get(source).map(|count| match *source {
                "git" => format!("{count} git status change(s)"),
                "health" => format!("{count} health scan update(s)"),
                "scheduler" => format!("{count} scheduled job(s) ran"),
                "agent_run" => format!("{count} agent run(s) completed"),
                _ => unreachable!(),
            })
        })
        .collect();

    clauses.join("; ")
}

#[cfg(test)]
mod tests {
    use super::resolve_pending_diff_target;
    use tempfile::TempDir;

    #[test]
    fn resolve_pending_diff_target_accepts_existing_workspace_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();

        let resolved = resolve_pending_diff_target(&file, tmp.path()).unwrap();
        assert_eq!(resolved, file.canonicalize().unwrap());
    }

    #[test]
    fn resolve_pending_diff_target_rejects_path_without_filename() {
        let tmp = TempDir::new().unwrap();
        let resolved = resolve_pending_diff_target(std::path::Path::new(""), tmp.path());
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_pending_diff_target_rejects_outside_workspace() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let file = outside.path().join("other.rs");
        std::fs::write(&file, "").unwrap();

        let resolved = resolve_pending_diff_target(&file, tmp.path());
        assert!(resolved.is_none());
    }

    #[test]
    fn http_may_execute_rejects_human_approval_commands() {
        let cmd = raios_contracts::FactoryCommand::ApprovePlan {
            plan_id: "plan-1".into(),
            idempotency_key: "idem-1".into(),
        };
        assert!(!super::http_may_execute(&cmd));

        let allowed = raios_contracts::FactoryCommand::CreateWorkspace {
            name: "WS".into(),
            idempotency_key: "idem-2".into(),
        };
        assert!(super::http_may_execute(&allowed));
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn build_digest_summary_returns_quiet_message_when_no_events() {
        assert_eq!(build_digest_summary(&[]), "No background activity.");
    }

    #[test]
    fn build_digest_summary_groups_by_source_in_fixed_order() {
        let events = vec![
            raios_core::db::ActivityEvent {
                ts: "t1".into(),
                source: "scheduler".into(),
                project: None,
                summary: "x".into(),
            },
            raios_core::db::ActivityEvent {
                ts: "t2".into(),
                source: "git".into(),
                project: Some("a".into()),
                summary: "y".into(),
            },
        ];
        let summary = build_digest_summary(&events);
        // git must appear before scheduler, matching the fixed `order` array
        let git_pos = summary.find("git").unwrap();
        let sched_pos = summary.find("scheduled").unwrap();
        assert!(git_pos < sched_pos);
    }

    #[test]
    fn build_digest_summary_reports_exact_counts_per_source() {
        // Regression guard: a subtly wrong count (e.g. off-by-one from mixing
        // up sources, or double-counting) would still pass a substring-only
        // check like `contains("git")`. Assert the literal formatted clause.
        let events = vec![
            raios_core::db::ActivityEvent {
                ts: "t1".into(),
                source: "git".into(),
                project: Some("a".into()),
                summary: "a".into(),
            },
            raios_core::db::ActivityEvent {
                ts: "t2".into(),
                source: "git".into(),
                project: Some("b".into()),
                summary: "b".into(),
            },
            raios_core::db::ActivityEvent {
                ts: "t3".into(),
                source: "agent_run".into(),
                project: None,
                summary: "c".into(),
            },
        ];
        let summary = build_digest_summary(&events);
        assert_eq!(summary, "2 git status change(s); 1 agent run(s) completed");
    }

    #[test]
    fn build_digest_summary_omits_sources_with_zero_events() {
        // Only "health" events present — the other three fixed-order
        // categories (git, scheduler, agent_run) must not appear at all,
        // not just appear with a "0" count.
        let events = vec![raios_core::db::ActivityEvent {
            ts: "t1".into(),
            source: "health".into(),
            project: None,
            summary: "scan".into(),
        }];
        let summary = build_digest_summary(&events);
        assert_eq!(summary, "1 health scan update(s)");
        assert!(!summary.contains("git"));
        assert!(!summary.contains("scheduler"));
        assert!(!summary.contains("scheduled"));
        assert!(!summary.contains("agent run"));
    }
}
