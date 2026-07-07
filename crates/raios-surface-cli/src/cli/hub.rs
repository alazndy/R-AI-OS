use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const DAEMON_PORT: u16 = 42069;
const MCP_PORT: u16 = 42070;
const HTTP_PORT: u16 = 42071;

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
}

fn pid_path() -> PathBuf {
    config_dir().join("aiosd.pid")
}

fn log_path() -> PathBuf {
    config_dir().join("aiosd.log")
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    // Sending signal 0 checks if the process exists without killing it
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn is_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

fn probe_port(port: u16) -> bool {
    // Try localhost first, then Tailscale IP if configured
    let addrs: &[&str] = &["127.0.0.1", "0.0.0.0"];

    // Check localhost
    if TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(200),
    ).is_ok() {
        return true;
    }

    // Try Tailscale IP from policy
    if let Some(ip) = raios_runtime::server::http::detect_tailscale_ip() {
        if TcpStream::connect_timeout(
            &format!("{ip}:{port}").parse().unwrap(),
            Duration::from_millis(200),
        ).is_ok() {
            return true;
        }
    }

    let _ = addrs; // suppress warning
    false
}

// ─── start ────────────────────────────────────────────────────────────────────

pub fn cmd_start(json: bool) {
    // If the daemon TCP port is already responding, hub is running regardless of PID file
    if probe_port(DAEMON_PORT) {
        let pid = read_pid().filter(|&p| is_alive(p));
        if json {
            println!(
                "{{\"status\":\"already_running\",\"pid\":{}}}",
                pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into())
            );
        } else {
            match pid {
                Some(p) => println!("  Hub already running  (PID {p})"),
                None => {
                    println!("  Hub already running  (orphaned — no PID file)");
                    println!("  To adopt it: find its PID with `raios ps` or `ss -tlnp | grep 42069`");
                }
            }
        }
        return;
    }

    if let Some(pid) = read_pid() {
        if is_alive(pid) {
            if json {
                println!("{{\"status\":\"already_running\",\"pid\":{pid}}}");
            } else {
                println!("  Hub already running  (PID {pid})");
            }
            return;
        }
        // Stale PID file — remove it
        let _ = fs::remove_file(pid_path());
    }

    let config_d = config_dir();
    let _ = fs::create_dir_all(&config_d);

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path());

    let (stdout, stderr) = match log_file.and_then(|f| f.try_clone().map(|f2| (f, f2))) {
        Ok((f, f2)) => (Stdio::from(f), Stdio::from(f2)),
        Err(_) => (Stdio::null(), Stdio::null()),
    };

    let aiosd = which::which("aiosd").unwrap_or_else(|_| PathBuf::from("aiosd"));

    match Command::new(&aiosd)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            if let Err(e) = fs::write(pid_path(), pid.to_string()) {
                eprintln!("  Warning: could not write PID file: {e}");
            }
            // Detach — don't wait()
            std::mem::forget(child);

            if json {
                println!("{{\"status\":\"started\",\"pid\":{pid},\"log\":\"{}\"}}",
                    log_path().display());
            } else {
                println!("  Hub started  PID {pid}");
                println!("  Log  {}", log_path().display());
                println!("  Ports  :{DAEMON_PORT} (IPC)  :{MCP_PORT} (MCP)  :{HTTP_PORT} (HTTP)");
            }
        }
        Err(e) => {
            eprintln!("  Failed to start aiosd: {e}");
            std::process::exit(1);
        }
    }
}

// ─── stop ─────────────────────────────────────────────────────────────────────

pub fn cmd_stop(json: bool) {
    // Resolve which PID to kill: prefer PID file, fall back to port scan
    let pid = resolve_running_pid();

    let Some(pid) = pid else {
        let _ = fs::remove_file(pid_path());
        if json {
            println!("{{\"status\":\"not_running\"}}");
        } else {
            println!("  Hub is not running");
        }
        return;
    };

    if !json {
        println!("  Stopping PID {pid}…");
    }

    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };

        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if !is_alive(pid) {
                break;
            }
        }
    }

    if is_alive(pid) {
        // Unix: SIGTERM above didn't finish the job — escalate to a hard kill.
        // Windows: there's no graceful signal to send in the first place, so
        // this is the only kill attempt.
        let _ = raios_core::core::process::kill_pid(pid);
    }

    let _ = fs::remove_file(pid_path());

    if json {
        println!("{{\"status\":\"stopped\",\"pid\":{pid}}}");
    } else {
        println!("  Hub stopped  (was PID {pid})");
    }
}

fn resolve_running_pid() -> Option<u32> {
    // First: PID file
    if let Some(p) = read_pid() {
        if is_alive(p) {
            return Some(p);
        }
    }
    // Second: find aiosd process by port via /proc
    find_pid_by_port(DAEMON_PORT)
}

