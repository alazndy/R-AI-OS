use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High     => "HIGH",
            Self::Medium   => "MEDIUM",
            Self::Low      => "LOW",
            Self::Info     => "INFO",
        }
    }
    pub fn deduction(&self) -> i32 {
        match self {
            Self::Critical => 25,
            Self::High     => 15,
            Self::Medium   => 10,
            Self::Low      =>  5,
            Self::Info     =>  0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityIssue {
    pub owasp: &'static str,     // e.g. "A02"
    pub title: &'static str,
    pub severity: Severity,
    pub file: Option<PathBuf>,
    pub line: Option<usize>,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub score: u8,
    pub grade: &'static str,
    pub issues: Vec<SecurityIssue>,
    pub audit_output: Option<String>,  // npm/cargo audit raw
    pub project_type: ProjectType,
    pub checks_run: usize,
}

impl SecurityReport {
    pub fn grade_from_score(score: u8) -> &'static str {
        match score {
            90..=100 => "A",
            75..=89  => "B",
            50..=74  => "C",
            25..=49  => "D",
            _        => "F",
        }
    }

    pub fn critical_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == Severity::Critical).count()
    }
    pub fn high_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == Severity::High).count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProjectType {
    Rust,
    NodeJs,
    Python,
    Web,    // HTML/CSS/JS no package manager
    Mixed,
    Unknown,
}

// ─── OWASP static patterns ───────────────────────────────────────────────────

struct Pattern {
    owasp:    &'static str,
    title:    &'static str,
    severity: Severity,
    pattern:  &'static str,
    exts:     &'static [&'static str],  // file extensions to check
}

