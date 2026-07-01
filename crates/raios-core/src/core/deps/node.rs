use super::common::{CveIssue, DepsReport, OutdatedDep};
use std::path::Path;
use std::process::Command;

pub fn check_node(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Node");
    let pm = if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    };
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
                    let latest = info["latest"].as_str().unwrap_or("?").to_string();
                    if current != latest {
                        report.outdated.push(OutdatedDep {
                            name: name.clone(),
                            current,
                            latest,
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
                    let via: Vec<String> = info["via"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v["title"].as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    let desc = via.join(", ");
                    if severity == "critical" {
                        report.cve_critical += 1;
                    }
                    report.cve_issues.push(CveIssue {
                        package: name.clone(),
                        version: info["range"].as_str().unwrap_or("?").to_string(),
                        severity,
                        description: desc,
                        advisory_id: String::new(),
                    });
                }
                report.cve_count = report.cve_issues.len();
            }
        }
    }

    report
}