/// Scan /proc/*/net/tcp to find which PID owns a given listen port.
fn find_pid_by_port(port: u16) -> Option<u32> {
    let hex_port = format!("{:04X}", port);

    // Find the inode listening on the port from /proc/net/tcp
    let tcp = fs::read_to_string("/proc/net/tcp").ok()?;
    let inode = tcp.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            return None;
        }
        let local = cols[1];
        let state = cols[3];
        let inode = cols[9];
        // state 0A = LISTEN
        let port_hex = local.split(':').nth(1)?;
        if port_hex == hex_port && state == "0A" {
            inode.parse::<u64>().ok()
        } else {
            None
        }
    })?;

    // Scan /proc/<pid>/fd/* for a socket with that inode
    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return None,
    };
    let target_link = format!("socket:[{inode}]");
    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
            continue; // skip non-numeric entries like /proc/self
        };
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = fs::read_dir(&fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(link) = fs::read_link(fd.path()) {
                if link.to_string_lossy() == target_link {
                    return Some(pid);
                }
            }
        }
    }
    None
}

// ─── status ───────────────────────────────────────────────────────────────────

pub fn cmd_status(json: bool) {
    let ipc_up = probe_port(DAEMON_PORT);
    let mcp_up = probe_port(MCP_PORT);
    let http_up = probe_port(HTTP_PORT);

    let mut pid = read_pid();
    let running = match pid {
        Some(p) if is_alive(p) => true,
        Some(_) => {
            // Stale PID file — clear it
            let _ = fs::remove_file(pid_path());
            pid = None;
            ipc_up
        }
        None => ipc_up,
    };

    if json {
        println!(
            "{{\"running\":{running},\"pid\":{},\"ports\":{{\"ipc\":{ipc_up},\"mcp\":{mcp_up},\"http\":{http_up}}}}}",
            pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into())
        );
        return;
    }

    let bullet = |up: bool| if up { "\x1b[32m●\x1b[0m" } else { "\x1b[90m○\x1b[0m" };
    let state = if running { "\x1b[32mrunning\x1b[0m" } else { "\x1b[31mstopped\x1b[0m" };

    println!();
    println!("  R-AI-OS Hub  —  {state}");
    if let Some(p) = pid {
        println!("  PID      {p}");
    }
    println!("  Log      {}", log_path().display());
    println!();
    println!("  {} :{DAEMON_PORT}  Daemon IPC", bullet(ipc_up));
    println!("  {} :{MCP_PORT}  MCP-over-TCP", bullet(mcp_up));
    println!("  {} :{HTTP_PORT}  HTTP API", bullet(http_up));
    println!();

    if running && pid.is_none() {
        println!("  Note: running via orphaned process (no PID file).");
        println!("  Tip: raios hub stop  will kill by port, or check `ss -tlnp | grep 42069`");
    }
}

// ─── install ──────────────────────────────────────────────────────────────────

pub fn cmd_install(enable: bool, json: bool) {
    let aiosd_bin = which::which("aiosd")
        .unwrap_or_else(|_| PathBuf::from("/home/alaz/.cargo/bin/aiosd"));

    let user = std::env::var("USER").unwrap_or_else(|_| "alaz".into());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home").join(&user));

    let service = format!(
        "[Unit]
Description=R-AI-OS Hub (aiosd — Tri-Protocol Kernel)
After=network.target

[Service]
Type=simple
ExecStart={aiosd}
Restart=on-failure
RestartSec=5
StandardOutput=append:{log}
StandardError=append:{log}
Environment=HOME={home}

[Install]
WantedBy=default.target
",
        aiosd = aiosd_bin.display(),
        log = log_path().display(),
        home = home.display(),
    );

    let service_dir = home.join(".config/systemd/user");
    let _ = fs::create_dir_all(&service_dir);
    let service_path = service_dir.join("raios-hub.service");

    if let Err(e) = fs::write(&service_path, &service) {
        eprintln!("  Failed to write service file: {e}");
        std::process::exit(1);
    }

    if json {
        println!("{{\"status\":\"installed\",\"path\":\"{}\"}}", service_path.display());
    } else {
        println!("  Wrote  {}", service_path.display());
    }

    if enable {
        let reload = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let enabled = Command::new("systemctl")
            .args(["--user", "enable", "--now", "raios-hub.service"])
            .status();

        match (reload, enabled) {
            (Ok(_), Ok(s)) if s.success() => {
                if !json {
                    println!("  Enabled & started via systemd");
                    println!("  Run: systemctl --user status raios-hub");
                }
            }
            _ => {
                eprintln!("  systemctl enable failed — run manually:");
                eprintln!("    systemctl --user daemon-reload");
                eprintln!("    systemctl --user enable --now raios-hub.service");
            }
        }
    } else if !json {
        println!();
        println!("  To enable at boot:");
        println!("    systemctl --user daemon-reload");
        println!("    systemctl --user enable --now raios-hub.service");
    }
}

// ─── logs ─────────────────────────────────────────────────────────────────────

