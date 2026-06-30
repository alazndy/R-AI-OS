use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::{BufRead, BufReader};
use rusqlite;

// ── Manifest types ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Debug)]
struct ExtensionManifest {
    extension: ExtensionMeta,
    #[serde(default)]
    commands: Vec<ExtCommand>,
    #[serde(default)]
    config_schema: Vec<ConfigField>,
    #[serde(default)]
    schedules: Vec<ExtSchedule>,
}

#[derive(serde::Deserialize, Debug)]
struct ExtensionMeta {
    name: String,
    version: String,
    description: String,
    interpreter: Option<String>,
    entry: Option<String>,
    services: Option<Vec<String>>,
    log_file: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct ExtCommand {
    name: String,
    kind: CommandKind,
    #[serde(default)]
    args: Vec<String>,
    env_key: Option<String>,
    separator: Option<String>,
    #[serde(default = "default_tail")]
    tail: usize,
    #[serde(default)]
    description: String,
}

fn default_tail() -> usize { 50 }

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum CommandKind {
    Run,
    ServiceStart,
    ServiceStop,
    ServiceStatus,
    Logs,
    EnvAppend,
}

#[derive(serde::Deserialize, Debug)]
struct ConfigField {
    key: String,
    label: String,
    #[serde(rename = "type")]
    field_type: String,
    #[serde(default)]
    description: String,
}

#[derive(serde::Deserialize, Debug)]
struct ExtSchedule {
    name: String,
    /// Standard 5-field cron expression (e.g. "0 2 * * *")
    cron: String,
    /// Must match a [[commands]] name in this extension
    command: String,
    #[serde(default)]
    description: String,
}

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Find an extension by name. Searches entities.json first, then scans dev_ops_path subdirs.
fn find_extension_path(name: &str, dev_ops_path: &Path) -> Option<PathBuf> {
    // 1. entities.json lookup
    let entities_path = dev_ops_path.join("entities.json");
    if let Ok(raw) = std::fs::read_to_string(&entities_path) {
        if let Ok(arr) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(list) = arr.as_array() {
                for item in list {
                    let item_name = item["name"].as_str().unwrap_or("");
                    if item_name.to_lowercase().contains(&name.to_lowercase()) {
                        if let Some(p) = item["local_path"].as_str() {
                            let ext_toml = PathBuf::from(p).join("raios-extension.toml");
                            if ext_toml.exists() {
                                return Some(PathBuf::from(p));
                            }
                        }
                    }
                }
            }
        }
    }
    // 2. Scan dev_ops_path category subdirs for raios-extension.toml
    let categories = ["ai", "web", "tools", "embedded", "core"];
    for cat in categories {
        let cat_path = dev_ops_path.join(cat);
        if let Ok(entries) = std::fs::read_dir(&cat_path) {
            for entry in entries.flatten() {
                let proj = entry.path();
                let toml_path = proj.join("raios-extension.toml");
                if toml_path.exists() {
                    if let Ok(raw) = std::fs::read_to_string(&toml_path) {
                        if let Ok(m) = toml::from_str::<ExtensionManifest>(&raw) {
                            if m.extension.name.to_lowercase() == name.to_lowercase() {
                                return Some(proj);
                            }
                        }
                    }
                }
            }
        }
    }
    // 3. Also scan dev_ops_path root level
    if let Ok(entries) = std::fs::read_dir(dev_ops_path) {
        for entry in entries.flatten() {
            let proj = entry.path();
            let toml_path = proj.join("raios-extension.toml");
            if toml_path.exists() {
                if let Ok(raw) = std::fs::read_to_string(&toml_path) {
                    if let Ok(m) = toml::from_str::<ExtensionManifest>(&raw) {
                        if m.extension.name.to_lowercase() == name.to_lowercase() {
                            return Some(proj);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Collect all extensions from dev_ops_path (for `raios ext list`).
fn discover_all_extensions(dev_ops_path: &Path) -> Vec<(PathBuf, ExtensionManifest)> {
    let mut result = Vec::new();
    let categories = ["ai", "web", "tools", "embedded", "core", ""];
    for cat in categories {
        let search_path = if cat.is_empty() {
            dev_ops_path.to_path_buf()
        } else {
            dev_ops_path.join(cat)
        };
        if let Ok(entries) = std::fs::read_dir(&search_path) {
            for entry in entries.flatten() {
                let proj = entry.path();
                let toml_path = proj.join("raios-extension.toml");
                if toml_path.exists() {
                    if let Ok(raw) = std::fs::read_to_string(&toml_path) {
                        if let Ok(m) = toml::from_str::<ExtensionManifest>(&raw) {
                            result.push((proj, m));
                        }
                    }
                }
            }
        }
    }
    result
}

// ── Public entry points ───────────────────────────────────────────────────────

pub fn cmd_ext_list(dev_ops_path: &Path, json: bool) {
    let extensions = discover_all_extensions(dev_ops_path);
    if json {
        let out: Vec<serde_json::Value> = extensions
            .iter()
            .map(|(path, m)| {
                serde_json::json!({
                    "name": m.extension.name,
                    "version": m.extension.version,
                    "description": m.extension.description,
                    "path": path.display().to_string(),
                    "commands": m.commands.iter().map(|c| &c.name).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }
    if extensions.is_empty() {
        println!("No raios extensions found under {}", dev_ops_path.display());
        println!("Add a raios-extension.toml to any project to register it.");
        return;
    }
    println!();
    println!("  Registered Extensions");
    println!("  ─────────────────────────────────────────────");
    for (path, m) in &extensions {
        println!("  {} v{}  —  {}", m.extension.name, m.extension.version, m.extension.description);
        println!("    Path:     {}", path.display());
        let cmd_names: Vec<_> = m.commands.iter().map(|c| c.name.as_str()).collect();
        println!("    Commands: {}", cmd_names.join("  "));
        println!();
    }
}

pub fn cmd_ext(name: &str, ext_args: &[String], dev_ops_path: &Path, json: bool) {
    if name == "list" {
        cmd_ext_list(dev_ops_path, json);
        return;
    }
    if name == "install" {
        println!("\n  Installing raios extensions...\n");
        let registered = discover_and_register_extensions(dev_ops_path);
        println!("\n  {} extension(s) registered.", registered.len());
        return;
    }

    let ext_path = match find_extension_path(name, dev_ops_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "Extension '{}' not found. Run: raios ext list  to see available extensions.",
                name
            );
            std::process::exit(1);
        }
    };

    let toml_path = ext_path.join("raios-extension.toml");
    let raw = match std::fs::read_to_string(&toml_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", toml_path.display(), e);
            std::process::exit(1);
        }
    };
    let manifest: ExtensionManifest = match toml::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("raios-extension.toml parse error: {}", e);
            std::process::exit(1);
        }
    };

    let subcmd = ext_args.first().map(|s| s.as_str()).unwrap_or("status");
    let subcmd_args: Vec<&str> = ext_args.iter().skip(1).map(|s| s.as_str()).collect();

    // Find the matching command definition
    let cmd_def = manifest.commands.iter().find(|c| c.name == subcmd);

    match cmd_def {
        None => {
            println!();
            println!("  {} v{}", manifest.extension.name, manifest.extension.version);
            println!("  {}", manifest.extension.description);
            println!();
            println!("  Commands:");
            for c in &manifest.commands {
                println!("    {:<12} {}", c.name, c.description);
            }
            println!();
        }
        Some(def) => match def.kind {
            CommandKind::Run => run_python(&ext_path, &manifest, &def.args, &subcmd_args),
            CommandKind::ServiceStart => service_action(&manifest, "start"),
            CommandKind::ServiceStop => service_action(&manifest, "stop"),
            CommandKind::ServiceStatus => service_status(&ext_path, &manifest),
            CommandKind::Logs => tail_logs(&ext_path, &manifest, def.tail),
            CommandKind::EnvAppend => {
                if subcmd_args.is_empty() {
                    eprintln!("Usage: raios ext {} {} <value>", name, subcmd);
                    std::process::exit(1);
                }
                let key = def.env_key.as_deref().unwrap_or("");
                let sep = def.separator.as_deref().unwrap_or(", ");
                let value = subcmd_args.join(" ");
                env_append(&ext_path, key, &value, sep);
            }
        },
    }
}

// ── Command implementations ───────────────────────────────────────────────────

fn resolve_python(ext_path: &Path, manifest: &ExtensionManifest) -> PathBuf {
    let rel = manifest.extension.interpreter.as_deref().unwrap_or("venv/bin/python");
    let candidate = ext_path.join(rel);
    if candidate.exists() {
        return candidate;
    }
    // Fallback: system python3
    PathBuf::from("python3")
}

fn run_python(ext_path: &Path, manifest: &ExtensionManifest, cmd_args: &[String], extra: &[&str]) {
    let python = resolve_python(ext_path, manifest);
    let entry = manifest.extension.entry.as_deref().unwrap_or("main.py");

    let mut full_args: Vec<&str> = vec![entry];
    for a in cmd_args {
        full_args.push(a.as_str());
    }
    for a in extra {
        full_args.push(a);
    }

    let status = Command::new(&python)
        .args(&full_args)
        .current_dir(ext_path)
        .status();

    match status {
        Ok(s) if !s.success() => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to run {}: {}", python.display(), e);
            std::process::exit(1);
        }
        Ok(_) => {}
    }
}

fn service_action(manifest: &ExtensionManifest, action: &str) {
    let services = match &manifest.extension.services {
        Some(s) => s.clone(),
        None => {
            eprintln!("No services defined in raios-extension.toml");
            return;
        }
    };
    for svc in &services {
        let out = Command::new("systemctl").args([action, svc]).output();
        match out {
            Ok(o) if o.status.success() => println!("  ✓ systemctl {} {}", action, svc),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("  ✗ systemctl {} {}: {}", action, svc, err.trim());
            }
            Err(e) => eprintln!("  ✗ systemctl not available: {}", e),
        }
    }
}

fn service_status(ext_path: &Path, manifest: &ExtensionManifest) {
    println!();
    println!("  {} v{} — status", manifest.extension.name, manifest.extension.version);
    println!("  ─────────────────────────────────────────────");

    // Services
    if let Some(services) = &manifest.extension.services {
        for svc in services {
            let out = Command::new("systemctl")
                .args(["is-active", "--quiet", svc])
                .output();
            let active = out.map(|o| o.status.success()).unwrap_or(false);
            let icon = if active { "●" } else { "○" };
            let state = if active { "running" } else { "stopped" };
            println!("  {} {:20} {}", icon, svc, state);
        }
    }

    // Last report
    let vault = std::env::var("OBSIDIAN_VAULT_PATH")
        .or_else(|_| {
            let env_path = ext_path.join(".env");
            read_env_key(&env_path, "OBSIDIAN_VAULT_PATH")
                .ok_or_else(|| std::env::VarError::NotPresent)
        })
        .unwrap_or_default();

    if !vault.is_empty() {
        let vault_path = Path::new(&vault);
        if let Ok(entries) = std::fs::read_dir(vault_path) {
            let mut reports: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.ends_with("-atc-radar.md") {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            reports.sort();
            if let Some(latest) = reports.last() {
                println!("  Last report:  {}", latest.trim_end_matches("-atc-radar.md"));
            } else {
                println!("  Last report:  none found in vault");
            }
        }
    }

    // AI engine keys
    let api_key_status = |key: &str| -> &'static str {
        if std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false) {
            "configured"
        } else {
            "missing"
        }
    };
    println!("  Gemini:       {}", api_key_status("GEMINI_API_KEY"));
    println!("  Groq:         {}", api_key_status("GROQ_API_KEY"));
    println!();
}

fn tail_logs(ext_path: &Path, manifest: &ExtensionManifest, n: usize) {
    let log_file = match &manifest.extension.log_file {
        Some(f) => ext_path.join(f),
        None => {
            eprintln!("No log_file defined in raios-extension.toml");
            return;
        }
    };
    if !log_file.exists() {
        println!("Log file not found: {}", log_file.display());
        return;
    }
    let file = match std::fs::File::open(&log_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Cannot open {}: {}", log_file.display(), e);
            return;
        }
    };
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();
    let start = lines.len().saturating_sub(n);
    println!("── {} (last {} lines) ──", log_file.display(), n);
    for line in &lines[start..] {
        println!("{}", line);
    }
}

fn env_append(ext_path: &Path, key: &str, value: &str, separator: &str) {
    let env_path = ext_path.join(".env");
    let current = read_env_key(&env_path, key).unwrap_or_default();
    let new_val = if current.is_empty() {
        value.to_string()
    } else {
        // Avoid duplicates
        let mut parts: Vec<&str> = current.split(',').map(|s| s.trim()).collect();
        if !parts.contains(&value.trim()) {
            parts.push(value.trim());
        }
        parts.join(separator)
    };
    match write_env_key(&env_path, key, &new_val) {
        Ok(_) => println!("  ✓ {}={}", key, new_val),
        Err(e) => eprintln!("  ✗ Failed to update .env: {}", e),
    }
}

// ── .env helpers ──────────────────────────────────────────────────────────────

fn read_env_key(env_path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(env_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            return Some(v.to_string());
        }
    }
    None
}

fn write_env_key(env_path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let content = std::fs::read_to_string(env_path).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if !line.starts_with('#') && line.contains('=') {
                if let Some((k, _)) = line.split_once('=') {
                    if k.trim() == key {
                        found = true;
                        return format!("{}={}", key, value);
                    }
                }
            }
            line.to_string()
        })
        .collect();
    if !found {
        lines.push(format!("{}={}", key, value));
    }
    std::fs::write(env_path, lines.join("\n") + "\n")
}