const PATTERNS: &[Pattern] = &[
    // A02 — Cryptographic Failures
    Pattern {
        owasp: "A02", title: "Hardcoded API key / secret",
        severity: Severity::Critical,
        pattern: r#"(?i)(api_key|api_secret|secret_key|auth_token|access_token|private_key)\s*[=:]\s*['"][a-zA-Z0-9_\-]{16,}['"]"#,
        exts: &["rs","py","ts","tsx","js","jsx","go","env","toml","yaml","yml","json"],
    },
    Pattern {
        owasp: "A02", title: "Hardcoded password",
        severity: Severity::Critical,
        pattern: r#"(?i)(password|passwd|pwd)\s*[=:]\s*['"][^'"]{4,}['"]"#,
        exts: &["rs","py","ts","js","go","yaml","yml","toml","env"],
    },
    Pattern {
        owasp: "A02", title: "MD5 used for hashing (weak)",
        severity: Severity::High,
        pattern: r"(?i)(md5|Md5)\s*::",
        exts: &["rs","py","ts","js","go"],
    },
    Pattern {
        owasp: "A02", title: "SHA1 used for hashing (weak)",
        severity: Severity::Medium,
        pattern: r"(?i)(sha1|Sha1)\s*::",
        exts: &["rs","py","ts","js","go"],
    },
    Pattern {
        owasp: "A02", title: "HTTP instead of HTTPS in config",
        severity: Severity::Medium,
        pattern: r#"http://(?!localhost|127\.0\.0\.1|0\.0\.0\.0)"#,
        exts: &["env","toml","yaml","yml","json","ts","js","py"],
    },

    // A03 — Injection
    Pattern {
        owasp: "A03", title: "SQL string interpolation (injection risk)",
        severity: Severity::High,
        pattern: r#"(?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s+.*\$\{|format!\s*\(\s*"(?i)(SELECT|INSERT|UPDATE|DELETE)"#,
        exts: &["rs","py","ts","js","go"],
    },
    Pattern {
        owasp: "A03", title: "eval() usage",
        severity: Severity::High,
        pattern: r"\beval\s*\(",
        exts: &["ts","js","jsx","tsx","py"],
    },
    Pattern {
        owasp: "A03", title: "innerHTML assignment (XSS risk)",
        severity: Severity::High,
        pattern: r"\.innerHTML\s*=",
        exts: &["ts","tsx","js","jsx","html"],
    },
    Pattern {
        owasp: "A03", title: "dangerouslySetInnerHTML",
        severity: Severity::Medium,
        pattern: r"dangerouslySetInnerHTML",
        exts: &["tsx","jsx","ts","js"],
    },
    Pattern {
        owasp: "A03", title: "Command injection via shell",
        severity: Severity::Critical,
        pattern: r#"(?i)(os\.system|subprocess\.call|popen|exec\s*\(|shell\s*=\s*True)"#,
        exts: &["py"],
    },
    Pattern {
        owasp: "A03", title: "Command injection via shell (JS)",
        severity: Severity::High,
        pattern: r"(?i)(exec\s*\(|execSync\s*\(|spawnSync\s*\().*\$\{",
        exts: &["ts","js"],
    },

    // A05 — Security Misconfiguration
    Pattern {
        owasp: "A05", title: "DEBUG=True in settings",
        severity: Severity::High,
        pattern: r"(?i)DEBUG\s*=\s*True",
        exts: &["py","env","toml","yaml","yml"],
    },
    Pattern {
        owasp: "A05", title: "CORS wildcard (*)",
        severity: Severity::Medium,
        pattern: r#"(?i)(cors|Access-Control-Allow-Origin).*['"]\*['""]"#,
        exts: &["rs","ts","js","py","go","yaml","yml"],
    },
    Pattern {
        owasp: "A05", title: "JWT secret is 'secret' or 'changeme'",
        severity: Severity::Critical,
        pattern: r#"(?i)(jwt|token).*['"](secret|changeme|your.?secret|example)['""]"#,
        exts: &["rs","ts","js","py","go","env","yaml","yml"],
    },
    Pattern {
        owasp: "A05", title: "Default credentials in config",
        severity: Severity::High,
        pattern: r#"(?i)(username|user)\s*[=:]\s*['"]admin['"]"#,
        exts: &["env","yaml","yml","toml","json"],
    },

    // A07 — Identification and Authentication Failures
    Pattern {
        owasp: "A07", title: "No rate limiting (missing throttle/ratelimit)",
        severity: Severity::Low,
        pattern: r"(?i)app\.(post|put|delete)\s*\(",
        exts: &["ts","js"],
    },
    Pattern {
        owasp: "A07", title: "Hardcoded JWT algorithm 'none'",
        severity: Severity::Critical,
        pattern: r#"(?i)algorithm.*['""]none['""]\s*"#,
        exts: &["rs","py","ts","js","go"],
    },

    // A09 — Security Logging and Monitoring Failures
    Pattern {
        owasp: "A09", title: "console.log with potential sensitive data",
        severity: Severity::Low,
        pattern: r"console\.log\s*\(.*(?i)(password|token|secret|key)",
        exts: &["ts","js","tsx","jsx"],
    },
    Pattern {
        owasp: "A09", title: "print() with potential sensitive data (Python)",
        severity: Severity::Low,
        pattern: r"print\s*\(.*(?i)(password|token|secret|key)",
        exts: &["py"],
    },

    // A01 — Broken Access Control
    Pattern {
        owasp: "A01", title: "Directory traversal pattern",
        severity: Severity::High,
        pattern: r"\.\./|\.\.\\",
        exts: &["rs","py","ts","js","go"],
    },
    Pattern {
        owasp: "A01", title: ".unwrap() on auth/permission result (Rust)",
        severity: Severity::Low,
        pattern: r"(?i)(auth|permission|role|access).*\.unwrap\(\)",
        exts: &["rs"],
    },
];

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", ".git", "dist", "build",
    ".next", "__pycache__", "vendor", ".turbo",
];

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn scan_project(path: &Path) -> SecurityReport {
    let project_type = detect_project_type(path);
    let mut issues = Vec::new();
    let mut checks_run = 0;

    // 1. Static pattern scan
    static_scan(path, &mut issues, &mut checks_run);

    // 2. Check .env committed to git
    check_env_in_git(path, &mut issues);
    checks_run += 1;

    // 3. Dependency audit
    let audit_output = run_dependency_audit(path, &project_type);

    // 4. Parse audit output for issues
    if let Some(ref output) = audit_output {
        parse_audit_issues(output, &project_type, &mut issues);
    }

    // 5. Calculate score
    let deductions: i32 = issues.iter().map(|i| i.severity.deduction()).sum();
    let score = (100i32 - deductions).clamp(0, 100) as u8;
    let grade = SecurityReport::grade_from_score(score);

    SecurityReport { score, grade, issues, audit_output, project_type, checks_run }
}

// ─── Detection ────────────────────────────────────────────────────────────────

fn detect_project_type(path: &Path) -> ProjectType {
    if path.join("Cargo.toml").exists() { return ProjectType::Rust; }
    if path.join("package.json").exists() { return ProjectType::NodeJs; }
    if path.join("pyproject.toml").exists() || path.join("requirements.txt").exists() || path.join("setup.py").exists() {
        return ProjectType::Python;
    }
    if path.join("index.html").exists() { return ProjectType::Web; }
    ProjectType::Unknown
}

