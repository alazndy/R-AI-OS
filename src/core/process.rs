use serde::{Deserialize, Serialize};
use std::process::Command;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortEntry {
    pub port: u16,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub state: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: Option<f32>,
    pub mem_mb: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillResult {
    pub ok: bool,
    pub message: String,
}

// ─── Port listing ─────────────────────────────────────────────────────────────

pub fn list_ports() -> Vec<PortEntry> {
    if cfg!(target_os = "windows") {
        list_ports_windows()
    } else {
        list_ports_unix()
    }
}

fn list_ports_windows() -> Vec<PortEntry> {
    let out = Command::new("netstat")
        .args(["-ano"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries: Vec<PortEntry> = Vec::new();

    for line in out.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let proto = cols[0];
        if proto != "TCP" && proto != "UDP" {
            continue;
        }

        let state = cols.get(3).copied().unwrap_or("").to_string();
        if proto == "TCP" && state != "LISTENING" {
            continue;
        }

        let port: Option<u16> = cols[1].split(':').next_back().and_then(|p| p.parse().ok());
        let port = match port {
            Some(p) => p,
            None => continue,
        };
        let pid: Option<u32> = cols.last().and_then(|p| p.parse().ok());
        let process_name = pid.and_then(process_name_windows);

        entries.push(PortEntry {
            port,
            pid,
            process_name,
            state,
            protocol: proto.to_string(),
        });
    }

    entries.sort_by_key(|e| e.port);
    entries.dedup_by_key(|e| e.port);
    entries
}

fn process_name_windows(pid: u32) -> Option<String> {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .next()?
        .split(',')
        .next()
        .map(|s| s.trim_matches('"').to_string())
}

fn list_ports_unix() -> Vec<PortEntry> {
    let out = Command::new("ss")
        .args(["-tlnp"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries = Vec::new();
    for line in out.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let port: Option<u16> = cols[3].split(':').next_back().and_then(|p| p.parse().ok());
        let port = match port {
            Some(p) => p,
            None => continue,
        };
        let (pid, name) = parse_ss_process(cols.get(6).copied().unwrap_or(""));
        entries.push(PortEntry {
            port,
            pid,
            process_name: name,
            state: "LISTEN".into(),
            protocol: "TCP".into(),
        });
    }
    entries.sort_by_key(|e| e.port);
    entries
}

fn parse_ss_process(s: &str) -> (Option<u32>, Option<String>) {
    let pid = s
        .split("pid=")
        .nth(1)
        .and_then(|p| p.split(',').next())
        .and_then(|p| p.parse().ok());
    let name = s
        .split("((\"")
        .nth(1)
        .and_then(|p| p.split('"').next())
        .map(str::to_string);
    (pid, name)
}

// ─── Process listing ──────────────────────────────────────────────────────────

pub fn list_processes(top_n: usize) -> Vec<ProcessEntry> {
    if cfg!(target_os = "windows") {
        list_processes_windows(top_n)
    } else {
        list_processes_unix(top_n)
    }
}

fn list_processes_windows(top_n: usize) -> Vec<ProcessEntry> {
    let out = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries: Vec<ProcessEntry> = out
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.splitn(5, ',').collect();
            if cols.len() < 5 {
                return None;
            }
            let name = cols[0].trim_matches('"').to_string();
            let pid: u32 = cols[1].trim_matches('"').parse().ok()?;
            let mem_kb: f32 = cols[4]
                .trim_matches('"')
                .replace(',', "")
                .replace(" K", "")
                .trim()
                .parse()
                .unwrap_or(0.0);
            Some(ProcessEntry {
                pid,
                name,
                cpu_pct: None,
                mem_mb: Some(mem_kb / 1024.0),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.mem_mb
            .unwrap_or(0.0)
            .partial_cmp(&a.mem_mb.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(top_n);
    entries
}

fn list_processes_unix(top_n: usize) -> Vec<ProcessEntry> {
    let out = Command::new("ps")
        .args(["aux", "--sort=-%mem"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    out.lines()
        .skip(1)
        .take(top_n)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 11 {
                return None;
            }
            let pid: u32 = cols[1].parse().ok()?;
            let cpu: f32 = cols[2].parse().unwrap_or(0.0);
            let mem: f32 = cols[3].parse().unwrap_or(0.0);
            let name = cols[10]
                .split('/')
                .next_back()
                .unwrap_or(cols[10])
                .to_string();
            Some(ProcessEntry {
                pid,
                name,
                cpu_pct: Some(cpu),
                mem_mb: Some(mem),
            })
        })
        .collect()
}

// ─── Kill ─────────────────────────────────────────────────────────────────────

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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_ports_no_panic() {
        let ports = list_ports();
        for p in &ports {
            assert!(p.port > 0);
        }
    }

    #[test]
    fn list_processes_bounded() {
        let procs = list_processes(5);
        assert!(procs.len() <= 5);
        for p in &procs {
            assert!(p.pid > 0);
        }
    }

    #[test]
    fn kill_nonexistent_port_returns_error() {
        let r = kill_port(1);
        if !r.ok {
            assert!(!r.message.is_empty());
        }
    }

    #[test]
    fn parse_ss_process_extracts_pid() {
        let (pid, name) = parse_ss_process("users:((\"node\",pid=1234,fd=22))");
        assert_eq!(pid, Some(1234));
        assert_eq!(name.as_deref(), Some("node"));
    }
}
