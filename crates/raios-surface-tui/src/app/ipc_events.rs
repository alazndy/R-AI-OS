use std::path::PathBuf;
use std::sync::mpsc::Sender;

use raios_surface_tui::app::state::BgMsg;
use raios_runtime::indexer::SearchResult;

pub(crate) fn dispatch_event(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(evt) = serde_json::from_value::<raios_contracts::Event>(v.clone()) {
        tx.send(BgMsg::ControlEvent(evt)).ok();
    }

    match v["event"].as_str() {
        Some("FileChanged") => handle_file_changed(tx, v),
        Some("SearchResults") => handle_search_results(tx, v),
        Some("HealthReport") => handle_health_report(tx, v),
        Some("ActivePorts") => handle_active_ports(tx, v),
        Some("StateSync") => handle_state_sync(tx, v),
        Some("HumanApprovalRequired") => handle_human_approval_required(tx, v),
        Some("FileChangeRequested") => handle_file_change_requested(tx, v),
        Some("HandoverApproved") => handle_handover_approved(tx, v),
        Some("HumanApprovalResult") => handle_human_approval_result(tx, v),
        Some("NewLog") => handle_new_log(tx, v),
        Some("AgentStarted") => handle_agent_started(tx, v),
        Some("AgentStopped") => handle_agent_stopped(tx, v),
        Some("HealthDelta") => handle_health_delta(tx, v),
        Some("UmaiBlocked") => handle_umai_blocked(tx, v),
        Some("JobSubmitted") => handle_job_submitted(tx, v),
        Some("JobComplete") => handle_job_complete(tx, v),
        Some("JobFailed") => handle_job_failed(tx, v),
        _ => {}
    }
}

fn handle_file_changed(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Some(p) = v["path"].as_str() {
        tx.send(BgMsg::FileChanged(PathBuf::from(p))).ok();
    }
}

fn handle_search_results(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(r) = serde_json::from_value::<Vec<SearchResult>>(v["results"].clone()) {
        tx.send(BgMsg::SearchResults(r)).ok();
    }
}

fn handle_health_report(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(r) = serde_json::from_value::<Vec<raios_runtime::health::ProjectHealth>>(v["report"].clone())
    {
        tx.send(BgMsg::HealthReport(r)).ok();
    }
}

fn handle_active_ports(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(p) = serde_json::from_value::<Vec<u16>>(v["ports"].clone()) {
        tx.send(BgMsg::ActivePorts(p)).ok();
    }
}

fn handle_state_sync(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    let projects =
        serde_json::from_value::<Vec<raios_core::entities::EntityProject>>(v["projects"].clone())
            .unwrap_or_default();
    let health_reports =
        serde_json::from_value::<Vec<raios_runtime::health::ProjectHealth>>(v["health_reports"].clone())
            .unwrap_or_default();
    let active_agents =
        serde_json::from_value::<Vec<raios_runtime::daemon::proxy::AgentProcess>>(v["active_agents"].clone())
            .unwrap_or_default();
    let index_ready = v["index_ready"].as_bool().unwrap_or(false);
    let handover_count = v["handover_count"].as_u64().unwrap_or(0) as u32;
    let pending_file_changes =
        serde_json::from_value::<Vec<raios_runtime::daemon::state::FileChangeApproval>>(
            v["pending_file_changes"].clone(),
        )
        .unwrap_or_default();
    let sentinel_files =
        serde_json::from_value::<Vec<raios_runtime::daemon::state::SentinelFileStatus>>(
            v["sentinel_files"].clone(),
        )
        .unwrap_or_default();

    let report_count = health_reports.len();
    tx.send(BgMsg::StateSync {
        projects,
        health_reports,
        active_agents,
        index_ready,
        handover_count,
        pending_file_changes,
        sentinel_files,
    })
    .ok();

    tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
        "IPC",
        format!("Synced state from daemon (Reports: {})", report_count),
    )))
    .ok();
}

fn handle_human_approval_required(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let (Some(target), Some(instruction), Some(reason)) = (
        v["target"].as_str(),
        v["instruction"].as_str(),
        v["reason"].as_str(),
    ) {
        tx.send(BgMsg::HumanApprovalRequired {
            target: target.into(),
            instruction: instruction.into(),
            reason: reason.into(),
        })
        .ok();
    }
}

fn handle_file_change_requested(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(approval) =
        serde_json::from_value::<raios_runtime::daemon::state::FileChangeApproval>(v["approval"].clone())
    {
        tx.send(BgMsg::FileChangeRequested { approval }).ok();
    }
}

fn handle_handover_approved(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let (Some(target), Some(instruction), Some(count)) = (
        v["target"].as_str(),
        v["instruction"].as_str(),
        v["count"].as_u64(),
    ) {
        tx.send(BgMsg::HandoverApproved {
            target: target.into(),
            instruction: instruction.into(),
            count: count as u32,
        })
        .ok();
    }
}

fn handle_human_approval_result(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Some(status) = v["status"].as_str() {
        tx.send(BgMsg::HumanApprovalResult {
            status: status.into(),
        })
        .ok();
    }
}

fn handle_new_log(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(log) = serde_json::from_value::<raios_surface_tui::app::state::LogEntry>(v["log"].clone()) {
        tx.send(BgMsg::NewLog(log)).ok();
    }
}

fn handle_agent_started(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let (Some(id), Some(name), Some(path)) = (
        v["agent_id"].as_str(),
        v["name"].as_str(),
        v["project_path"].as_str(),
    ) {
        tx.send(BgMsg::AgentStarted {
            agent_id: id.into(),
            name: name.into(),
            project_path: path.into(),
        })
        .ok();
    }
}

fn handle_agent_stopped(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let (Some(id), Some(name), Some(status)) = (
        v["agent_id"].as_str(),
        v["name"].as_str(),
        v["final_status"].as_str(),
    ) {
        tx.send(BgMsg::AgentStopped {
            agent_id: id.into(),
            name: name.into(),
            final_status: status.into(),
        })
        .ok();
    }
}

fn handle_health_delta(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Ok(r) = serde_json::from_value::<Vec<raios_runtime::health::ProjectHealth>>(v["report"].clone())
    {
        tx.send(BgMsg::HealthDelta(r)).ok();
    }
}

fn handle_umai_blocked(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    if let Some(reason) = v["reason"].as_str() {
        tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
            "UMAI",
            format!("Blocked: {}", reason),
        )))
        .ok();
    }
}

fn handle_job_submitted(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    let job_id = v["job_id"].as_str().unwrap_or("?");
    tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
        "RUN",
        format!("⏳ Job queued [{}]", &job_id[..8.min(job_id.len())]),
    )))
    .ok();
}

fn handle_job_complete(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    let result = v["result"].as_str().unwrap_or("").trim().to_string();
    let job_id = v["job_id"].as_str().unwrap_or("?");
    let lines: Vec<&str> = result.lines().collect();
    if lines.is_empty() {
        tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
            "RUN",
            format!("✓ [{}] done (no output)", &job_id[..8.min(job_id.len())]),
        )))
        .ok();
    } else {
        for line in lines {
            tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
                "RUN",
                line.to_string(),
            )))
            .ok();
        }
    }
    tx.send(BgMsg::RemoteCommandResult { output: result }).ok();
}

fn handle_job_failed(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    let error = v["error"].as_str().unwrap_or("unknown error");
    let job_id = v["job_id"].as_str().unwrap_or("?");
    tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
        "RUN",
        format!("✗ [{}] {}", &job_id[..8.min(job_id.len())], error),
    )))
    .ok();
}