// ─── Static scan ─────────────────────────────────────────────────────────────

fn static_scan(root: &Path, issues: &mut Vec<SecurityIssue>, checks_run: &mut usize) {
    let walker = WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.') && !SKIP_DIRS.contains(&n.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in walker {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let patterns_for_ext: Vec<&Pattern> = PATTERNS.iter()
            .filter(|p| p.exts.contains(&ext))
            .collect();

        if patterns_for_ext.is_empty() { continue; }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        *checks_run += 1;

        for pattern in &patterns_for_ext {
            let re = match regex_lite::Regex::new(pattern.pattern) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for (line_no, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    let snippet = line.trim().chars().take(80).collect::<String>();
                    issues.push(SecurityIssue {
                        owasp: pattern.owasp,
                        title: pattern.title,
                        severity: pattern.severity.clone(),
                        file: Some(path.to_path_buf()),
                        line: Some(line_no + 1),
                        snippet: Some(snippet),
                    });
                    break; // one finding per pattern per file
                }
            }
        }
    }
}

// ─── .env in git check ───────────────────────────────────────────────────────

fn check_env_in_git(path: &Path, issues: &mut Vec<SecurityIssue>) {
    // Check if .env is tracked by git
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

    // Check .gitignore for .env
    let gitignore = path.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if !content.lines().any(|l| l.trim() == ".env" || l.trim() == "*.env") {
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

// ─── Dependency audit ─────────────────────────────────────────────────────────

fn run_dependency_audit(path: &Path, ptype: &ProjectType) -> Option<String> {
    let (cmd, args): (&str, &[&str]) = match ptype {
        ProjectType::NodeJs => ("pnpm", &["audit", "--json"]),
        ProjectType::Rust   => ("cargo", &["audit", "--json"]),
        ProjectType::Python => ("pip-audit", &["--format=json"]),
        _ => return None,
    };

    let out = Command::new(cmd)
        .args(args)
        .current_dir(path)
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn parse_audit_issues(output: &str, ptype: &ProjectType, issues: &mut Vec<SecurityIssue>) {
    // Try to parse JSON audit output for vulnerability counts
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(output) {
        match ptype {
            ProjectType::NodeJs => parse_npm_audit(&json, issues),
            ProjectType::Rust   => parse_cargo_audit(&json, issues),
            ProjectType::Python => parse_pip_audit(&json, issues),
            _ => {}
        }
    }
}

fn parse_npm_audit(json: &serde_json::Value, issues: &mut Vec<SecurityIssue>) {
    // pnpm audit --json format
    if let Some(vulns) = json["vulnerabilities"].as_object() {
        for (pkg, vuln) in vulns {
            let severity_str = vuln["severity"].as_str().unwrap_or("low");
            let severity = match severity_str {
                "critical" => Severity::Critical,
                "high"     => Severity::High,
                "moderate" => Severity::Medium,
                _          => Severity::Low,
            };
            let title = vuln["title"].as_str()
                .or_else(|| vuln["via"].as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v["title"].as_str()))
                .unwrap_or("Vulnerable dependency");
            issues.push(SecurityIssue {
                owasp: "A06",
                title: "Vulnerable dependency (npm)",
                severity,
                file: None,
                line: None,
                snippet: Some(format!("{}: {}", pkg, title.chars().take(60).collect::<String>())),
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
                s if s.contains("7.") || s.contains("8.")  => Severity::High,
                s if s.contains("4.") || s.contains("5.") || s.contains("6.") => Severity::Medium,
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
                snippet: Some(format!("{}: {}", pkg, title.chars().take(60).collect::<String>())),
            });
        }
    }
}

fn parse_pip_audit(json: &serde_json::Value, issues: &mut Vec<SecurityIssue>) {
    if let Some(deps) = json["dependencies"].as_array() {
        for dep in deps {
            if let Some(vulns) = dep["vulns"].as_array() {
                for vuln in vulns {
                    let fix = vuln["fix_versions"].as_array()
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

pub fn score_color(score: u8) -> &'static str {
    match score {
        90..=100 => "A",
        75..=89  => "B",
        50..=74  => "C",
        25..=49  => "D",
        _        => "F",
    }
}

pub fn severity_emoji(s: &Severity) -> &'static str {
    match s {
        Severity::Critical => "🔴",
        Severity::High     => "🟠",
        Severity::Medium   => "🟡",
        Severity::Low      => "🔵",
        Severity::Info     => "⚪",
    }
}
