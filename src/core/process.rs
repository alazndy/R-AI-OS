use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

pub fn command_exists(command: &str) -> bool {
    resolve_command_path(command).is_some()
}

pub fn resolve_command_path(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists().then(|| path.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;

    #[cfg(target_os = "windows")]
    let exts: Vec<String> = std::env::var_os("PATHEXT")
        .map(|v| {
            v.to_string_lossy()
                .split(';')
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".BAT".into(), ".CMD".into()]);

    #[cfg(not(target_os = "windows"))]
    let exts: Vec<String> = vec![String::new()];

    std::env::split_paths(&path_var).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{command}{ext}")))
            .find(|candidate| candidate.exists())
    })
}

pub fn python_command() -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        if command_exists("py") {
            return ("py".to_string(), vec!["-3".to_string()]);
        }
        ("python".to_string(), Vec::new())
    }

    #[cfg(not(target_os = "windows"))]
    {
        if command_exists("python3") {
            return ("python3".to_string(), Vec::new());
        }
        ("python".to_string(), Vec::new())
    }
}

pub fn shell_command(command: &str) -> (String, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "cmd".to_string(),
            vec!["/C".to_string(), command.to_string()],
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        (
            "sh".to_string(),
            vec!["-lc".to_string(), command.to_string()],
        )
    }
}

pub fn open_in_system_editor(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/c", "start", "", &path.to_string_lossy()])
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }
}

pub fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "windows")]
    let mut child = match Command::new("clip.exe").stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };

    #[cfg(target_os = "macos")]
    let mut child = match Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut child = {
        let candidates: [(&str, &[&str]); 3] = [
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];

        let mut spawned = None;
        for (program, args) in candidates {
            if let Ok(child) = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .spawn()
            {
                spawned = Some(child);
                break;
            }
        }

        match spawned {
            Some(child) => child,
            None => return false,
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok()
}

pub fn launch_in_terminal(command: &str, work_dir: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let work_dir = work_dir.to_string_lossy().into_owned();
        if Command::new("wt")
            .args(["-d", &work_dir, "--", "cmd", "/K", command])
            .spawn()
            .is_ok()
        {
            return true;
        }

        let cmd_str = format!("cd /d \"{}\" && {}", work_dir, command);
        return Command::new("cmd")
            .args(["/c", "start", "cmd", "/k", &cmd_str])
            .spawn()
            .is_ok();
    }

    #[cfg(target_os = "macos")]
    {
        let escaped_dir = escape_shell_arg(&work_dir.to_string_lossy());
        let escaped_cmd = escape_applescript(&format!("cd {} && {}", escaped_dir, command));
        return Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "tell application \"Terminal\" to do script \"{}\"",
                    escaped_cmd
                ),
            ])
            .spawn()
            .is_ok();
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let shell_cmd = format!(
            "cd {} && {}; exec sh",
            escape_shell_arg(&work_dir.to_string_lossy()),
            command
        );

        let candidates: [(&str, Vec<String>); 4] = [
            (
                "x-terminal-emulator",
                vec!["-e".into(), "sh".into(), "-lc".into(), shell_cmd.clone()],
            ),
            (
                "gnome-terminal",
                vec![
                    "--working-directory".into(),
                    work_dir.to_string_lossy().into_owned(),
                    "--".into(),
                    "sh".into(),
                    "-lc".into(),
                    shell_cmd.clone(),
                ],
            ),
            (
                "konsole",
                vec![
                    "--workdir".into(),
                    work_dir.to_string_lossy().into_owned(),
                    "-e".into(),
                    "sh".into(),
                    "-lc".into(),
                    shell_cmd.clone(),
                ],
            ),
            (
                "xterm",
                vec!["-e".into(), "sh".into(), "-lc".into(), shell_cmd],
            ),
        ];

        for (program, args) in candidates {
            if Command::new(program).args(&args).spawn().is_ok() {
                return true;
            }
        }

        false
    }
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
        .args(["-Ao", "pid=,pcpu=,rss=,comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut entries: Vec<ProcessEntry> = out
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let pid: u32 = cols.next()?.parse().ok()?;
            let cpu: f32 = cols.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let rss_kb: f32 = cols.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let name = cols.next()?;
            let name = name.split('/').next_back().unwrap_or(name).to_string();
            Some(ProcessEntry {
                pid,
                name,
                cpu_pct: Some(cpu),
                mem_mb: Some(rss_kb / 1024.0),
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

fn escape_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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

    #[test]
    fn python_command_returns_some_program_name() {
        let (program, _args) = python_command();
        assert!(!program.is_empty());
    }
}
