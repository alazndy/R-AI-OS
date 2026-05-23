use super::{SecurityIssue, Severity};
use std::path::Path;

// ─── Pattern definition ───────────────────────────────────────────────────────

pub(super) struct Pattern {
    pub owasp: &'static str,
    pub title: &'static str,
    pub severity: Severity,
    pub pattern: &'static str,
    pub exts: &'static [&'static str],
}

pub(super) const PATTERNS: &[Pattern] = &[
    // A02 — Cryptographic Failures
    Pattern {
        owasp: "A02",
        title: "Hardcoded API key / secret",
        severity: Severity::Critical,
        pattern: r#"(?i)(api_key|api_secret|secret_key|auth_token|access_token|private_key)\s*[=:]\s*['"][a-zA-Z0-9_-]{16,}['"]"#,
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml", "json",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "Hardcoded password",
        severity: Severity::Critical,
        pattern: r#"(?i)(password|passwd|pwd)\s*[=:]\s*['"][^'"]{4,}['"]"#,
        exts: &["rs", "py", "ts", "js", "go", "yaml", "yml", "toml", "env"],
    },
    Pattern {
        owasp: "A02",
        title: "MD5 used for hashing (weak)",
        severity: Severity::High,
        pattern: r"(?i)(md5|Md5)\s*::",
        exts: &["rs", "py", "ts", "js", "go"],
    },
    Pattern {
        owasp: "A02",
        title: "SHA1 used for hashing (weak)",
        severity: Severity::Medium,
        pattern: r"(?i)(sha1|Sha1)\s*::",
        exts: &["rs", "py", "ts", "js", "go"],
    },
    Pattern {
        owasp: "A02",
        title: "HTTP instead of HTTPS in config",
        severity: Severity::Medium,
        pattern: r#"http://(?!localhost|127\.0\.0\.1|0\.0\.0\.0)"#,
        exts: &["env", "toml", "yaml", "yml", "json", "ts", "js", "py"],
    },
    // A03 — Injection
    Pattern {
        owasp: "A03",
        title: "SQL string interpolation (injection risk)",
        severity: Severity::High,
        pattern: r#"(?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s+.*\$\{|format!\s*\(\s*"(?i)(SELECT|INSERT|UPDATE|DELETE)"#,
        exts: &["rs", "py", "ts", "js", "go"],
    },
    Pattern {
        owasp: "A03",
        title: "eval() usage",
        severity: Severity::High,
        pattern: r"\beval\s*\(",
        exts: &["ts", "js", "jsx", "tsx", "py"],
    },
    Pattern {
        owasp: "A03",
        title: "innerHTML assignment (XSS risk)",
        severity: Severity::High,
        pattern: r"\.innerHTML\s*=",
        exts: &["ts", "tsx", "js", "jsx", "html"],
    },
    Pattern {
        owasp: "A03",
        title: "dangerouslySetInnerHTML",
        severity: Severity::Medium,
        pattern: r"dangerouslySetInnerHTML",
        exts: &["tsx", "jsx", "ts", "js"],
    },
    Pattern {
        owasp: "A03",
        title: "Command injection via shell",
        severity: Severity::Critical,
        pattern: r#"(?i)(os\.system|subprocess\.call|popen|exec\s*\(|shell\s*=\s*True)"#,
        exts: &["py"],
    },
    Pattern {
        owasp: "A03",
        title: "Command injection via shell (JS)",
        severity: Severity::High,
        pattern: r"(?i)(exec\s*\(|execSync\s*\(|spawnSync\s*\().*\$\{",
        exts: &["ts", "js"],
    },
    // A05 — Security Misconfiguration
    Pattern {
        owasp: "A05",
        title: "DEBUG=True in settings",
        severity: Severity::High,
        pattern: r"(?i)DEBUG\s*=\s*True",
        exts: &["py", "env", "toml", "yaml", "yml"],
    },
    Pattern {
        owasp: "A05",
        title: "CORS wildcard (*)",
        severity: Severity::Medium,
        pattern: r#"(?i)(cors|Access-Control-Allow-Origin).*['"]\*['""]"#,
        exts: &["rs", "ts", "js", "py", "go", "yaml", "yml"],
    },
    Pattern {
        owasp: "A05",
        title: "JWT secret is 'secret' or 'changeme'",
        severity: Severity::Critical,
        pattern: r#"(?i)(jwt|token).*['"](secret|changeme|your.?secret|example)['""]"#,
        exts: &["rs", "ts", "js", "py", "go", "env", "yaml", "yml"],
    },
    Pattern {
        owasp: "A05",
        title: "Default credentials in config",
        severity: Severity::High,
        pattern: r#"(?i)(username|user)\s*[=:]\s*['"]admin['"]"#,
        exts: &["env", "yaml", "yml", "toml", "json"],
    },
    // A07 — Identification and Authentication Failures
    Pattern {
        owasp: "A07",
        title: "No rate limiting (missing throttle/ratelimit)",
        severity: Severity::Low,
        pattern: r"(?i)app\.(post|put|delete)\s*\(",
        exts: &["ts", "js"],
    },
    Pattern {
        owasp: "A07",
        title: "Hardcoded JWT algorithm 'none'",
        severity: Severity::Critical,
        pattern: r#"(?i)algorithm.*['""]none['""]\s*"#,
        exts: &["rs", "py", "ts", "js", "go"],
    },
    // A09 — Security Logging and Monitoring Failures
    Pattern {
        owasp: "A09",
        title: "console.log with potential sensitive data",
        severity: Severity::Low,
        pattern: r"console\.log\s*\(.*(?i)(password|token|secret|key)",
        exts: &["ts", "js", "tsx", "jsx"],
    },
    Pattern {
        owasp: "A09",
        title: "print() with potential sensitive data (Python)",
        severity: Severity::Low,
        pattern: r"print\s*\(.*(?i)(password|token|secret|key)",
        exts: &["py"],
    },
    // A02 — Specific credential formats (high-specificity, low false-positive rate)
    Pattern {
        owasp: "A02",
        title: "AWS Access Key ID (AKIA prefix)",
        severity: Severity::Critical,
        pattern: r"\bAKIA[0-9A-Z]{16}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "sh",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "GitHub Personal Access Token (ghp_ prefix)",
        severity: Severity::Critical,
        pattern: r"\bghp_[a-zA-Z0-9]{36}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "sh",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "Stripe secret key (sk_live_ or sk_test_)",
        severity: Severity::Critical,
        pattern: r"\bsk_(live|test)_[0-9a-zA-Z]{24,}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml", "json",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "Google API key (AIza prefix)",
        severity: Severity::Critical,
        pattern: r"\bAIza[0-9A-Za-z\-_]{35}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "html",
        ],
    },
    // A01 — Broken Access Control
    Pattern {
        owasp: "A01",
        title: "Directory traversal pattern",
        severity: Severity::High,
        // Match ../ or ..\ only when NOT on an import/require/use/include line,
        // to avoid flagging module-relative imports as directory traversal.
        pattern: r"^(?!.*\b(import|require|use|from|include)\b).*(\.\./|\.\.\\)",
        exts: &["rs", "py", "ts", "js", "go"],
    },
    Pattern {
        owasp: "A01",
        title: ".unwrap() on auth/permission result (Rust)",
        severity: Severity::Low,
        pattern: r"(?i)(auth|permission|role|access).*\.unwrap\(\)",
        exts: &["rs"],
    },
];

