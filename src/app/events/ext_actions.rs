use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use crate::app::{
    state::{BgMsg, ExtCmdInfo, ExtConfigField, ExtensionInfo},
    App,
};

impl App {
    /// Trigger async extension discovery + .env value loading.
    pub fn load_extensions(&mut self) {
        let tx = self.tx.clone();
        let dev_ops = self.config.dev_ops_path.clone();
        thread::spawn(move || {
            let exts = scan_extensions(&dev_ops);
            tx.send(BgMsg::ExtensionsLoaded(exts)).ok();
        });
    }

    /// Run the currently highlighted extension command.
    pub fn run_ext_cmd(&mut self) {
        let ext = match self.ext.extensions.get(self.ext.ext_cursor) {
            Some(e) => e.clone(),
            None => return,
        };
        let cmd = match ext.commands.get(self.ext.cmd_cursor) {
            Some(c) => c.clone(),
            None => return,
        };
        self.ext.status = Some(format!("▶ {} {}…", ext.name, cmd.name));
        self.add_activity("Ext", &format!("Running {} {}", ext.name, cmd.name), "Info");

        let tx = self.tx.clone();
        let ext_name = ext.name.clone();
        let cmd_name = cmd.name.clone();
        let ext_path = ext.path.clone();

        // Read the manifest to get python interpreter + entry + args
        let toml_path = ext_path.join("raios-extension.toml");
        thread::spawn(move || {
            run_ext_command_bg(&tx, &ext_path, &toml_path, &ext_name, &cmd_name);
        });
    }

    /// Save the current config field edit to .env.
    pub fn save_ext_config_field(&mut self) {
        let ext = match self.ext.extensions.get_mut(self.ext.ext_cursor) {
            Some(e) => e,
            None => return,
        };
        let field = match ext.config_schema.get_mut(self.ext.cfg_cursor) {
            Some(f) => f,
            None => return,
        };
        let new_val = self.ext.input.clone();
        let env_path = ext.path.join(".env");
        match write_env_key(&env_path, &field.key, &new_val) {
            Ok(_) => {
                field.value = if field.masked && !new_val.is_empty() {
                    new_val.clone()
                } else {
                    new_val.clone()
                };
                self.ext.status = Some(format!("✓ {} saved", field.label));
            }
            Err(e) => {
                self.ext.status = Some(format!("✗ Save failed: {}", e));
            }
        }
        self.ext.editing = false;
        self.ext.input.clear();
    }
}

// ── Background command runner ──────────────────────────────────────────────────

fn run_ext_command_bg(
    tx: &std::sync::mpsc::Sender<BgMsg>,
    ext_path: &Path,
    toml_path: &Path,
    ext_name: &str,
    cmd_name: &str,
) {
    #[derive(serde::Deserialize)]
    struct Manifest {
        extension: Meta,
        #[serde(default)]
        commands: Vec<Cmd>,
    }
    #[derive(serde::Deserialize)]
    struct Meta {
        interpreter: Option<String>,
        entry: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Cmd {
        name: String,
        kind: String,
        #[serde(default)]
        args: Vec<String>,
        #[allow(dead_code)]
        env_key: Option<String>,
        #[allow(dead_code)]
        separator: Option<String>,
    }

    let raw = match std::fs::read_to_string(toml_path) {
        Ok(s) => s,
        Err(e) => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Cannot read manifest: {}", e),
            })
            .ok();
            return;
        }
    };
    let manifest: Manifest = match toml::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Manifest parse error: {}", e),
            })
            .ok();
            return;
        }
    };

    let def = match manifest.commands.iter().find(|c| c.name == cmd_name) {
        Some(d) => d,
        None => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("✗ Unknown command: {}", cmd_name),
            })
            .ok();
            return;
        }
    };

    match def.kind.as_str() {
        "run" => {
            let rel_interp = manifest.extension.interpreter.as_deref().unwrap_or("venv/bin/python");
            let interpreter = {
                let candidate = ext_path.join(rel_interp);
                if candidate.exists() { candidate } else { PathBuf::from("python3") }
            };
            let entry = manifest.extension.entry.as_deref().unwrap_or("main.py");
            let mut full_args = vec![entry.to_string()];
            full_args.extend_from_slice(&def.args);

            let child = Command::new(&interpreter)
                .args(&full_args)
                .current_dir(ext_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match child {
                Err(e) => {
                    tx.send(BgMsg::ExtCmdOutput {
                        ext: ext_name.into(),
                        cmd: cmd_name.into(),
                        line: format!("✗ Spawn failed: {}", e),
                    })
                    .ok();
                }
                Ok(mut child) => {
                    // Stream stdout
                    if let Some(stdout) = child.stdout.take() {
                        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                            tx.send(BgMsg::ExtCmdOutput {
                                ext: ext_name.into(),
                                cmd: cmd_name.into(),
                                line,
                            })
                            .ok();
                        }
                    }
                    let code = child.wait().map(|s| s.code().unwrap_or(0)).unwrap_or(-1);
                    tx.send(BgMsg::ExtCmdOutput {
                        ext: ext_name.into(),
                        cmd: cmd_name.into(),
                        line: format!("✓ exited ({})", code),
                    })
                    .ok();
                }
            }
        }
        "service_start" | "service_stop" | "service_status" => {
            let action = match def.kind.as_str() {
                "service_start" => "start",
                "service_stop" => "stop",
                _ => "status",
            };
            // Services come from manifest meta — re-parse services field
            #[derive(serde::Deserialize)]
            struct MetaFull { services: Option<Vec<String>> }
            #[derive(serde::Deserialize)]
            struct ManifestFull { extension: MetaFull }
            let services: Vec<String> = toml::from_str::<ManifestFull>(&raw)
                .ok()
                .and_then(|m| m.extension.services)
                .unwrap_or_default();
            for svc in &services {
                let out = Command::new("systemctl").args([action, svc]).output();
                let line = match out {
                    Ok(o) if o.status.success() => format!("✓ systemctl {} {}", action, svc),
                    Ok(o) => format!("✗ {}: {}", svc, String::from_utf8_lossy(&o.stderr).trim()),
                    Err(e) => format!("✗ systemctl error: {}", e),
                };
                tx.send(BgMsg::ExtCmdOutput { ext: ext_name.into(), cmd: cmd_name.into(), line }).ok();
            }
        }
        "env_append" => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: "Use: raios ext <name> follow <keyword> from terminal".into(),
            })
            .ok();
        }
        other => {
            tx.send(BgMsg::ExtCmdOutput {
                ext: ext_name.into(),
                cmd: cmd_name.into(),
                line: format!("Unhandled kind '{}' in TUI runner", other),
            })
            .ok();
        }
    }
}

