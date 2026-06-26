use super::audit::{parse_audit_issues, run_dependency_audit};
use super::patterns::{compiled_pattern_regexes, PATTERNS, SKIP_DIRS};
use super::{ProjectType, SecurityIssue, SecurityReport, Severity};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Strip everything from the first `#[cfg(test)]` marker to end of file.
/// Test modules in idiomatic Rust live at the bottom, so this eliminates
/// fixture data from security scans without a full parser.
fn strip_cfg_test_tail<'a>(content: &'a str, ext: &str) -> &'a str {
    if ext != "rs" {
        return content;
    }
    match content.find("#[cfg(test)]") {
        Some(idx) => &content[..idx],
        None => content,
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Full scan: static patterns + semgrep + dependency audit (`cargo audit` / `pnpm audit`).
/// Use for `raios security` — heavy, may spike RAM on large workspaces.
pub fn scan_project(path: &Path) -> SecurityReport {
    scan_project_impl(path, true)
}

/// Fast scan: static patterns only — no semgrep, no dependency audit.
/// Use for the background health worker to avoid `cargo audit` RAM spikes.
pub fn scan_project_fast(path: &Path) -> SecurityReport {
    scan_project_impl(path, false)
}

fn scan_project_impl(path: &Path, include_dep_audit: bool) -> SecurityReport {
    let project_type = detect_project_type(path);
    let mut issues = Vec::new();
    let mut checks_run = 0;

    static_scan(path, &mut issues, &mut checks_run);
    check_env_in_git(path, &mut issues);
    checks_run += 1;

    if include_dep_audit {
        if run_semgrep(path, &mut issues) {
            checks_run += 1;
        }
    }

    let audit_output = if include_dep_audit {
        let out = run_dependency_audit(path, &project_type);
        if let Some(ref output) = out {
            parse_audit_issues(output, &project_type, &mut issues);
        }
        out
    } else {
        None
    };

    let deductions: i32 = issues.iter().map(|i| i.severity.deduction()).sum();
    let score = (100i32 - deductions).clamp(0, 100) as u8;
    let grade = SecurityReport::grade_from_score(score);

    SecurityReport {
        score,
        grade,
        issues,
        audit_output,
        project_type,
        checks_run,
    }
}

// ─── Project type detection ───────────────────────────────────────────────────

fn detect_project_type(path: &Path) -> ProjectType {
    if let Ok(content) = std::fs::read_to_string(path.join(".raios.yaml")) {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("stack:") {
                let stack = rest.trim().trim_matches('"');
                return match stack {
                    "rust" => ProjectType::Rust,
                    "node" | "nodejs" => ProjectType::NodeJs,
                    "python" => ProjectType::Python,
                    "web" => ProjectType::Web,
                    _ => ProjectType::Unknown,
                };
            }
        }
    }
    if path.join("Cargo.toml").exists() {
        return ProjectType::Rust;
    }
    if path.join("package.json").exists() {
        return ProjectType::NodeJs;
    }
    if path.join("pyproject.toml").exists()
        || path.join("requirements.txt").exists()
        || path.join("setup.py").exists()
    {
        return ProjectType::Python;
    }
    if path.join("index.html").exists() {
        return ProjectType::Web;
    }
    ProjectType::Unknown
}

// ─── Static scan ─────────────────────────────────────────────────────────────

