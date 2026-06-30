use std::process::Command;

use super::PortEntry;
use crate::core::process::command_exists;

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
    if let Some(entries) = list_ports_via_ss() {
        return entries;
    }

    if let Some(entries) = list_ports_via_lsof() {
        return entries;
    }

    Vec::new()
}

fn list_ports_via_ss() -> Option<Vec<PortEntry>> {
    if !command_exists("ss") {
        return None;
    }

    let out = Command::new("ss").args(["-tlnp"]).output().ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let Some(port) = cols[3].split(':').next_back().and_then(|p| p.parse().ok()) else {
            continue;
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
    Some(entries)
}

fn list_ports_via_lsof() -> Option<Vec<PortEntry>> {
    if !command_exists("lsof") {
        return None;
    }

    let out = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    let mut pid: Option<u32> = None;
    let mut name: Option<String> = None;

    for line in stdout.lines() {
        if let Some(raw) = line.strip_prefix('p') {
            pid = raw.parse().ok();
            name = None;
        } else if let Some(raw) = line.strip_prefix('c') {
            name = Some(raw.to_string());
        } else if let Some(raw) = line.strip_prefix('n') {
            let Some(port) = parse_port_from_endpoint(raw) else {
                continue;
            };
            entries.push(PortEntry {
                port,
                pid,
                process_name: name.clone(),
                state: "LISTEN".into(),
                protocol: "TCP".into(),
            });
        }
    }

    entries.sort_by_key(|e| e.port);
    entries.dedup_by_key(|e| e.port);
    Some(entries)
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

fn parse_port_from_endpoint(endpoint: &str) -> Option<u16> {
    endpoint
        .split(':')
        .next_back()
        .and_then(|p| p.split_whitespace().next())
        .and_then(|p| p.parse().ok())
}

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
    fn parse_ss_process_extracts_pid() {
        let (pid, name) = parse_ss_process("users:((\"node\",pid=1234,fd=22))");
        assert_eq!(pid, Some(1234));
        assert_eq!(name.as_deref(), Some("node"));
    }

    #[test]
    fn parse_lsof_output_extracts_pid_and_name() {
        let sample = "p4321\ncpython3\nnTCP 127.0.0.1:8000 (LISTEN)\n";
        let mut parsed = Vec::new();
        let mut pid: Option<u32> = None;
        let mut name: Option<String> = None;

        for line in sample.lines() {
            if let Some(raw) = line.strip_prefix('p') {
                pid = raw.parse().ok();
                name = None;
            } else if let Some(raw) = line.strip_prefix('c') {
                name = Some(raw.to_string());
            } else if let Some(raw) = line.strip_prefix('n') {
                let port = parse_port_from_endpoint(raw).unwrap();
                parsed.push((port, pid, name.clone()));
            }
        }

        assert_eq!(
            parsed,
            vec![(8000, Some(4321), Some("python3".to_string()))]
        );
    }
}
