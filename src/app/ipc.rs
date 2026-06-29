use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::app::state::BgMsg;
use crate::indexer::SearchResult;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

const LOCAL_DAEMON_ADDR: &str = "127.0.0.1:42069";
const RETRY_INTERVAL: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 10;

fn ensure_local_daemon_running() {
    if TcpStream::connect_timeout(
        &LOCAL_DAEMON_ADDR.parse().unwrap(),
        Duration::from_millis(200),
    )
    .is_ok()
    {
        return;
    }

    println!("Daemon not found. Spawning aiosd in background...");

    let mut cmd = Command::new("aiosd");

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    if let Ok(mut child) = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }

    thread::sleep(Duration::from_secs(2));
}

/// Read the auth token appropriate for the target address.
///   local  → ~/.config/raios/.ipc_token  (session token, short-lived)
///   remote → ~/.config/raios/.hub_api_key (persistent API key)
fn read_auth_token(is_remote: bool) -> Option<String> {
    let config_dir = dirs::config_dir()?.join("raios");
    let filename = if is_remote { ".hub_api_key" } else { ".ipc_token" };
    std::fs::read_to_string(config_dir.join(filename))
        .ok()
        .map(|s| s.trim().to_owned())
}

/// Connect to the local daemon (localhost).
pub fn connect_daemon(tx: Sender<BgMsg>) -> Option<Sender<String>> {
    connect_daemon_addr(tx, None)
}

/// Connect to daemon at an explicit address.
/// Pass `Some("100.x.x.x")` for remote Tailscale Hub access.
pub fn connect_daemon_addr(tx: Sender<BgMsg>, remote_host: Option<String>) -> Option<Sender<String>> {
    let daemon_addr = match &remote_host {
        Some(h) => {
            if h.contains(':') {
                h.clone()
            } else {
                format!("{h}:42069")
            }
        }
        None => LOCAL_DAEMON_ADDR.to_string(),
    };
    let is_remote = remote_host.is_some();

    let (tx_daemon_local, rx_daemon_local) = mpsc::channel::<String>();

    thread::spawn(move || {
        // Only auto-spawn aiosd when connecting locally
        if !is_remote {
            ensure_local_daemon_running();
        }

        let mut attempts = 0u32;
        loop {
            match TcpStream::connect(&daemon_addr) {
                Ok(mut stream) => {
                    // Auth handshake
                    if let Some(token) = read_auth_token(is_remote) {
                        let _ = stream.write_all(format!("AUTH {}\n", token).as_bytes());
                    }

                    let stream_clone = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => break,
                    };

                    // Initial state request
                    let _ = stream.write_all(b"{\"command\":\"GetState\"}\n");

                    // Notify TUI that daemon is now connected
                    tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        sender: "IPC".into(),
                        content: format!("Connected to aiosd @ {daemon_addr}"),
                    }))
                    .ok();

                    // Reader thread
                    let tx_read = tx.clone();
                    let reader_handle = thread::spawn(move || {
                        let reader = BufReader::new(stream_clone);
                        for line in reader.lines().map_while(|r| r.ok()) {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                                dispatch_event(&tx_read, &v);
                            }
                        }
                    });

                    // Writer loop — blocks until the channel is closed or the stream drops
                    while let Ok(cmd) = rx_daemon_local.recv() {
                        if stream.write_all(cmd.as_bytes()).is_err()
                            || stream.write_all(b"\n").is_err()
                        {
                            break;
                        }
                    }

                    // Stream dropped — wait for reader to finish then retry
                    let _ = reader_handle.join();

                    tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        sender: "IPC".into(),
                        content: "Daemon connection lost — retrying...".into(),
                    }))
                    .ok();

                    attempts = 0; // reset on reconnect
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= MAX_RETRIES {
                        // Give up silently — user can restart aiosd manually
                        tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            sender: "IPC".into(),
                            content: format!(
                                "aiosd@{daemon_addr} not reachable after {MAX_RETRIES} attempts — offline mode"
                            ),
                        }))
                        .ok();
                        break;
                    }
                }
            }

            thread::sleep(RETRY_INTERVAL);
        }
    });

    Some(tx_daemon_local)
}

fn dispatch_event(tx: &Sender<BgMsg>, v: &serde_json::Value) {
    match v["event"].as_str() {
        Some("FileChanged") => {
            if let Some(p) = v["path"].as_str() {
                tx.send(BgMsg::FileChanged(PathBuf::from(p))).ok();
            }
        }
        Some("SearchResults") => {
            if let Ok(r) = serde_json::from_value::<Vec<SearchResult>>(v["results"].clone()) {
                tx.send(BgMsg::SearchResults(r)).ok();
            }
        }
        Some("HealthReport") => {
            if let Ok(r) =
                serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["report"].clone())
            {
                tx.send(BgMsg::HealthReport(r)).ok();
            }
        }
        Some("ActivePorts") => {
            if let Ok(p) = serde_json::from_value::<Vec<u16>>(v["ports"].clone()) {
                tx.send(BgMsg::ActivePorts(p)).ok();
            }
        }
        Some("StateSync") => {
            let projects = serde_json::from_value::<Vec<crate::entities::EntityProject>>(
                v["projects"].clone(),
            )
            .unwrap_or_default();
            let health_reports = serde_json::from_value::<Vec<crate::health::ProjectHealth>>(
                v["health_reports"].clone(),
            )
            .unwrap_or_default();
            let active_agents = serde_json::from_value::<Vec<crate::daemon::proxy::AgentProcess>>(
                v["active_agents"].clone(),
            )
            .unwrap_or_default();
            let index_ready = v["index_ready"].as_bool().unwrap_or(false);
            let handover_count = v["handover_count"].as_u64().unwrap_or(0) as u32;
            let pending_file_changes = serde_json::from_value::<
                Vec<crate::daemon::state::FileChangeApproval>,
            >(v["pending_file_changes"].clone())
            .unwrap_or_default();
            let sentinel_files = serde_json::from_value::<
                Vec<crate::daemon::state::SentinelFileStatus>,
            >(v["sentinel_files"].clone())
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

            tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                sender: "IPC".into(),
                content: format!("Synced state from daemon (Reports: {})", report_count),
            }))
            .ok();
        }
        Some("HumanApprovalRequired") => {
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
        Some("FileChangeRequested") => {
            if let Ok(approval) = serde_json::from_value::<crate::daemon::state::FileChangeApproval>(
                v["approval"].clone(),
            ) {
                tx.send(BgMsg::FileChangeRequested { approval }).ok();
            }
        }
        Some("HandoverApproved") => {
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
        Some("HumanApprovalResult") => {
            if let Some(status) = v["status"].as_str() {
                tx.send(BgMsg::HumanApprovalResult {
                    status: status.into(),
                })
                .ok();
            }
        }
        Some("NewLog") => {
            if let Ok(log) = serde_json::from_value::<crate::app::state::LogEntry>(v["log"].clone())
            {
                tx.send(BgMsg::NewLog(log)).ok();
            }
        }
        Some("AgentStarted") => {
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
        Some("AgentStopped") => {
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
        Some("HealthDelta") => {
            if let Ok(r) =
                serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["report"].clone())
            {
                tx.send(BgMsg::HealthDelta(r)).ok();
            }
        }
        Some("UmaiBlocked") => {
            if let Some(reason) = v["reason"].as_str() {
                tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                    sender: "UMAI".into(),
                    content: format!("Blocked: {}", reason),
                }))
                .ok();
            }
        }
        _ => {}
    }
}
