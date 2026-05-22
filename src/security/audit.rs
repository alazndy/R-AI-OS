use super::{ProjectType, SecurityIssue, Severity};
use std::path::Path;
use std::process::Command;

pub(super) fn run_dependency_audit(path: &Path, ptype: &ProjectType) -> Option<String> {
    let (cmd, args): (&str, &[&str]) = match ptype {
        ProjectType::NodeJs => ("pnpm", &["audit", "--json"]),
        ProjectType::Rust => ("cargo", &["audit", "--json"]),
        ProjectType::Python => ("pip-audit", &["--format=json"]),
        _ => return None,
    };

    let out = Command::new(cmd)
        .args(args)
        .current_dir(path)
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(super) fn parse_audit_issues(
    output: &str,
    ptype: &ProjectType,
    issues: &mut Vec<SecurityIssue>,
) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(output) {
        match ptype {
            ProjectType::NodeJs => parse_npm_audit(&json, issues),
            ProjectType::Rust => parse_cargo_audit(&json, issues),
            ProjectType::Python => parse_pip_audit(&json, issues),
            _ => {}
        }
    }
}

fn parse_npm_audit(json: &serde_json::Value, issues: &mut Vec<SecurityIssue>) {
    if let Some(vulns) = json["vulnerabilities"].as_object() {
        for (pkg, vuln) in vulns {
            let severity_str = vuln["severity"].as_str().unwrap_or("low");
            let severity = match severity_str {
                "critical" => Severity::Critical,
                "high" => Severity::High,
                "moderate" => Severity::Medium,
                _ => Severity::Low,
            };
            let title = vuln["title"]
                .as_str()
                .or_else(|| {
                    vuln["via"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v["title"].as_str())
                })
                .unwrap_or("Vulnerable dependency");
            issues.push(SecurityIssue {
                owasp: "A06",
                title: "Vulnerable dependency (npm)",
                severity,
                file: None,
                line: None,
                snippet: Some(format!(
                    "{}: {}",
                    pkg,
                    title.chars().take(60).collect::<String>()
                )),
            });
        }
    }
}

fn parse_cargo_audit(json: &serde_json::Value, issues: &mut Vec<SecurityIssue>) {
    if let Some(vulns) = json["vulnerabilities"]["list"].as_array() {
        for vuln in vulns {
            let severity_str = vuln["advisory"]["cvss"].as_str().unwrap_or("");
            let severity = match severity_str {
                s if s.contains("9.") || s.contains("10.") => Severity::Critical,
                s if s.contains("7.") || s.contains("8.") => Severity::High,
                s if s.contains("4.")
                    || s.contains("5.")
                    || s.contains("6.") =>
                {
                    Severity::Medium
                }
                _ => Severity::Low,
            };
            let pkg = vuln["package"]["name"].as_str().unwrap_or("unknown");
            let title = vuln["advisory"]["title"].as_str().unwrap_or("Vulnerability");
            issues.push(SecurityIssue {
                owasp: "A06",
                title: "Vulnerable dependency (cargo)",
                severity,
                file: None,
                line: None,
                snippet: Some(format!(
                    "{}: {}",
                    pkg,
                    title.chars().take(60).collect::<String>()
                )),
            });
        }
    }
}

fn parse_pip_audit(json: &serde_json::Value, issues: &mut Vec<SecurityIssue>) {
    if let Some(deps) = json["dependencies"].as_array() {
        for dep in deps {
            if let Some(vulns) = dep["vulns"].as_array() {
                for vuln in vulns {
                    let fix = vuln["fix_versions"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("no fix");
                    let pkg = dep["name"].as_str().unwrap_or("unknown");
                    let id = vuln["id"].as_str().unwrap_or("CVE-?");
                    issues.push(SecurityIssue {
                        owasp: "A06",
                        title: "Vulnerable dependency (pip)",
                        severity: Severity::High,
                        file: None,
                        line: None,
                        snippet: Some(format!("{} {} (fix: {})", pkg, id, fix)),
                    });
                }
            }
        }
    }
}
