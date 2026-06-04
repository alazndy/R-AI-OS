use std::path::Path;
use std::process::Command;
use super::common::{DepsReport, CveIssue, OutdatedDep, cvss_to_severity};

pub fn check_rust(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Rust");
    report.has_lockfile = dir.join("Cargo.lock").exists();

    // cargo audit
    let audit_out = Command::new("cargo")
        .args(["audit", "--json"])
        .current_dir(dir)
        .output();

    match audit_out {
        Err(_) => report
            .tool_missing
            .push("cargo-audit (install: cargo install cargo-audit)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(vulns) = v["vulnerabilities"]["list"].as_array() {
                    for vuln in vulns {
                        let pkg = vuln["package"]["name"].as_str().unwrap_or("?").to_string();
                        let version = vuln["package"]["version"]
                            .as_str()
                            .unwrap_or("?")
                            .to_string();
                        let advisory = vuln["advisory"]["id"].as_str().unwrap_or("?").to_string();
                        let desc = vuln["advisory"]["title"]
                            .as_str()
                            .unwrap_or("?")
                            .to_string();
                        let severity =
                            cvss_to_severity(vuln["advisory"]["cvss"].as_str().unwrap_or(""))
                                .to_string();

                        if severity == "critical" {
                            report.cve_critical += 1;
                        }
                        report.cve_issues.push(CveIssue {
                            package: pkg,
                            version,
                            severity,
                            description: desc,
                            advisory_id: advisory,
                        });
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
        Err(_) => report
            .tool_missing
            .push("cargo-outdated (install: cargo install cargo-outdated)".into()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Some(deps) = v["dependencies"].as_array() {
                    for dep in deps {
                        let name = dep["name"].as_str().unwrap_or("?").to_string();
                        let current = dep["project"].as_str().unwrap_or("?").to_string();
                        let latest = dep["latest"].as_str().unwrap_or("?").to_string();
                        let kind = dep["kind"].as_str().unwrap_or("direct").to_string();
                        if current != latest && latest != "Removed" {
                            report.outdated.push(OutdatedDep {
                                name,
                                current,
                                latest,
                                kind,
                            });
                        }
                    }
                    report.outdated_count = report.outdated.len();
                }
            }
        }
    }

    report
}