// ── Extension scanner ──────────────────────────────────────────────────────────

fn scan_extensions(dev_ops_path: &Path) -> Vec<ExtensionInfo> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        extension: ManifestMeta,
        #[serde(default)]
        commands: Vec<ManifestCmd>,
        #[serde(default)]
        config_schema: Vec<ManifestCfg>,
    }
    #[derive(serde::Deserialize)]
    struct ManifestMeta {
        name: String,
        version: String,
        description: String,
        services: Option<Vec<String>>,
    }
    #[derive(serde::Deserialize)]
    struct ManifestCmd {
        name: String,
        #[serde(default)]
        description: String,
    }
    #[derive(serde::Deserialize)]
    struct ManifestCfg {
        key: String,
        label: String,
        #[serde(rename = "type")]
        field_type: String,
        #[serde(default)]
        description: String,
    }

    let mut result = Vec::new();
    let categories = ["ai", "web", "tools", "embedded", "core", ""];
    for cat in categories {
        let search = if cat.is_empty() {
            dev_ops_path.to_path_buf()
        } else {
            dev_ops_path.join(cat)
        };
        let Ok(entries) = std::fs::read_dir(&search) else { continue };
        for entry in entries.flatten() {
            let proj = entry.path();
            let toml_path = proj.join("raios-extension.toml");
            if !toml_path.exists() { continue; }
            let Ok(raw) = std::fs::read_to_string(&toml_path) else { continue };
            let Ok(m) = toml::from_str::<Manifest>(&raw) else { continue };
            let env_path = proj.join(".env");

            let config_schema: Vec<ExtConfigField> = m
                .config_schema
                .iter()
                .map(|f| {
                    let value = read_env_key(&env_path, &f.key).unwrap_or_default();
                    ExtConfigField {
                        key: f.key.clone(),
                        label: f.label.clone(),
                        field_type: f.field_type.clone(),
                        description: f.description.clone(),
                        masked: f.field_type == "secret",
                        value,
                    }
                })
                .collect();

            result.push(ExtensionInfo {
                name: m.extension.name.clone(),
                version: m.extension.version.clone(),
                description: m.extension.description.clone(),
                path: proj,
                commands: m.commands.iter().map(|c| ExtCmdInfo { name: c.name.clone(), description: c.description.clone() }).collect(),
                config_schema,
                services: m.extension.services.unwrap_or_default(),
            });
        }
    }
    result
}

// ── .env helpers ──────────────────────────────────────────────────────────────

fn read_env_key(env_path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(env_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') { continue; }
        let (k, v) = line.split_once('=')?;
        if k.trim() == key {
            return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
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
