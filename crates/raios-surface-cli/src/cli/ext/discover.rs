use super::ExtensionManifest;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn find_extension_path(name: &str, dev_ops_path: &Path) -> Option<PathBuf> {
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

pub(super) fn discover_all_extensions(dev_ops_path: &Path) -> Vec<(PathBuf, ExtensionManifest)> {
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

pub(super) fn discover_and_register_extensions(dev_ops_path: &Path) -> Vec<(String, PathBuf)> {
    let extensions = discover_all_extensions(dev_ops_path);
    let mut registered = Vec::new();

    for (path, manifest) in extensions {
        let name = manifest.extension.name.clone();
        println!(
            "  Found extension: {} v{}",
            name, manifest.extension.version
        );

        let req_path = path.join("requirements.txt");
        let interp = manifest
            .extension
            .interpreter
            .as_deref()
            .unwrap_or("venv/bin/python");
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
                println!(
                    " {}",
                    if ok {
                        "done"
                    } else {
                        "failed (check manually)"
                    }
                );
            }
        }
        if !manifest.schedules.is_empty() {
            super::schedule::register_extension_schedules(&name, &path, &manifest.schedules);
        }

        registered.push((name, path));
    }
    registered
}
