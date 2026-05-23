use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseDep {
    pub name: String,
    pub version: String,
    pub license: String,
    pub is_copyleft: bool,
    pub is_unknown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReport {
    pub project_path: PathBuf,
    pub deps: Vec<LicenseDep>,
    pub copyleft_count: usize,
    pub unknown_count: usize,
    pub total: usize,
}

pub fn scan_licenses(path: &Path) -> LicenseReport {
    let mut deps = Vec::new();

    if path.join("Cargo.lock").exists() {
        deps.extend(scan_cargo_lock(path));
    } else if path.join("package.json").exists() {
        deps.extend(scan_package_json(path));
    }

    let copyleft_count = deps.iter().filter(|d| d.is_copyleft).count();
    let unknown_count = deps.iter().filter(|d| d.is_unknown).count();
    let total = deps.len();

    LicenseReport { project_path: path.to_path_buf(), deps, copyleft_count, unknown_count, total }
}

pub fn is_copyleft(license: &str) -> bool {
    let l = license.to_uppercase();
    l.contains("GPL") || l.contains("AGPL") || l.contains("LGPL")
}

pub fn is_unknown(license: &str) -> bool {
    let l = license.trim().to_uppercase();
    l.is_empty() || l == "UNKNOWN" || l == "UNLICENSED"
}

fn scan_cargo_lock(path: &Path) -> Vec<LicenseDep> {
    let content = match std::fs::read_to_string(path.join("Cargo.lock")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if !name.is_empty() {
                let license = lookup_cargo_license(path, &name, &version);
                deps.push(make_dep(name.clone(), version.clone(), license));
            }
            name = String::new();
            version = String::new();
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name = ") {
            name = rest.trim_matches('"').to_string();
        } else if let Some(rest) = trimmed.strip_prefix("version = ") {
            version = rest.trim_matches('"').to_string();
        }
    }
    if !name.is_empty() {
        let license = lookup_cargo_license(path, &name, &version);
        deps.push(make_dep(name, version, license));
    }
    deps
}

fn lookup_cargo_license(project_path: &Path, name: &str, version: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let registry_root = home.join(".cargo").join("registry").join("src");
        if let Ok(entries) = std::fs::read_dir(&registry_root) {
            for entry in entries.flatten() {
                let crate_dir = entry.path().join(format!("{}-{}", name, version));
                let manifest = crate_dir.join("Cargo.toml");
                if let Ok(content) = std::fs::read_to_string(&manifest) {
                    for line in content.lines() {
                        if let Some(rest) = line.strip_prefix("license") {
                            let val = rest.trim_start_matches(&[' ', '='][..]);
                            let cleaned = val.trim().trim_matches('"').to_string();
                            if !cleaned.is_empty() {
                                return cleaned;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Ok(content) = std::fs::read_to_string(project_path.join("Cargo.toml")) {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("license = ") {
                return rest.trim().trim_matches('"').to_string();
            }
        }
    }

    String::from("UNKNOWN")
}

fn scan_package_json(path: &Path) -> Vec<LicenseDep> {
    let content = match std::fs::read_to_string(path.join("package.json")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();
    if let Some(dependencies) = json["dependencies"].as_object() {
        for (dep_name, version_val) in dependencies {
            let version = version_val.as_str().unwrap_or("*").to_string();
            let license = lookup_node_license(path, dep_name).unwrap_or_else(|| "UNKNOWN".into());
            deps.push(make_dep(dep_name.clone(), version, license));
        }
    }
    deps
}

fn lookup_node_license(path: &Path, name: &str) -> Option<String> {
    let pkg_json = path.join("node_modules").join(name).join("package.json");
    let content = std::fs::read_to_string(&pkg_json).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json["license"].as_str().map(|s| s.to_string())
}

fn make_dep(name: String, version: String, license: String) -> LicenseDep {
    let copyleft = is_copyleft(&license);
    let unknown = is_unknown(&license);
    LicenseDep { name, version, license, is_copyleft: copyleft, is_unknown: unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_licenses_parses_cargo_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo_lock = "# This file is automatically @generated by Cargo.\nversion = 3\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"abc123\"\n";
        let cargo_toml = "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1.0.0\"\n";
        fs::write(tmp.path().join("Cargo.lock"), cargo_lock).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), cargo_toml).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find serde in Cargo.lock");
    }

    #[test]
    fn scan_licenses_parses_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = r#"{"name":"my-app","version":"1.0.0","license":"MIT","dependencies":{"react":"18.0.0"}}"#;
        fs::write(tmp.path().join("package.json"), pkg).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find react in package.json");
    }

    #[test]
    fn is_copyleft_identifies_gpl() {
        assert!(is_copyleft("GPL-3.0"));
        assert!(is_copyleft("GPL-2.0-only"));
        assert!(is_copyleft("AGPL-3.0"));
        assert!(is_copyleft("LGPL-2.1"));
        assert!(!is_copyleft("MIT"));
        assert!(!is_copyleft("Apache-2.0"));
        assert!(!is_copyleft("BSD-3-Clause"));
    }

    #[test]
    fn is_unknown_license_identifies_blanks() {
        assert!(is_unknown(""));
        assert!(is_unknown("UNKNOWN"));
        assert!(is_unknown("UNLICENSED"));
        assert!(!is_unknown("MIT"));
        assert!(!is_unknown("GPL-3.0"));
    }
}
