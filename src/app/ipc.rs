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
            let stream_clone = stream.try_clone().unwrap();
            let reader = BufReader::new(stream_clone);
            
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