// ── Setup wizard auto-discovery ───────────────────────────────────────────────

/// Called by setup wizard to find and register all extensions.
/// Returns list of (extension_name, path) that were discovered.
pub fn discover_and_register_extensions(dev_ops_path: &Path) -> Vec<(String, PathBuf)> {
    let extensions = discover_all_extensions(dev_ops_path);
    let mut registered = Vec::new();

    for (path, manifest) in extensions {
        let name = manifest.extension.name.clone();
        println!("  Found extension: {} v{}", name, manifest.extension.version);

        // Install Python deps if venv + requirements.txt exist
        let req_path = path.join("requirements.txt");
        let interp = manifest.extension.interpreter.as_deref().unwrap_or("venv/bin/python");
        let venv_path = path.join(interp.split('/').next().unwrap_or("venv"));

        if req_path.exists() {
            if !venv_path.exists() {
                print!("    Creating venv...");
                let _ = Command::new("python3")
                    .args(["-m", "venv", "venv"])
                    .current_dir(&path)
                    .status();
                println!(" done");
            }
            let pip = path.join("venv/bin/pip");
            if pip.exists() {
                print!("    Installing dependencies...");
                let ok = Command::new(&pip)
                    .args(["install", "-r", "requirements.txt", "-q"])
                    .current_dir(&path)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                println!(" {}", if ok { "done" } else { "failed (check manually)" });
            }
        }
        // Register schedules into cp_scheduled_jobs
        if !manifest.schedules.is_empty() {
            register_extension_schedules(&name, &path, &manifest.schedules);
        }

        registered.push((name, path));
    }
    registered
}

