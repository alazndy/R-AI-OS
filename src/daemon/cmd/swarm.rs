use crate::swarm::store::SwarmStore;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub async fn handle_create_swarm_task<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    swarm_store: &Arc<SwarmStore>,
    writer: &mut W,
) {
    let project_name = v["project_name"].as_str().unwrap_or("unknown").to_string();
    let project_path = v["project_path"].as_str().unwrap_or(".").to_string();
    let description = v["description"].as_str().unwrap_or("").to_string();
    let agent = v["agent"].as_str().unwrap_or("claude").to_string();
    let r = match swarm_store.create(
        &project_name,
        std::path::Path::new(&project_path),
        &description,
        &agent,
    ) {
        Ok(task) => serde_json::json!({ "event": "SwarmTaskCreated", "task_id": task.id }),
        Err(e) => serde_json::json!({ "event": "SwarmError", "error": e.to_string() }),
    };
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}

pub async fn handle_get_swarm_task<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    swarm_store: &Arc<SwarmStore>,
    writer: &mut W,
) {
    if let Some(id) = v["task_id"].as_str() {
        let r = match swarm_store.get(id) {
            Some(task) => serde_json::json!({ "event": "SwarmTaskState", "task": task }),
            None => serde_json::json!({
                "event": "SwarmError",
                "error": format!("task {} not found", id)
            }),
        };
        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
    }
}

pub async fn handle_list_swarm_tasks<W: AsyncWriteExt + Unpin>(
    swarm_store: &Arc<SwarmStore>,
    writer: &mut W,
) {
    let tasks = swarm_store.list_active();
    let r = serde_json::json!({ "event": "SwarmTaskList", "tasks": tasks });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}

pub async fn handle_approve_swarm_task<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    swarm_store: &Arc<SwarmStore>,
    writer: &mut W,
) {
    if let Some(id) = v["task_id"].as_str() {
        if let Some(task) = swarm_store.get(id) {
            let msg = format!("swarm merge: {}", task.task_description);
            let r =
                match crate::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg)
                {
                    Ok(_) => {
                        let _ = crate::swarm::worktree::remove_worktree(
                            &task.project_path,
                            &task.worktree_path,
                        );
                        swarm_store.set_status(id, crate::swarm::SwarmStatus::Merged);
                        serde_json::json!({ "event": "SwarmTaskMerged", "task_id": id })
                    }
                    Err(e) => serde_json::json!({ "event": "SwarmError", "error": e.to_string() }),
                };
            let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
        }
    }
}

pub async fn handle_reject_swarm_task<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    swarm_store: &Arc<SwarmStore>,
    writer: &mut W,
) {
    if let Some(id) = v["task_id"].as_str() {
        if let Some(task) = swarm_store.get(id) {
            let _ = crate::swarm::worktree::remove_worktree(
                &task.project_path,
                &task.worktree_path,
            );
            let _ =
                crate::swarm::merge::delete_branch(&task.project_path, &task.branch_name);
            swarm_store.set_status(id, crate::swarm::SwarmStatus::Rejected);
            let r = serde_json::json!({ "event": "SwarmTaskRejected", "task_id": id });
            let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
        }
    }
}
