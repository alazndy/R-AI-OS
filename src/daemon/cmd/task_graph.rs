use crate::factory::Factory;
use crate::task_graph::GraphStore;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub async fn handle_create_task_graph<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    graph_store: &Arc<GraphStore>,
    writer: &mut W,
) {
    let goal = v["goal"].as_str().unwrap_or("unnamed goal").to_string();
    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();
    let nodes_val = v["nodes"].as_array().cloned().unwrap_or_default();
    let nodes: Vec<crate::task_graph::NodeSpec> = nodes_val
        .iter()
        .filter_map(|n| {
            Some(crate::task_graph::NodeSpec {
                id: n["id"].as_str()?.to_string(),
                description: n["description"].as_str()?.to_string(),
                shell_cmd: n["shell_cmd"].as_str()?.to_string(),
                deps: n["deps"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| d.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    let response = match graph_store.create(&goal, &agent_name, nodes) {
        Ok(graph_id) => serde_json::json!({ "event": "TaskGraphCreated", "graph_id": graph_id }),
        Err(e) => serde_json::json!({ "event": "TaskGraphError", "error": e.to_string() }),
    };
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

pub async fn handle_execute_task_graph<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    graph_store: &Arc<GraphStore>,
    factory: &Arc<Factory>,
    writer: &mut W,
) {
    if let Some(graph_id) = v["graph_id"].as_str().map(|s| s.to_string()) {
        let store = graph_store.clone();
        let fac = factory.clone();
        let gid = graph_id.clone();
        tokio::spawn(async move {
            execute_graph_async(store, fac, gid).await;
        });
        let r = serde_json::json!({ "event": "TaskGraphExecuting", "graph_id": graph_id });
        let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
    }
}

pub async fn handle_get_task_graph<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    graph_store: &Arc<GraphStore>,
    writer: &mut W,
) {
    if let Some(graph_id) = v["graph_id"].as_str() {
        let response = match graph_store.get(graph_id) {
            Some(graph) => serde_json::json!({ "event": "TaskGraphState", "graph": graph }),
            None => serde_json::json!({
                "event": "TaskGraphError",
                "error": format!("graph {} not found", graph_id)
            }),
        };
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    }
}

async fn execute_graph_async(store: Arc<GraphStore>, factory: Arc<Factory>, graph_id: String) {
    let timeout = std::time::Duration::from_secs(600);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            eprintln!("[TaskGraph] Execution timeout for graph {}", graph_id);
            break;
        }

        let _ = store.ready_nodes(&graph_id);

        let ready_tasks: Vec<(String, String, String)> = if let Ok(conn) = crate::db::open_db() {
            crate::db::cp_scheduler_list_ready(&conn)
                .unwrap_or_default()
                .into_iter()
                .filter(|t| t.plan_id.as_deref() == Some(graph_id.as_str()))
                .filter_map(|t| {
                    let cmd = crate::db::cp_task_graph_shell_cmd(&conn, &t.id)
                        .ok()
                        .flatten()?;
                    let (_, node_id) = crate::db::cp_task_graph_node_ids(&conn, &t.id)
                        .ok()
                        .flatten()?;
                    Some((t.id, node_id, cmd))
                })
                .collect()
        } else {
            store
                .ready_nodes(&graph_id)
                .into_iter()
                .filter_map(|n| {
                    let task_id = n.task_id?;
                    Some((task_id, n.id, n.shell_cmd))
                })
                .collect()
        };

        if ready_tasks.is_empty() {
            if let Some(graph) = store.get(&graph_id) {
                if graph.status != "pending" && graph.status != "running" {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }

        for (_task_id, node_id, cmd) in ready_tasks {
            let store2 = store.clone();
            let factory2 = factory.clone();
            let gid = graph_id.clone();
            let nid = node_id.clone();
            let desc = format!("Graph node {}", node_id);

            let job = crate::factory::Job::new(&desc, "graph-executor", None, None);
            let job_id = job.id;

            store2.mark_node_running(&gid, &nid, &job_id.to_string());

            factory2.submit(
                job,
                Box::pin(async move {
                    let (program, args) = crate::core::process::shell_command(&cmd);
                    let output = tokio::process::Command::new(&program)
                        .args(&args)
                        .output()
                        .await?;
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let result = if output.status.success() {
                        Ok(stdout)
                    } else {
                        Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
                    };
                    match &result {
                        Ok(r) => store2.mark_node_complete(&gid, &nid, r, &job_id.to_string()),
                        Err(e) => store2.mark_node_failed(&gid, &nid, &e.to_string()),
                    }
                    result
                }),
            );
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