pub(super) const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    ".next",
    "__pycache__",
    "vendor",
    ".turbo",
    "graphify-out",
];

pub const WATCHED_EXTS: &[&str] = &[
    "rs", "ts", "js", "tsx", "jsx", "py", "env", "json", "toml", "yaml", "yml",
];

// ─── Regex cache ─────────────────────────────────────────────────────────────

pub(super) fn compiled_pattern_regexes() -> &'static Vec<Option<regex_lite::Regex>> {
    static COMPILED: std::sync::OnceLock<Vec<Option<regex_lite::Regex>>> =
        std::sync::OnceLock::new();
    COMPILED.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|p| regex_lite::Regex::new(p.pattern).ok())
            .collect()
    })
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn scan_file(path: &Path) -> Vec<SecurityIssue> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !WATCHED_EXTS.contains(&ext) {
        return vec![];
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[security] cannot read {}: {e}", path.display());
            return vec![];
        }
    };
    // Strip #[cfg(test)] tail from Rust files to avoid fixture false positives
    let content = if ext == "rs" {
        raw.find("#[cfg(test)]").map(|i| &raw[..i]).unwrap_or(&raw)
    } else {
        &raw
    };

    let compiled = compiled_pattern_regexes();
    let mut issues = Vec::new();

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
    issues
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_scan_file {
    use super::*;
    use std::io::Write;

    #[test]
    fn watched_exts_contains_expected() {
        assert!(WATCHED_EXTS.contains(&"rs"));
        assert!(WATCHED_EXTS.contains(&"env"));
        assert!(WATCHED_EXTS.contains(&"ts"));
        assert!(WATCHED_EXTS.contains(&"py"));
        assert!(!WATCHED_EXTS.contains(&"png"));
    }

    #[test]
    fn scan_file_detects_hardcoded_secret() {
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, r#"api_key = "sk-abc123456789abcdef""#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            !issues.is_empty(),
            "Expected at least one issue for hardcoded api_key"
        );
        assert!(issues.iter().any(|i| i.owasp == "A02"));
    }

    #[test]
    fn scan_file_clean_file_returns_empty() {
        let mut f = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(f, "fn main() {{ println!(\"hello\"); }}").unwrap();
        let issues = scan_file(f.path());
        assert!(issues.is_empty(), "Expected no issues for clean file");
    }

    #[test]
    fn detects_aws_access_key() {
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE").unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("AWS")),
            "Should detect AWS access key ID"
        );
    }

    #[test]
    fn detects_github_pat() {
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, "GITHUB_TOKEN=ghp_16C7e42F292c6912E7710c838347Ae178B4a").unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("GitHub")),
            "Should detect GitHub PAT (ghp_ prefix)"
        );
    }

    #[test]
    fn detects_stripe_live_key() {
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        // Key split at runtime so static scanners don't flag this test fixture
        let fake = format!("STRIPE_SECRET=sk_live_{}", "51H2BLkJ3Ow1234567890abcde");
        writeln!(f, "{}", fake).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("Stripe")),
            "Should detect Stripe live secret key"
        );
    }

    #[test]
    fn detects_google_api_key() {
        let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
        writeln!(f, r#"const key = "AIzaSyD-9tSrke72I6e0sEh8bT9SfGgfHIqnYjw";"#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("Google")),
            "Should detect Google API key (AIza prefix)"
        );
    }
}
