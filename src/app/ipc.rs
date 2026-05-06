use std::net::TcpStream;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::app::state::BgMsg;
use crate::indexer::SearchResult;

const DAEMON_ADDR: &str = "127.0.0.1:42069";
const RETRY_INTERVAL: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 10;

pub fn connect_daemon(tx: Sender<BgMsg>) -> Option<Sender<String>> {
    let (tx_daemon_local, rx_daemon_local) = mpsc::channel::<String>();

    thread::spawn(move || {
        // Give the daemon a moment if it was just launched
        thread::sleep(Duration::from_millis(600));

        let mut attempts = 0u32;

        loop {
            match TcpStream::connect(DAEMON_ADDR) {
                Ok(mut stream) => {
                    // Auth handshake
                    if let Some(config_dir) = crate::config::Config::config_file().parent() {
                        let token_path = config_dir.join(".ipc_token");
                        if let Ok(token) = std::fs::read_to_string(token_path) {
                            let _ = stream.write_all(format!("AUTH {}\n", token.trim()).as_bytes());
                        }
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
                        content: "Connected to aiosd daemon".into(),
                    })).ok();

                    // Reader thread
                    let tx_read = tx.clone();
                    let reader_handle = thread::spawn(move || {
                        let reader = BufReader::new(stream_clone);
                        for line in reader.lines().flatten() {
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
                    })).ok();

                    attempts = 0; // reset on reconnect
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= MAX_RETRIES {
                        // Give up silently — user can restart aiosd manually
                        tx.send(BgMsg::NewLog(crate::app::state::LogEntry {
                            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                            sender: "IPC".into(),
                            content: format!("aiosd not reachable after {} attempts — offline mode", MAX_RETRIES),
                        })).ok();
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
            if let Ok(r) = serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["report"].clone()) {
                tx.send(BgMsg::HealthReport(r)).ok();
            }
        }
        Some("ActivePorts") => {
            if let Ok(p) = serde_json::from_value::<Vec<u16>>(v["ports"].clone()) {
                tx.send(BgMsg::ActivePorts(p)).ok();
            }
        }
        Some("StateSync") => {
            if let (Ok(projects), Ok(health_reports), Ok(active_agents), Some(index_ready), Some(handover_count), Ok(pending_file_changes)) = (
                serde_json::from_value::<Vec<crate::entities::EntityProject>>(v["projects"].clone()),
                serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["health_reports"].clone()),
                serde_json::from_value::<Vec<crate::daemon::proxy::AgentProcess>>(v["active_agents"].clone()),
                v["index_ready"].as_bool(),
                v["handover_count"].as_u64(),
                serde_json::from_value::<Vec<crate::daemon::state::FileChangeApproval>>(v["pending_file_changes"].clone()),
            ) {
                tx.send(BgMsg::StateSync {
                    projects,
                    health_reports,
                    active_agents,
                    index_ready,
                    handover_count: handover_count as u32,
                    pending_file_changes,
                }).ok();
            }
        }
        Some("HumanApprovalRequired") => {
            if let (Some(target), Some(instruction), Some(reason)) = (
                v["target"].as_str(), v["instruction"].as_str(), v["reason"].as_str(),
            ) {
                tx.send(BgMsg::HumanApprovalRequired {
                    target: target.into(), instruction: instruction.into(), reason: reason.into(),
                }).ok();
            }
        }
        Some("FileChangeRequested") => {
            if let Ok(approval) = serde_json::from_value::<crate::daemon::state::FileChangeApproval>(v["approval"].clone()) {
                tx.send(BgMsg::FileChangeRequested { approval }).ok();
            }
        }
        Some("HandoverApproved") => {
            if let (Some(target), Some(instruction), Some(count)) = (
                v["target"].as_str(), v["instruction"].as_str(), v["count"].as_u64(),
            ) {
                tx.send(BgMsg::HandoverApproved {
                    target: target.into(), instruction: instruction.into(), count: count as u32,
                }).ok();
            }
        }
        Some("HumanApprovalResult") => {
            if let Some(status) = v["status"].as_str() {
                tx.send(BgMsg::HumanApprovalResult { status: status.into() }).ok();
            }
        }
        _ => {}
    }
}