pub fn cmd_logs(lines: usize) {
    let path = log_path();

    if !path.exists() {
        eprintln!("  No log file yet: {}", path.display());
        eprintln!("  Start the hub first: raios hub start");
        std::process::exit(1);
    }

    // Print last N lines, then follow
    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("  Cannot open log: {e}");
            std::process::exit(1);
        }
    };

    let reader = BufReader::new(file);
    let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("{line}");
    }

    // Follow mode — stream new lines until Ctrl-C
    println!("\x1b[90m--- following {} (Ctrl-C to exit) ---\x1b[0m", path.display());
    let mut tail = Command::new("tail")
        .args(["-f", "-n", "0"])
        .arg(&path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::null())
        .spawn()
        .expect("tail not found");
    let _ = tail.wait();
}

// ─── api-key ──────────────────────────────────────────────────────────────────

fn api_key_file() -> PathBuf {
    config_dir().join(".hub_api_key")
}

fn policy_path() -> Option<PathBuf> {
    // Standard raios-policy.toml locations
    let candidates = [
        dirs::config_dir()?.join("raios/raios-policy.toml"),
        std::env::current_dir().ok()?.join("raios-policy.toml"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Masks a secret for display: first/last 4 chars visible, middle collapsed.
/// Short secrets (<=8 chars) are fully redacted to avoid leaking most of the value.
fn mask_secret(secret: &str) -> String {
    let len = secret.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    format!("{}…{}", &secret[..4], &secret[len - 4..])
}

pub fn cmd_api_key_show(reveal: bool) {
    let path = api_key_file();
    match fs::read_to_string(&path) {
        Ok(key) => {
            let key = key.trim();
            println!("\n  Hub API Key (remote bearer token):\n");
            if reveal {
                println!("  {}\n", key);
            } else {
                println!("  {}", mask_secret(key));
                println!("  (pass --reveal to print the full key; avoid recorded/logged terminals)\n");
            }
            println!("  Usage:  Authorization: Bearer <key>");
            println!("  Stored: {}\n", path.display());
        }
        Err(_) => {
            eprintln!("  No API key yet. Generate one with: raios hub api-key generate");
            std::process::exit(1);
        }
    }
}

pub fn cmd_api_key_generate(force: bool) {
    let path = api_key_file();
    if path.exists() && !force {
        eprintln!("  API key already exists. Use --force to rotate it.");
        eprintln!("  Current key: raios hub api-key show");
        std::process::exit(1);
    }

    let _ = fs::create_dir_all(config_dir());

    // Generate 32-byte cryptographically strong key from the OS CSPRNG
    let key = raios_core::security::generate_secret_hex();

    // Save key plaintext (chmod 600)
    if let Err(e) = fs::write(&path, &key) {
        eprintln!("  Failed to write key: {e}");
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    // Compute hash for policy.toml
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    let key_hash = format!("{:x}", h.finalize());

    println!("\n  New API key generated.\n");
    println!("  Key:  {key}");
    println!("  ⚠ This is the only time the full key is printed. It is saved to {}", path.display());
    println!("    Retrieve it later (masked) with: raios hub api-key show");
    println!("  Hash: {key_hash}\n");

    // Write hash to policy.toml if it exists
    if let Some(pol_path) = policy_path() {
        let content = fs::read_to_string(&pol_path).unwrap_or_default();
        let new_line = format!("api_key_hash = \"{key_hash}\"");

        let updated = if content.contains("api_key_hash") {
            let re = regex::Regex::new(r#"api_key_hash\s*=\s*"[^"]*""#).unwrap();
            re.replace(&content, new_line.as_str()).into_owned()
        } else if content.contains("[server.hub]") {
            content.replace("[server.hub]", &format!("[server.hub]\n{new_line}"))
        } else if content.contains("[server]") {
            // Append hub section after [server]
            let insert = format!("\n[server.hub]\n{new_line}\n");
            content.replacen("[server]", &format!("[server]{insert}"), 1)
        } else {
            format!("{content}\n[server]\n[server.hub]\n{new_line}\n")
        };

        if let Err(e) = fs::write(&pol_path, updated) {
            eprintln!("  Warning: could not update policy.toml: {e}");
        } else {
            println!("  Hash written to {}", pol_path.display());
        }
    } else {
        println!("  Add this to your raios-policy.toml:\n");
        println!("  [server.hub]");
        println!("  bind_mode = \"tailscale\"");
        println!("  api_key_hash = \"{key_hash}\"\n");
    }
}

// ─── libc shim (only needs kill + SIGTERM; unix-only, see is_alive/cmd_stop) ─

#[cfg(unix)]
mod libc {
    extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
    pub const SIGTERM: i32 = 15;
}

#[cfg(test)]
mod tests {
    use super::mask_secret;

    #[test]
    fn masks_long_secret_keeping_first_and_last_four() {
        let key = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let masked = mask_secret(key);
        assert!(masked.starts_with("a1b2"));
        assert!(masked.ends_with("a1b2"));
        assert!(!masked.contains(&key[10..20]), "middle of secret must not leak");
    }

    #[test]
    fn fully_redacts_short_secrets() {
        assert_eq!(mask_secret("short"), "*****");
        assert_eq!(mask_secret(""), "");
    }
}
