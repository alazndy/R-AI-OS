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
                s if s.contains("4.") || s.contains("5.") || s.contains("6.") => Severity::Medium,
                _ => Severity::Low,
            };
            let pkg = vuln["package"]["name"].as_str().unwrap_or("unknown");
            let title = vuln["advisory"]["title"]
                .as_str()
                .unwrap_or("Vulnerability");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(output: &str, ptype: &ProjectType) -> Vec<SecurityIssue> {
        let mut issues = Vec::new();
        parse_audit_issues(output, ptype, &mut issues);
        issues
    }

    #[test]
    fn npm_audit_maps_all_severity_levels() {
        let output = r#"{
            "vulnerabilities": {
                "critical-pkg": { "severity": "critical", "title": "RCE in critical-pkg" },
                "high-pkg": { "severity": "high", "title": "XSS in high-pkg" },
                "moderate-pkg": { "severity": "moderate", "title": "CSRF in moderate-pkg" },
                "low-pkg": { "severity": "low", "title": "DoS in low-pkg" },
                "unknown-pkg": { "title": "Unlabeled severity" }
            }
        }"#;
        let issues = parse(output, &ProjectType::NodeJs);
        assert_eq!(issues.len(), 5);
        let by_title: Vec<_> = issues
            .iter()
            .map(|i| (i.severity.clone(), i.snippet.clone().unwrap()))
            .collect();
        assert!(by_title.contains(&(
            Severity::Critical,
            "critical-pkg: RCE in critical-pkg".into()
        )));
        assert!(by_title.contains(&(Severity::High, "high-pkg: XSS in high-pkg".into())));
        assert!(by_title.contains(&(
            Severity::Medium,
            "moderate-pkg: CSRF in moderate-pkg".into()
        )));
        assert!(by_title.contains(&(Severity::Low, "low-pkg: DoS in low-pkg".into())));
        assert!(by_title.contains(&(Severity::Low, "unknown-pkg: Unlabeled severity".into())));
    }

    #[test]
    fn npm_audit_falls_back_to_via_title() {
        let output = r#"{
            "vulnerabilities": {
                "lodash": {
                    "via": [ { "title": "Prototype Pollution in lodash" } ],
                    "severity": "high"
                }
            }
        }"#;
        let issues = parse(output, &ProjectType::NodeJs);
        assert_eq!(issues.len(), 1);
        assert!(issues[0]
            .snippet
            .as_deref()
            .unwrap()
            .contains("Prototype Pollution in lodash"));
    }

    #[test]
    fn npm_audit_truncates_long_titles_to_60_chars() {
        let long_title = "x".repeat(200);
        let output = format!(
            r#"{{ "vulnerabilities": {{ "pkg": {{ "severity": "low", "title": "{}" }} }} }}"#,
            long_title
        );
        let issues = parse(&output, &ProjectType::NodeJs);
        assert_eq!(issues.len(), 1);
        let snippet = issues[0].snippet.as_deref().unwrap();
        assert_eq!(snippet.len(), "pkg: ".len() + 60);
        assert!(snippet.starts_with("pkg: "));
    }

    #[test]
    fn cargo_audit_maps_cvss_score_to_severity() {
        let output = r#"{
            "vulnerabilities": {
                "list": [
                    { "advisory": { "cvss": "9.8", "title": "Critical advisory" },
                      "package": { "name": "tough-crate" } },
                    { "advisory": { "cvss": "7.5", "title": "High advisory" },
                      "package": { "name": "mid-crate" } },
                    { "advisory": { "cvss": "5.0", "title": "Medium advisory" },
                      "package": { "name": "ok-crate" } },
                    { "advisory": { "cvss": "3.0", "title": "Low advisory" },
                      "package": { "name": "low-crate" } },
                    { "advisory": { "title": "No cvss" },
                      "package": { "name": "nocrv-crate" } }
                ]
            }
        }"#;
        let issues = parse(output, &ProjectType::Rust);
        assert_eq!(issues.len(), 5);
        let severities: Vec<_> = issues.iter().map(|i| i.severity.clone()).collect();
        assert_eq!(
            severities,
            vec![
                Severity::Critical,
                Severity::High,
                Severity::Medium,
                Severity::Low,
                Severity::Low,
            ]
        );
        assert!(issues[0]
            .snippet
            .as_deref()
            .unwrap()
            .starts_with("tough-crate: "));
    }

    #[test]
    fn cargo_audit_skips_empty_vuln_list() {
        let output = r#"{ "vulnerabilities": { "list": [] } }"#;
        let issues = parse(output, &ProjectType::Rust);
        assert!(issues.is_empty());
    }

    #[test]
    fn pip_audit_extracts_fix_version_and_id() {
        let output = r#"{
            "dependencies": [
                {
                    "name": "requests",
                    "vulns": [
                        { "id": "CVE-2026-1234", "fix_versions": ["2.32.1"] }
                    ]
                },
                {
                    "name": "urllib3",
                    "vulns": [
                        { "id": "CVE-2026-5678" }
                    ]
                }
            ]
        }"#;
        let issues = parse(output, &ProjectType::Python);
        assert_eq!(issues.len(), 2);
        assert_eq!(
            issues[0].snippet.as_deref(),
            Some("requests CVE-2026-1234 (fix: 2.32.1)")
        );
        assert_eq!(
            issues[1].snippet.as_deref(),
            Some("urllib3 CVE-2026-5678 (fix: no fix)")
        );
        assert!(issues.iter().all(|i| i.severity == Severity::High));
    }

    #[test]
    fn invalid_json_produces_no_issues() {
        assert!(parse("not json", &ProjectType::NodeJs).is_empty());
        assert!(parse("", &ProjectType::Rust).is_empty());
        assert!(parse("[]", &ProjectType::Python).is_empty());
    }

    #[test]
    fn unsupported_project_types_are_ignored() {
        let output = r#"{ "vulnerabilities": { "pkg": { "severity": "critical" } } }"#;
        assert!(parse(output, &ProjectType::Web).is_empty());
        assert!(parse(output, &ProjectType::Mixed).is_empty());
        assert!(parse(output, &ProjectType::Unknown).is_empty());
    }
}