// ── Schedule registration ─────────────────────────────────────────────────────

/// Converts a 5-field cron string to an approximate repeat interval in seconds.
/// Only handles the common daily/weekly/hourly patterns used by extensions.
fn cron_to_interval_secs(cron: &str) -> u64 {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return 86400; // fallback: daily
    }
    let dow = fields[4]; // day-of-week
    let hour = fields[1];
    let minute = fields[0];

    // Weekly: day-of-week is specific (not *)
    if dow != "*" {
        return 604800;
    }
    // Daily: hour is specific
    if hour != "*" {
        return 86400;
    }
    // Hourly: minute is specific
    if minute != "*" {
        return 3600;
    }
    // Every minute (catch-all)
    60
}

/// Computes the next UTC run time string from a cron expression.
/// Approximates by computing "next occurrence of hour:minute today, or tomorrow".
fn cron_next_run(cron: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let fields: Vec<&str> = cron.split_whitespace().collect();
    let hour: u64 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let minute: u64 = fields.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Seconds since midnight (UTC)
    let day_secs = now_secs % 86400;
    let target_day_secs = hour * 3600 + minute * 60;

    let next_secs = if target_day_secs > day_secs {
        now_secs - day_secs + target_day_secs
    } else {
        now_secs - day_secs + 86400 + target_day_secs
    };

    // Format as ISO-8601 (rough, without chrono dependency here)
    let s = next_secs;
    let days = s / 86400;
    let rem = s % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let sec = rem % 60;
    // Unix epoch was 1970-01-01. We'll just store a readable approx.
    let _ = (days, h, m, sec); // used below via chrono if available

    // Use chrono for proper formatting (already a dependency in Cargo.toml)
    chrono_next_run_str(next_secs)
}

