use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Sender};
use std::thread;

use raios_surface_tui::app::state::BgMsg;

/// Connect to the local daemon (localhost).
pub fn connect_daemon(tx: Sender<BgMsg>) -> Option<Sender<String>> {
    connect_daemon_addr(tx, None)
}

/// Connect to daemon at an explicit address.
/// Pass `Some("100.x.x.x")` for remote Tailscale Hub access.
pub fn connect_daemon_addr(
    tx: Sender<BgMsg>,
    remote_host: Option<String>,
) -> Option<Sender<String>> {
    let (daemon_addr, is_remote) =
        raios_surface_tui::app::ipc_support::resolve_daemon_addr(remote_host.as_deref());

    let (tx_daemon_local, rx_daemon_local) = mpsc::channel::<String>();

    thread::spawn(move || {
        // Only auto-spawn aiosd when connecting locally
        if !is_remote {
            raios_surface_tui::app::ipc_support::ensure_local_daemon_running();
        }

        let mut attempts = 0u32;
        loop {
            match std::net::TcpStream::connect(&daemon_addr) {
                Ok(mut stream) => {
                    raios_surface_tui::app::ipc_support::write_auth_handshake(
                        &mut stream,
                        is_remote,
                    );

                    let stream_clone = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => break,
                    };

                    raios_surface_tui::app::ipc_support::initial_state_request(&mut stream);

                    // Notify TUI that daemon is now connected
                    tx.send(BgMsg::NewLog(
                        raios_surface_tui::app::ipc_support::log_entry(
                            "IPC",
                            format!("Connected to aiosd @ {daemon_addr}"),
                        ),
                    ))
                    .ok();

                    // Reader thread
                    let tx_read = tx.clone();
                    let reader_handle = thread::spawn(move || {
                        let reader = BufReader::new(stream_clone);
                        for line in reader.lines().map_while(|r| r.ok()) {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                                raios_surface_tui::app::ipc_events::dispatch_event(&tx_read, &v);
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

                    tx.send(BgMsg::NewLog(
                        raios_surface_tui::app::ipc_support::log_entry(
                            "IPC",
                            "Daemon connection lost — retrying...",
                        ),
                    ))
                    .ok();

                    attempts = 0; // reset on reconnect
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= raios_surface_tui::app::ipc_support::MAX_RETRIES {
                        // Give up silently — user can restart aiosd manually
                        tx.send(BgMsg::NewLog(raios_surface_tui::app::ipc_support::log_entry(
                            "IPC",
                            format!(
                                "aiosd@{daemon_addr} not reachable after {} attempts — offline mode",
                                raios_surface_tui::app::ipc_support::MAX_RETRIES
                            ),
                        )))
                        .ok();
                        break;
                    }
                }
            }

            thread::sleep(raios_surface_tui::app::ipc_support::RETRY_INTERVAL);
        }
    });

    Some(tx_daemon_local)
}
