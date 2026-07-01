use std::process::Command;

use super::KillResult;
use raios_core::core::process::list_ports;

pub fn kill_port(port: u16) -> KillResult {
    match list_ports().iter().find(|e| e.port == port) {
        None => KillResult {
            ok: false,
            message: format!("No process listening on port {}", port),
        },
        Some(e) => match e.pid {
            None => KillResult {
                ok: false,
                message: format!("Port {} found but PID unknown", port),
            },
            Some(pid) => kill_pid(pid),
        },
    }
}

pub fn kill_pid(pid: u32) -> KillResult {
    let out = if cfg!(target_os = "windows") {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
    } else {
        Command::new("kill").args(["-9", &pid.to_string()]).output()
    };

    match out {
        Ok(o) if o.status.success() => KillResult {
            ok: true,
            message: format!("Process {} terminated", pid),
        },
        Ok(o) => KillResult {
            ok: false,
            message: String::from_utf8_lossy(&o.stderr).trim().to_string(),
        },
        Err(e) => KillResult {
            ok: false,
            message: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_nonexistent_port_returns_error() {
        let r = kill_port(1);
        if !r.ok {
            assert!(!r.message.is_empty());
        }
    }
}
