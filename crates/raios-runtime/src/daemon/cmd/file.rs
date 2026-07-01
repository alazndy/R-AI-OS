use raios_core::config::Config;
use tokio::io::AsyncWriteExt;

type SharedState = std::sync::Arc<tokio::sync::RwLock<crate::daemon::state::DaemonState>>;

fn validate_file_change_target(
    target: &std::path::Path,
    workspace: &std::path::Path,
    blocked_paths: &[String],
) -> Result<std::path::PathBuf, String> {
    raios_core::security::sandbox::SandboxGuard::new(workspace.to_path_buf())
        .with_blocked_paths(blocked_paths.to_vec())
        .check(target)
        .map_err(|e| e.to_string())
}

fn policy_blocked_paths() -> Vec<String> {
    raios_core::security::PolicyConfig::try_load_default()
        .map(|p| p.filesystem.blocked_paths)
        .unwrap_or_default()
}

fn approval_workflow_ids(
    approval: &crate::daemon::state::FileChangeApproval,
) -> Option<raios_core::control_plane::FileChangeWorkflowIds> {
    Some(raios_core::control_plane::FileChangeWorkflowIds {
        task_id: approval.task_id.clone()?,
        agent_run_id: approval.agent_run_id.clone()?,
        artifact_id: approval.artifact_id.clone()?,
        approval_id: approval.id.clone(),
        project_id: None,
    })
}

fn decode_base64(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(Into::into)
}

pub async fn handle_request_file_change<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    state: &SharedState,
    sessions: &crate::session::SessionStore,
    session_id: &str,
    writer: &mut W,
) {
    let path = v["path"].as_str().unwrap_or("").to_string();
    if let Ok(conn) = raios_core::db::open_db() {
        if let Err(err) = raios_core::db::create_file_change_workflow(
            &conn,
            &path,
            v["original"].as_str().unwrap_or(""),
            v["new"].as_str().unwrap_or(""),
            v["agent"].as_str().unwrap_or("unknown"),
        ) {
            eprintln!("[Daemon] Failed to persist file change workflow: {}", err);
        }
    }
    let mut s = state.write().await;
    s.refresh_pending_from_db();
    sessions.record_event(session_id, "file_change_request", &format!("path={}", path));
    if let Some(approval) = s.pending_file_changes.iter().find(|a| a.path == path).cloned() {
        let event = serde_json::json!({
            "event": "FileChangeRequested",
            "approval": approval
        });
        let _ = writer.write_all(format!("{}\n", event).as_bytes()).await;
    }
}

/// Returns `CmdResult::Continue` when the change was rejected (preserves original `continue` behaviour).
pub async fn handle_approve_file_change(
    v: &serde_json::Value,
    state: &SharedState,
) -> super::CmdResult {
    if let Some(id_str) = v["id"].as_str() {
        let mut s = state.write().await;
        if let Some(pos) = s.pending_file_changes.iter().position(|a| a.id == id_str) {
            let approval = s.pending_file_changes.remove(pos);
            let approved = v["approved"].as_bool().unwrap_or(true);
            let workflow_ids = approval_workflow_ids(&approval);

            if !approved {
                println!("[Daemon] Rejected file change for: {}", approval.path);
                if let (Some(ids), Ok(conn)) = (workflow_ids, raios_core::db::open_db()) {
                    let _ = raios_core::db::mark_file_change_workflow_rejected(
                        &conn,
                        &ids,
                        "human",
                        "rejected_by_user",
                    );
                }
                s.refresh_pending_from_db();
                return super::CmdResult::Continue;
            }

            println!("[Daemon] Approved file change for: {}", approval.path);
            match Config::load() {
                Some(config) => {
                    let blocked_paths = policy_blocked_paths();
                    match validate_file_change_target(
                        std::path::Path::new(&approval.path),
                        &config.dev_ops_path,
                        &blocked_paths,
                    ) {
                        Ok(path) => match std::fs::write(path, &approval.new_content) {
                            Ok(_) => {
                                if let (Some(ids), Ok(conn)) =
                                    (workflow_ids, raios_core::db::open_db())
                                {
                                    let _ = raios_core::db::mark_file_change_workflow_applied(
                                        &conn, &ids, "human",
                                    );
                                }
                            }
                            Err(err) => {
                                eprintln!(
                                    "[Daemon] Failed writing approved file change for {}: {}",
                                    approval.path, err
                                );
                                if let (Some(ids), Ok(conn)) =
                                    (approval_workflow_ids(&approval), raios_core::db::open_db())
                                {
                                    let _ = raios_core::db::mark_file_change_workflow_apply_failed(
                                        &conn,
                                        &ids,
                                        "human",
                                        &format!("write_failed: {}", err),
                                    );
                                }
                            }
                        },
                        Err(err) => {
                            eprintln!(
                                "[Daemon] Rejected file change for {}: {}",
                                approval.path, err
                            );
                            if let (Some(ids), Ok(conn)) =
                                (approval_workflow_ids(&approval), raios_core::db::open_db())
                            {
                                let _ = raios_core::db::mark_file_change_workflow_apply_failed(
                                    &conn,
                                    &ids,
                                    "human",
                                    &format!("validation_failed: {}", err),
                                );
                            }
                        }
                    }
                }
                None => {
                    eprintln!(
                        "[Daemon] Rejected file change for {}: config not loaded",
                        approval.path
                    );
                    if let (Some(ids), Ok(conn)) =
                        (approval_workflow_ids(&approval), raios_core::db::open_db())
                    {
                        let _ = raios_core::db::mark_file_change_workflow_apply_failed(
                            &conn,
                            &ids,
                            "human",
                            "config_not_loaded",
                        );
                    }
                }
            }
            s.refresh_pending_from_db();
        }
    }
    super::CmdResult::Ok
}

