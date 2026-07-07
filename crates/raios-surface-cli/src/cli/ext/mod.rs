use std::path::Path;

mod discover;
mod run;
mod schedule;

use discover::{discover_all_extensions, discover_and_register_extensions, find_extension_path};
use run::{env_append, run_python, service_action, service_status, tail_logs};

// ── Manifest types ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Debug)]
struct ExtensionManifest {
    extension: ExtensionMeta,
    #[serde(default)]
    commands: Vec<ExtCommand>,
    // Parsed so extension.toml manifests declaring a config_schema validate
    // correctly, but nothing consumes it yet — there's no settings-prompt
    // UI that would ask a user for these values.
    #[serde(default)]
    #[allow(dead_code)]
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

fn default_tail() -> usize {
    50
}

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
    #[allow(dead_code)]
    key: String,
    #[allow(dead_code)]
    label: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    field_type: String,
    #[serde(default)]
    #[allow(dead_code)]
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
        println!(
            "  {} v{}  —  {}",
            m.extension.name, m.extension.version, m.extension.description
        );
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
