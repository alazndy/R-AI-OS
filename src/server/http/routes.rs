use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;

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
    match crate::db::open_db() {
        Ok(conn) => {
            let tasks = crate::db::cp_query_active_tasks(&conn).unwrap_or_default();
            let approvals = crate::db::cp_query_pending_approvals(&conn).unwrap_or_default();
            let runs = crate::db::cp_query_active_runs(&conn).unwrap_or_default();
            let blocked = crate::db::cp_query_blocked_tasks(&conn).unwrap_or_default();
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

    resolved.starts_with(allowed_base).then_some(resolved)
}

fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(Into::into)
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
}
