use std::path::Path;
use std::process::Command;
use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedDep {
    pub name: String,
    pub current: String,
    pub latest: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveIssue {
    pub package: String,
    pub version: String,
    pub severity: String,
    pub description: String,
    pub advisory_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepsReport {
    pub project_type: String,
    pub has_lockfile: bool,
    pub outdated: Vec<OutdatedDep>,
    pub outdated_count: usize,
    pub cve_issues: Vec<CveIssue>,
    pub cve_count: usize,
    pub cve_critical: usize,
    pub tool_missing: Vec<String>,
}

impl DepsReport {
    fn empty(project_type: &str) -> Self {
        Self {
            project_type: project_type.into(),
            has_lockfile: false,
            outdated: vec![],
            outdated_count: 0,
            cve_issues: vec![],
            cve_count: 0,
            cve_critical: 0,
            tool_missing: vec![],
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn check(dir: &Path) -> DepsReport {
    use crate::core::build::detect_type;
    use crate::core::build::ProjectType;

    match detect_type(dir) {
        ProjectType::Rust   => check_rust(dir),
        ProjectType::Node   => check_node(dir),
        ProjectType::Python => check_python(dir),
        ProjectType::Go     => check_go(dir),
        ProjectType::Unknown => {
            let mut r = DepsReport::empty("Unknown");
            r.tool_missing.push("Cannot detect project type".into());
            r
        }
    }
}

// ─── Rust ────────────────────────────────────────────────────────────────────

fn check_rust(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Rust");
    report.has_lockfile = dir.join("Cargo.lock").exists();

    // cargo audit
    let audit_out = Command::new("cargo")
        .args(["audit", "--json"])
        .current_dir(dir)
        .output();

    match audit_out {
        Err(_) => report.tool_missing.push("cargo-audit (install: cargo install cargo-audit)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(vulns) = v["vulnerabilities"]["list"].as_array() {
                    for vuln in vulns {
                        let pkg      = vuln["package"]["name"].as_str().unwrap_or("?").to_string();
                        let version  = vuln["package"]["version"].as_str().unwrap_or("?").to_string();
                        let advisory = vuln["advisory"]["id"].as_str().unwrap_or("?").to_string();
                        let desc     = vuln["advisory"]["title"].as_str().unwrap_or("?").to_string();
                        let severity = cvss_to_severity(
                            vuln["advisory"]["cvss"].as_str().unwrap_or("")
                        ).to_string();

                        if severity == "critical" { report.cve_critical += 1; }
                        report.cve_issues.push(CveIssue { package: pkg, version, severity, description: desc, advisory_id: advisory });
                    }
                    report.cve_count = report.cve_issues.len();
                }
            }
        }
    }

    // cargo outdated
    let outdated_out = Command::new("cargo")
        .args(["outdated", "--format=json"])
        .current_dir(dir)
        .output();

    match outdated_out {
        Err(_) => report.tool_missing.push("cargo-outdated (install: cargo install cargo-outdated)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(deps) = v["dependencies"].as_array() {
                    for dep in deps {
                        let name    = dep["name"].as_str().unwrap_or("?").to_string();
                        let current = dep["project"].as_str().unwrap_or("?").to_string();
                        let latest  = dep["latest"].as_str().unwrap_or("?").to_string();
                        let kind    = dep["kind"].as_str().unwrap_or("direct").to_string();
                        if current != latest && latest != "Removed" {
                            report.outdated.push(OutdatedDep { name, current, latest, kind });
                        }
                    }
                    report.outdated_count = report.outdated.len();
                }
            }
        }
    }

    report
}

// ─── Node ────────────────────────────────────────────────────────────────────

fn check_node(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Node");
    let pm = if dir.join("pnpm-lock.yaml").exists() { "pnpm" }
             else if dir.join("bun.lockb").exists()  { "bun" }
             else { "npm" };
    report.has_lockfile = dir.join("package-lock.json").exists()
        || dir.join("yarn.lock").exists()
        || dir.join("pnpm-lock.yaml").exists()
        || dir.join("bun.lockb").exists();

    // npm/pnpm outdated --json
    let out = Command::new(pm)
        .args(["outdated", "--json"])
        .current_dir(dir)
        .output();

    if let Ok(o) = out {
        let stdout = String::from_utf8_lossy(&o.stdout);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(obj) = v.as_object() {
                for (name, info) in obj {
                    let current = info["current"].as_str().unwrap_or("?").to_string();
                    let latest  = info["latest"].as_str().unwrap_or("?").to_string();
                    if current != latest {
                        report.outdated.push(OutdatedDep {
                            name: name.clone(), current, latest,
                            kind: "direct".into(),
                        });
                    }
                }
                report.outdated_count = report.outdated.len();
            }
        }
    }

    // npm audit --json
    let audit = Command::new("npm")
        .args(["audit", "--json"])
        .current_dir(dir)
        .output();

    if let Ok(o) = audit {
        let stdout = String::from_utf8_lossy(&o.stdout);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            // npm audit v7+ format
            if let Some(vulns) = v["vulnerabilities"].as_object() {
                for (name, info) in vulns {
                    let severity = info["severity"].as_str().unwrap_or("unknown").to_string();
                    let via: Vec<String> = info["via"].as_array()
                        .map(|a| a.iter()
                            .filter_map(|v| v["title"].as_str().map(str::to_string))
                            .collect())
                        .unwrap_or_default();
                    let desc = via.join(", ");
                    if severity == "critical" { report.cve_critical += 1; }
                    report.cve_issues.push(CveIssue {
                        package: name.clone(),
                        version: info["range"].as_str().unwrap_or("?").to_string(),
                        severity, description: desc, advisory_id: String::new(),
                    });
                }
                report.cve_count = report.cve_issues.len();
            }
        }
    }

    report
}