fn static_scan(root: &Path, issues: &mut Vec<SecurityIssue>, checks_run: &mut usize) {
    let walker = WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let n = e.file_name().to_string_lossy();
            if e.file_type().is_dir() {
                !n.starts_with('.') && !SKIP_DIRS.contains(&n.as_ref())
            } else {
                true
            }
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    let compiled = compiled_pattern_regexes();

    for entry in walker {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let has_patterns = PATTERNS.iter().any(|p| p.exts.contains(&ext));
        if !has_patterns {
            continue;
        }

        // Skip the scanner's own pattern definition file to avoid self-matches
        // on pattern title strings (e.g. title: "JWT secret is 'secret'...").
        let path_str = path.to_string_lossy();
        if path_str.contains("security") && path_str.ends_with("patterns.rs") {
            continue;
        }

        let raw = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let content = strip_cfg_test_tail(&raw, ext);

        *checks_run += 1;

        for (pattern, re_opt) in PATTERNS.iter().zip(compiled.iter()) {
            if !pattern.exts.contains(&ext) {
                continue;
            }
            let re = match re_opt {
                Some(r) => r,
                None => continue,
            };
            for (line_no, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    let raw = line.trim();
                    let snippet = if raw.chars().count() > 80 {
                        format!("{}…", raw.chars().take(80).collect::<String>())
                    } else {
                        raw.to_string()
                    };
                    issues.push(SecurityIssue {
                        owasp: pattern.owasp,
                        title: pattern.title,
                        severity: pattern.severity.clone(),
                        file: Some(path.to_path_buf()),
                        line: Some(line_no + 1),
                        snippet: Some(snippet),
                    });
                    break;
                }
            }
        }
    }
}

// ─── .env in git check ───────────────────────────────────────────────────────

fn check_env_in_git(path: &Path, issues: &mut Vec<SecurityIssue>) {
    let out = Command::new("git")
        .args(["ls-files", ".env"])
        .current_dir(path)
        .output();

    if let Ok(o) = out {
        if !o.stdout.is_empty() {
            issues.push(SecurityIssue {
                owasp: "A02",
                title: ".env file is tracked by git",
                severity: Severity::Critical,
                file: Some(path.join(".env")),
                line: None,
                snippet: Some(".env should be in .gitignore".into()),
            });
        }
    }

    let gitignore = path.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if !content
                .lines()
                .any(|l| l.trim() == ".env" || l.trim() == "*.env")
            {
                issues.push(SecurityIssue {
                    owasp: "A02",
                    title: ".env not in .gitignore",
                    severity: Severity::High,
                    file: Some(gitignore),
                    line: None,
                    snippet: Some("Add .env to .gitignore".into()),
                });
            }
        }
    }
}

// ─── Semgrep ─────────────────────────────────────────────────────────────────

fn run_semgrep(path: &Path, issues: &mut Vec<SecurityIssue>) -> bool {
    run_semgrep_inner(path, issues).unwrap_or(false)
}

