use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

pub(crate) const LOCAL_DAEMON_ADDR: &str = "127.0.0.1:42069";
pub(crate) const RETRY_INTERVAL: Duration = Duration::from_secs(8);
pub(crate) const MAX_RETRIES: u32 = 10;

pub(crate) fn resolve_daemon_addr(remote_host: Option<&str>) -> (String, bool) {
    match remote_host {
        Some(host) if host.contains(':') => (host.to_string(), true),
        Some(host) => (format!("{host}:42069"), true),
        None => (LOCAL_DAEMON_ADDR.to_string(), false),
    }
}

pub(crate) fn ensure_local_daemon_running() {
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

pub(crate) fn read_auth_token(is_remote: bool) -> Option<String> {
    let config_dir = dirs::config_dir()?.join("raios");
    let filename = if is_remote {
        ".hub_api_key"
    } else {
        ".session_token"
    };
    std::fs::read_to_string(config_dir.join(filename))
        .ok()
        .map(|s| s.trim().to_owned())
}

pub(crate) fn write_auth_handshake(stream: &mut TcpStream, is_remote: bool) {
    if let Some(token) = read_auth_token(is_remote) {
        let _ = stream.write_all(format!("AUTH {}\n", token).as_bytes());
    }
}

pub(crate) fn initial_state_request(stream: &mut TcpStream) {
    let _ = stream.write_all(b"{\"command\":\"GetState\"}\n");
}

pub(crate) fn log_entry(
    sender: &str,
    content: impl Into<String>,
) -> raios_surface_tui::app::state::LogEntry {
    raios_surface_tui::app::state::LogEntry {
        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        sender: sender.into(),
        content: content.into(),
    }
}