// ─── Python ──────────────────────────────────────────────────────────────────

fn check_python(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Python");
    report.has_lockfile = dir.join("poetry.lock").exists()
        || dir.join("Pipfile.lock").exists()
        || dir.join("requirements.txt").exists();

    // pip list --outdated --format=json
    let out = Command::new("pip")
        .args(["list", "--outdated", "--format=json"])
        .current_dir(dir)
        .output();

    match out {
        Err(_) => report.tool_missing.push("pip".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                for pkg in arr {
                    let name    = pkg["name"].as_str().unwrap_or("?").to_string();
                    let current = pkg["version"].as_str().unwrap_or("?").to_string();
                    let latest  = pkg["latest_version"].as_str().unwrap_or("?").to_string();
                    report.outdated.push(OutdatedDep { name, current, latest, kind: "direct".into() });
                }
                report.outdated_count = report.outdated.len();
            }
        }
    }

    // pip-audit --format=json (optional tool)
    let audit = Command::new("pip-audit")
        .args(["--format=json"])
        .current_dir(dir)
        .output();

    match audit {
        Err(_) => report.tool_missing.push("pip-audit (install: pip install pip-audit)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(deps) = v["dependencies"].as_array() {
                    for dep in deps {
                        if let Some(vulns) = dep["vulns"].as_array() {
                            for vuln in vulns {
                                let pkg  = dep["name"].as_str().unwrap_or("?").to_string();
                                let ver  = dep["version"].as_str().unwrap_or("?").to_string();
                                let id   = vuln["id"].as_str().unwrap_or("?").to_string();
                                let desc = vuln["description"].as_str().unwrap_or("?").to_string();
                                let sev  = vuln["fix_versions"].as_array()
                                    .map(|_| "high")
                                    .unwrap_or("unknown")
                                    .to_string();
                                report.cve_issues.push(CveIssue {
                                    package: pkg, version: ver, severity: sev,
                                    description: desc, advisory_id: id,
                                });
                            }
                        }
                    }
                    report.cve_count = report.cve_issues.len();
                    report.cve_critical = report.cve_issues.iter()
                        .filter(|i| i.severity == "critical")
                        .count();
                }
            }
        }
    }

    report
}

// ─── Go ──────────────────────────────────────────────────────────────────────

fn check_go(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Go");
    report.has_lockfile = dir.join("go.sum").exists();

    // go list -u -m -json all
    let out = Command::new("go")
        .args(["list", "-u", "-m", "-json", "all"])
        .current_dir(dir)
        .output();

    if let Ok(o) = out {
        let stdout = String::from_utf8_lossy(&o.stdout);
        // go list outputs multiple JSON objects concatenated — parse each
        let mut depth = 0i32;
        let mut buf = String::new();
        for ch in stdout.chars() {
            match ch {
                '{' => { depth += 1; buf.push(ch); }
                '}' => {
                    depth -= 1;
                    buf.push(ch);
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&buf) {
                            if let Some(update) = v.get("Update") {
                                let name    = v["Path"].as_str().unwrap_or("?").to_string();
                                let current = v["Version"].as_str().unwrap_or("?").to_string();
                                let latest  = update["Version"].as_str().unwrap_or("?").to_string();
                                report.outdated.push(OutdatedDep {
                                    name, current, latest, kind: "module".into(),
                                });
                            }
                        }
                        buf.clear();
                    }
                }
                _ if depth > 0 => buf.push(ch),
                _ => {}
            }
        }
        report.outdated_count = report.outdated.len();
    }

    // govulncheck (optional)
    let vuln = Command::new("govulncheck")
        .args(["./..."])
        .current_dir(dir)
        .output();

    if vuln.is_err() {
        report.tool_missing.push("govulncheck (install: go install golang.org/x/vuln/cmd/govulncheck@latest)".into());
    }

    report
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn cvss_to_severity(cvss: &str) -> &'static str {
    // CVSS v3 score ranges
    let score: f64 = cvss.parse().unwrap_or(0.0);
    match score as u8 {
        9..=10 => "critical",
        7..=8  => "high",
        4..=6  => "medium",
        1..=3  => "low",
        _      => "unknown",
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvss_to_severity_ranges() {
        assert_eq!(cvss_to_severity("9.8"),  "critical");
        assert_eq!(cvss_to_severity("7.5"),  "high");
        assert_eq!(cvss_to_severity("5.0"),  "medium");
        assert_eq!(cvss_to_severity("2.0"),  "low");
        assert_eq!(cvss_to_severity(""),     "unknown");
        assert_eq!(cvss_to_severity("abc"),  "unknown");
    }

    #[test]
    fn check_rust_project_runs_without_panic() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let r = check(&root);
        assert_eq!(r.project_type, "Rust");
        assert!(r.has_lockfile, "Cargo.lock should exist");
        // cargo-audit veya cargo-outdated yüklü değilse tool_missing'e düşer, panic olmaz
    }

    #[test]
    fn check_unknown_project() {
        let tmp = std::env::temp_dir().join("raios_deps_unknown");
        let _ = std::fs::create_dir_all(&tmp);
        let r = check(&tmp);
        assert_eq!(r.project_type, "Unknown");
        assert!(!r.tool_missing.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