fn run_semgrep_inner(path: &Path, issues: &mut Vec<SecurityIssue>) -> Option<bool> {
    let semgrep_path = which::which("semgrep").ok()?;

    let output = Command::new(&semgrep_path)
        .args([
            "--config",
            "p/owasp-top-ten",
            "--json",
            "--quiet",
            "--no-rewrite-rule-ids",
            path.to_str().unwrap_or("."),
        ])
        .output()
        .ok()?;

    if !output.status.success() && output.stdout.is_empty() {
        return Some(false);
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let results = json["results"].as_array()?;

    for r in results {
        let msg = r["extra"]["message"]
            .as_str()
            .unwrap_or("semgrep finding")
            .to_string();
        let sev = match r["extra"]["severity"].as_str().unwrap_or("WARNING") {
            "ERROR" => Severity::Critical,
            "WARNING" => Severity::High,
            "INFO" => Severity::Low,
            _ => Severity::Medium,
        };
        let owasp_tag = r["extra"]["metadata"]["owasp"].as_str().unwrap_or("");
        let owasp: &'static str = [
            "A01", "A02", "A03", "A04", "A05", "A06", "A07", "A08", "A09", "A10",
        ]
        .iter()
        .find(|&&o| owasp_tag.contains(o))
        .copied()
        .unwrap_or("A00");

        issues.push(SecurityIssue {
            owasp,
            title: "semgrep finding",
            severity: sev,
            file: r["path"].as_str().map(PathBuf::from),
            line: r["start"]["line"].as_u64().map(|l| l as usize),
            snippet: Some(msg),
        });
    }
    Some(!results.is_empty() || output.status.success())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_file(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn static_scan_ignores_cfg_test_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    fn fixture() { let api_key = \"sk-abc123456789abcdef1234567890abcdef\"; }\n}\n",
        ).unwrap();
        let mut issues = Vec::new();
        let mut count = 0;
        static_scan(tmp.path(), &mut issues, &mut count);
        let has_false_positive = issues
            .iter()
            .any(|i| i.owasp == "A02" && i.severity == crate::security::Severity::Critical);
        assert!(
            !has_false_positive,
            "Should not flag API key string inside #[cfg(test)] block. Issues: {:?}",
            issues.len()
        );
    }

    #[test]
    fn hardcoded_api_key_detected() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "app.ts",
            r#"const api_key = "sk-1234567890abcdef1234567890abcdef";"#,
        );
        let report = scan_project(tmp.path());
        let has_key_issue = report
            .issues
            .iter()
            .any(|i| i.owasp == "A02" && i.severity == Severity::Critical);
        assert!(has_key_issue, "Should detect hardcoded API key");
    }

    #[test]
    fn clean_file_no_critical_issues() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "main.rs",
            r#"fn main() { println!("Hello, world!"); }"#,
        );
        let report = scan_project(tmp.path());
        assert_eq!(report.critical_count(), 0);
    }

    #[test]
    fn debug_true_detected() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "settings.py", "DEBUG = True\n");
        let report = scan_project(tmp.path());
        let found = report
            .issues
            .iter()
            .any(|i| i.owasp == "A05" && i.title.contains("DEBUG"));
        assert!(found, "Should detect DEBUG=True");
    }

    #[test]
    fn eval_usage_detected() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(tmp.path(), "app.js", "eval(userInput);");
        let report = scan_project(tmp.path());
        let found = report.issues.iter().any(|i| i.owasp == "A03");
        assert!(found, "Should detect eval()");
    }

    #[test]
    fn score_starts_at_100() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "clean.rs",
            "fn add(a: i32, b: i32) -> i32 { a + b }",
        );
        let report = scan_project(tmp.path());
        assert_eq!(report.score, 100, "Clean file should score 100");
    }

    #[test]
    fn api_key_regex_compiles_and_matches() {
        let pattern = r#"(?i)(api_key|api_secret|secret_key|auth_token|access_token|private_key)\s*[=:]\s*['"][a-zA-Z0-9_-]{16,}['"]"#;
        let re = regex_lite::Regex::new(pattern).expect("Regex must compile");
        let content = r#"const api_key = "sk-1234567890abcdef1234567890abcdef";"#;
        assert!(re.is_match(content), "Regex should match hardcoded API key");
    }

    #[test]
    fn eval_regex_compiles_and_matches() {
        let re = regex_lite::Regex::new(r"\beval\s*\(").expect("Regex must compile");
        assert!(re.is_match("eval(userInput);"));
    }

    #[test]
    fn debug_regex_compiles_and_matches() {
        let re = regex_lite::Regex::new(r"(?i)DEBUG\s*=\s*True").expect("Regex must compile");
        assert!(re.is_match("DEBUG = True"));
    }

    #[test]
    fn static_scan_finds_api_key() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "app.ts",
            r#"const api_key = "sk-1234567890abcdef1234567890abcdef";"#,
        );
        let mut issues = Vec::new();
        let mut count = 0;
        static_scan(tmp.path(), &mut issues, &mut count);
        let found = issues
            .iter()
            .any(|i| i.owasp == "A02" && i.severity == Severity::Critical);
        assert!(
            found,
            "static_scan should find API key. Issues found: {:?}",
            issues.len()
        );
    }

    #[test]
    fn manifest_detected_as_rust() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".raios.yaml"), "stack: \"rust\"\n").unwrap();
        let ptype = detect_project_type(tmp.path());
        assert_eq!(ptype, ProjectType::Rust);
    }

    #[test]
    fn manifest_overrides_heuristic() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join(".raios.yaml"), "stack: \"python\"\n").unwrap();
        let ptype = detect_project_type(tmp.path());
        assert_eq!(
            ptype,
            ProjectType::Python,
            "Manifest should override heuristic"
        );
    }
}
