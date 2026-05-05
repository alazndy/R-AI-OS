use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::{RwLock, broadcast};
use tokio::net::TcpListener;
use tokio::io::AsyncWriteExt;
use notify::{Watcher, RecursiveMode};
use r_ai_os::config::Config;
use super::state::DaemonState;

pub struct Server {
    state: Arc<RwLock<DaemonState>>,
}

impl Server {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
        Self { state }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        println!("Server is listening on 127.0.0.1:42069...");
        let listener = TcpListener::bind("127.0.0.1:42069").await?;
        
        let (tx, _) = broadcast::channel::<String>(100);
        
        // Start file watcher
        let config = Config::load().unwrap_or_else(|| Config {
            dev_ops_path: PathBuf::from(""),
            master_md_path: PathBuf::from(""),
            skills_path: PathBuf::from(""),
            vault_projects_path: PathBuf::from(""),
        });

        let watcher_tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let msg = format!("{{\"event\":\"FileChanged\",\"path\":\"{}\"}}", path.display().to_string().replace("\\", "\\\\"));
                    let _ = watcher_tx.send(msg);
                }
            }
        }).ok();

        if let Some(ref mut w) = watcher {
            w.watch(&config.dev_ops_path, RecursiveMode::Recursive).ok();
            // We need to keep `watcher` alive, so we'll move it into a blocked thread or just keep it in scope.
            // Since this function runs forever, keeping it in the scope is fine.
        }

        // Port Monitor Task
        let port_tx = tx.clone();
        tokio::spawn(async move {
            let common_ports = [3000, 5173, 8080, 4200];
            loop {
                let mut active = Vec::new();
                for &port in &common_ports {
                    let addr = format!("127.0.0.1:{}", port);
                    if tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        tokio::net::TcpStream::connect(&addr)
                    ).await.is_ok() {
                        active.push(port);
                    }
                }
                let msg = format!("{{\"event\":\"ActivePorts\",\"ports\":{}}}", serde_json::to_string(&active).unwrap());
                let _ = port_tx.send(msg);
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });

        loop {
            let (mut socket, addr) = listener.accept().await?;
            println!("Client connected: {}", addr);
            
            let mut rx = tx.subscribe();
            let state_for_client = self.state.clone();
            let tx_sender = tx.clone();
            tokio::spawn(async move {
                use tokio::io::{BufReader, AsyncBufReadExt};
                let (reader, mut writer) = socket.split();
                let mut reader = BufReader::new(reader);
                let mut line = String::new();

                loop {
                    tokio::select! {
                        // Read from socket
                        res = reader.read_line(&mut line) => {
                            if res.unwrap_or(0) == 0 { break; }
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                                if v["command"] == "Search" {
                                    if let Some(query) = v["query"].as_str() {
                                        let s = state_for_client.read().await;
                                        if let Some(ref idx) = s.index {
                                            let results = idx.search(query);
                                            let response = format!("{{\"event\":\"SearchResults\",\"results\":{}}}\n", serde_json::to_string(&results).unwrap());
                                            let _ = writer.write_all(response.as_bytes()).await;
                                        }
                                    }
                                } else if v["command"] == "HealthScan" {
                                    let s = state_for_client.read().await;
                                    let projects = s.projects.clone();
                                    drop(s); // Release lock before heavy work
                                    
                                    let tx_clone = tx_sender.clone();
                                    tokio::task::spawn_blocking(move || {
                                        let report: Vec<r_ai_os::health::ProjectHealth> =
                                            projects.iter().map(|p| r_ai_os::health::check_project(p)).collect();
                                        let response = format!("{{\"event\":\"HealthReport\",\"report\":{}}}", serde_json::to_string(&report).unwrap());
                                        let _ = tx_clone.send(response);
                                    });
                                }
                            }
                            line.clear();
                        }

                        // Read from broadcast
                        msg_res = rx.recv() => {
                            if let Ok(msg) = msg_res {
                                let mut payload = msg.clone();
                                payload.push('\n');
                                if writer.write_all(payload.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }
    }
}