fn chrono_next_run_str(unix_secs: u64) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(unix_secs as i64, 0)
        .unwrap_or_else(chrono::Utc::now);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn register_extension_schedules(ext_name: &str, ext_path: &Path, schedules: &[ExtSchedule]) {
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
        .join("workspace.db");

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("    Could not open raios DB to register schedules: {}", e);
            return;
        }
    };

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    for sched in schedules {
        let id = format!("ext-{}-{}", ext_name, sched.name);
        let interval = cron_to_interval_secs(&sched.cron);
        let next_run = cron_next_run(&sched.cron);
        // Shell command the daemon will execute
        let task_desc = format!("raios ext {} {}", ext_name, sched.command);
        let title = if sched.description.is_empty() {
            format!("[ext:{}] {}", ext_name, sched.command)
        } else {
            sched.description.clone()
        };

        let result = conn.execute(
            "INSERT INTO cp_scheduled_jobs
             (id, title, agent, task_description, interval_secs, status, next_run_at, created_at)
             VALUES (?1,?2,?3,?4,?5,'active',?6,?7)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title,
               task_description=excluded.task_description,
               interval_secs=excluded.interval_secs,
               next_run_at=excluded.next_run_at",
            rusqlite::params![id, title, ext_name, task_desc, interval as i64, next_run, now],
        );
        match result {
            Ok(_) => println!("    ✓ Schedule registered: {} ({}s)", sched.name, interval),
            Err(e) => eprintln!("    ✗ Failed to register schedule {}: {}", sched.name, e),
        }
    }
}
