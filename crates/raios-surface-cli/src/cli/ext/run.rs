use super::ExtensionManifest;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn resolve_python(ext_path: &Path, manifest: &ExtensionManifest) -> PathBuf {
    let rel = manifest
        .extension
        .interpreter
        .as_deref()
        .unwrap_or("venv/bin/python");
    let candidate = ext_path.join(rel);
    if candidate.exists() {
        return candidate;
    }
    PathBuf::from("python3")
}

pub(super) fn run_python(
    ext_path: &Path,
    manifest: &ExtensionManifest,
    cmd_args: &[String],
    extra: &[&str],
) {
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

pub(super) fn service_action(manifest: &ExtensionManifest, action: &str) {
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

pub(super) fn service_status(ext_path: &Path, manifest: &ExtensionManifest) {
    println!();
    println!(
        "  {} v{} — status",
        manifest.extension.name, manifest.extension.version
    );
    println!("  ─────────────────────────────────────────────");

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

    let vault = std::env::var("OBSIDIAN_VAULT_PATH")
        .or_else(|_| {
            let env_path = ext_path.join(".env");
            read_env_key(&env_path, "OBSIDIAN_VAULT_PATH").ok_or(std::env::VarError::NotPresent)
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
                println!(
                    "  Last report:  {}",
                    latest.trim_end_matches("-atc-radar.md")
                );
            } else {
                println!("  Last report:  none found in vault");
            }
        }
    }

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

pub(super) fn tail_logs(ext_path: &Path, manifest: &ExtensionManifest, n: usize) {
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

pub(super) fn env_append(ext_path: &Path, key: &str, value: &str, separator: &str) {
    let env_path = ext_path.join(".env");
    let current = read_env_key(&env_path, key).unwrap_or_default();
    let new_val = if current.is_empty() {
        value.to_string()
    } else {
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
