use crate::factory::{Factory, Job};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub async fn handle_submit_job<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    factory: &Arc<Factory>,
    writer: &mut W,
) {
    let description = v["description"]
        .as_str()
        .unwrap_or("unnamed task")
        .to_string();
    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();
    let project = v["project"].as_str().map(|s| s.to_string());
    let webhook_url = v["webhook_url"].as_str().map(|s| s.to_string());
    let shell_cmd = v["shell_cmd"].as_str().unwrap_or("").to_string();

    if shell_cmd.is_empty() {
        let err = serde_json::json!({ "event": "JobError", "error": "shell_cmd is required" });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
        return;
    }

    let job = Job::new(
        &description,
        &agent_name,
        project.as_deref(),
        webhook_url.as_deref(),
    );
    let task = Box::pin(async move {
        let (program, args) = raios_core::core::process::shell_command(&shell_cmd);
        let output = tokio::process::Command::new(&program)
            .args(&args)
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(stdout)
        } else {
            Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
        }
    });
    let job_id = factory.submit(job, task);
    let response = serde_json::json!({
        "event": "JobSubmitted",
        "job_id": job_id.to_string()
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

pub async fn handle_get_job<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    factory: &Arc<Factory>,
    writer: &mut W,
) {
    if let Some(id_str) = v["job_id"].as_str() {
        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
            let response = match factory.get(&id) {
                Some(job) => serde_json::json!({ "event": "JobInfo", "job": job }),
                None => serde_json::json!({
                    "event": "JobError",
                    "error": format!("job {} not found", id_str)
                }),
            };
            let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
        }
    }
}

pub async fn handle_list_inbox<W: AsyncWriteExt + Unpin>(
    v: &serde_json::Value,
    factory: &Arc<Factory>,
    writer: &mut W,
) {
    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
    let jobs = factory.list_inbox(limit);
    let r = serde_json::json!({ "event": "InboxList", "jobs": jobs });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}

pub async fn handle_list_running<W: AsyncWriteExt + Unpin>(factory: &Arc<Factory>, writer: &mut W) {
    let jobs = factory.list_running();
    let r = serde_json::json!({ "event": "RunningList", "jobs": jobs });
    let _ = writer.write_all(format!("{}\n", r).as_bytes()).await;
}
