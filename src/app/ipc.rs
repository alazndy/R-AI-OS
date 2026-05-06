use std::net::TcpStream;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::app::state::BgMsg;
use crate::indexer::SearchResult;

pub fn connect_daemon(tx: Sender<BgMsg>) -> Option<Sender<String>> {
    let (tx_daemon_local, rx_daemon_local) = mpsc::channel::<String>();
    
    thread::spawn(move || {
        // Wait a bit for the daemon to be ready if starting together
        thread::sleep(Duration::from_millis(500));

        if let Ok(mut stream) = TcpStream::connect("127.0.0.1:42069") {
            // Send token first for auth
            if let Some(config_file) = crate::config::Config::config_file().parent() {
                let token_path = config_file.join(".ipc_token");
                if let Ok(token) = std::fs::read_to_string(token_path) {
                    let auth_msg = format!("AUTH {}\n", token.trim());
                    let _ = stream.write_all(auth_msg.as_bytes());
                }
            }

            let stream_clone = stream.try_clone().unwrap();
            let reader = BufReader::new(stream_clone);
            
            // Trigger initial sync
            let _ = stream.write_all(b"{\"command\":\"GetState\"}\n");
            
            // Spawn a thread to read from daemon
            let tx_read = tx.clone();
            thread::spawn(move || {
                for line in reader.lines().flatten() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        if v["event"] == "FileChanged" {
                            if let Some(path_str) = v["path"].as_str() {
                                tx_read.send(BgMsg::FileChanged(PathBuf::from(path_str))).ok();
                            }
                        } else if v["event"] == "SearchResults" {
                            if let Ok(results) = serde_json::from_value::<Vec<SearchResult>>(v["results"].clone()) {
                                tx_read.send(BgMsg::SearchResults(results)).ok();
                            }
                        } else if v["event"] == "HealthReport" {
                            if let Ok(report) = serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["report"].clone()) {
                                tx_read.send(BgMsg::HealthReport(report)).ok();
                            }
                        } else if v["event"] == "ActivePorts" {
                            if let Ok(ports) = serde_json::from_value::<Vec<u16>>(v["ports"].clone()) {
                                tx_read.send(BgMsg::ActivePorts(ports)).ok();
                            }
                        } else if v["event"] == "StateSync" {
                            if let (Ok(projects), Ok(health_reports), Ok(active_agents), Some(index_ready), Some(handover_count), Ok(pending_file_changes)) = (
                                serde_json::from_value::<Vec<crate::entities::EntityProject>>(v["projects"].clone()),
                                serde_json::from_value::<Vec<crate::health::ProjectHealth>>(v["health_reports"].clone()),
                                serde_json::from_value::<Vec<crate::daemon::proxy::AgentProcess>>(v["active_agents"].clone()),
                                v["index_ready"].as_bool(),
                                v["handover_count"].as_u64(),
                                serde_json::from_value::<Vec<crate::daemon::state::FileChangeApproval>>(v["pending_file_changes"].clone())
                            ) {
                                tx_read.send(BgMsg::StateSync { projects, health_reports, active_agents, index_ready, handover_count: handover_count as u32, pending_file_changes }).ok();
                            }
                        } else if v["event"] == "HumanApprovalRequired" {
                            if let (Some(target), Some(instruction), Some(reason)) = (v["target"].as_str(), v["instruction"].as_str(), v["reason"].as_str()) {
                                tx_read.send(BgMsg::HumanApprovalRequired { target: target.to_string(), instruction: instruction.to_string(), reason: reason.to_string() }).ok();
                            }
                        } else if v["event"] == "FileChangeRequested" {
                            if let Ok(approval) = serde_json::from_value::<crate::daemon::state::FileChangeApproval>(v["approval"].clone()) {
                                tx_read.send(BgMsg::FileChangeRequested { approval }).ok();
                            }
                        } else if v["event"] == "HandoverApproved" {
                            if let (Some(target), Some(instruction), Some(count)) = (v["target"].as_str(), v["instruction"].as_str(), v["count"].as_u64()) {
                                tx_read.send(BgMsg::HandoverApproved { target: target.to_string(), instruction: instruction.to_string(), count: count as u32 }).ok();
                            }
                        } else if v["event"] == "HumanApprovalResult" {
                            if let Some(status) = v["status"].as_str() {
                                tx_read.send(BgMsg::HumanApprovalResult { status: status.to_string() }).ok();
                            }
                        }
                    }
                }
            });

            // Read from UI and send to daemon
            while let Ok(cmd) = rx_daemon_local.recv() {
                let _ = stream.write_all(cmd.as_bytes());
                let _ = stream.write_all(b"\n");
            }
        } else {
            println!("Warning: Could not connect to aiosd daemon on 127.0.0.1:42069");
        }
    });

    Some(tx_daemon_local)
}