pub async fn handle_human_approval<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    state: &SharedState,
    writer: &mut W,
) {
    if let Some(approved) = v["approved"].as_bool() {
        let mut s = state.write().await;
        if approved {
            println!("[Daemon] Human approved handover. Resetting counter.");
            s.handover_count = 0;
            let _ = writer
                .write_all(b"{\"event\":\"HumanApprovalResult\",\"status\":\"approved\"}\n")
                .await;
        } else {
            println!("[Daemon] Human rejected handover.");
            let _ = writer
                .write_all(b"{\"event\":\"HumanApprovalResult\",\"status\":\"rejected\"}\n")
                .await;
        }
    }
}

pub async fn handle_get_pending_diffs<W: AsyncWriteExt + Unpin>(
    state: &SharedState,
    writer: &mut W,
) {
    let s = state.read().await;
    let diffs: Vec<&crate::daemon::state::PendingDiff> = s.pending_diffs.iter().collect();
    let response = serde_json::json!({ "event": "PendingDiffs", "diffs": diffs });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

/// Returns `CmdResult::Disconnect` when a path-traversal violation is detected.
pub async fn handle_approve_diff<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    state: &SharedState,
    writer: &mut W,
) -> super::CmdResult {
    let diff_id = v["id"].as_str().unwrap_or("").to_string();
    let mut s = state.write().await;
    if let Some(pos) = s.pending_diffs.iter().position(|d| d.id == diff_id) {
        let diff = s.pending_diffs.remove(pos).unwrap();
        drop(s);
        match decode_base64(&diff.proposed) {
            Ok(content) => {
                let file_path = std::path::Path::new(&diff.file_path);
                let config = Config::load()
                    .unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));
                let allowed_base = match config.dev_ops_path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        let r = serde_json::json!({
                            "event": "DiffError",
                            "id": diff_id,
                            "error": "could not resolve allowed base path"
                        });
                        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                        return super::CmdResult::Disconnect;
                    }
                };
                let canonical_target = match file_path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        match file_path.parent().and_then(|p| p.canonicalize().ok()) {
                            Some(parent) if parent.starts_with(&allowed_base) => {
                                file_path.to_path_buf()
                            }
                            _ => {
                                let r = serde_json::json!({
                                    "event": "DiffError",
                                    "id": diff_id,
                                    "error": "file path is outside workspace"
                                });
                                let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                                return super::CmdResult::Disconnect;
                            }
                        }
                    }
                };
                if !canonical_target.starts_with(&allowed_base) {
                    let r = serde_json::json!({
                        "event": "DiffError",
                        "id": diff_id,
                        "error": "file path is outside workspace"
                    });
                    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                    return super::CmdResult::Disconnect;
                }
                match std::fs::write(file_path, &content) {
                    Ok(_) => {
                        let r = serde_json::json!({ "event": "DiffApproved", "id": diff_id });
                        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                    }
                    Err(e) => {
                        let r = serde_json::json!({
                            "event": "DiffError",
                            "id": diff_id,
                            "error": format!("write failed: {e}")
                        });
                        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
                    }
                }
            }
            Err(e) => {
                let r = serde_json::json!({
                    "event": "DiffError",
                    "id": diff_id,
                    "error": format!("base64 decode failed: {e}")
                });
                let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
            }
        }
    } else {
        drop(s);
        let r = serde_json::json!({
            "event": "DiffError",
            "id": diff_id,
            "error": format!("diff {} not found", diff_id)
        });
        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
    }
    super::CmdResult::Ok
}

pub async fn handle_reject_diff<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    state: &SharedState,
    writer: &mut W,
) {
    let diff_id = v["id"].as_str().unwrap_or("").to_string();
    let mut s = state.write().await;
    s.pending_diffs.retain(|d| d.id != diff_id);
    drop(s);
    let r = serde_json::json!({ "event": "DiffRejected", "id": diff_id });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}
