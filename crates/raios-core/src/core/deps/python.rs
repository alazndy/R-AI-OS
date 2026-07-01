use super::common::{CveIssue, DepsReport, OutdatedDep};
use std::path::Path;
use std::process::Command;

pub fn check_python(dir: &Path) -> DepsReport {
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
                    let name = pkg["name"].as_str().unwrap_or("?").to_string();
                    let current = pkg["version"].as_str().unwrap_or("?").to_string();
                    let latest = pkg["latest_version"].as_str().unwrap_or("?").to_string();
                    report.outdated.push(OutdatedDep {
                        name,
                        current,
                        latest,
                        kind: "direct".into(),
                    });
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
        Err(_) => report
            .tool_missing
            .push("pip-audit (install: pip install pip-audit)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(deps) = v["dependencies"].as_array() {
                    for dep in deps {
                        if let Some(vulns) = dep["vulns"].as_array() {
                            for vuln in vulns {
                                let pkg = dep["name"].as_str().unwrap_or("?").to_string();
                                let ver = dep["version"].as_str().unwrap_or("?").to_string();
                                let id = vuln["id"].as_str().unwrap_or("?").to_string();
                                let desc = vuln["description"].as_str().unwrap_or("?").to_string();
                                let sev = vuln["fix_versions"]
                                    .as_array()
                                    .map(|_| "high")
                                    .unwrap_or("unknown")
                                    .to_string();
                                report.cve_issues.push(CveIssue {
                                    package: pkg,
                                    version: ver,
                                    severity: sev,
                                    description: desc,
                                    advisory_id: id,
                                });
                            }
                        }
                    }
                    report.cve_count = report.cve_issues.len();
                    report.cve_critical = report
                        .cve_issues
                        .iter()
                        .filter(|i| i.severity == "critical")
                        .count();
                }
            }
        }
    }

    report
}
