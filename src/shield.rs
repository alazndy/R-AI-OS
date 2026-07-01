use regex_lite::Regex;
use std::path::Path;

pub struct AgentShield {
    dangerous_patterns: Vec<Regex>,
}

pub struct PreflightFinding {
    pub label: &'static str,
    pub detail: String,
    pub blocking: bool,
}

impl AgentShield {
    pub fn init() -> Self {
        let patterns = vec![
            r"rm\s+-rf\s+/",      // Delete root
            r"rm\s+-rf\s+\$HOME", // Delete home
            r"mkfs\s+",           // Format disk
            r"dd\s+if=",          // Low-level disk write
            r">\s+/dev/sd",       // Overwrite disk device
            r"curl.*\|\s*sh",     // Pipe to shell (dangerous)
            r"wget.*\|\s*sh",
            r"cat\s+\.env", // Stealing secrets
            r"grep.*sk-",   // Searching for API keys
        ];

        let compiled = patterns
            .iter()
            .map(|p| Regex::new(p).expect("Invalid regex in Shield"))
            .collect();

        Self {
            dangerous_patterns: compiled,
        }
    }

    /// Validates if a command string is safe to execute.
    pub fn is_safe(&self, command: &str) -> bool {
        for pattern in &self.dangerous_patterns {
            if pattern.is_match(command) {
                return false;
            }
        }
        true
    }

    /// Scans a directory for exposed secrets before an agent starts.
    pub fn preflight_check(&self, path: &Path) -> Vec<String> {
        self.preflight_report(path)
            .into_iter()
            .map(|f| {
                let icon = if f.blocking { "✗" } else { "⚠" };
                format!("{icon} {}: {}", f.label, f.detail)
            })
            .collect()
    }

    /// Run a lightweight project scan before starting an interactive agent
    /// session. Blocking findings should stop the run entirely; warnings are
    /// informational only.
    pub fn preflight_report(&self, path: &Path) -> Vec<PreflightFinding> {
        let mut findings = Vec::new();
        if !path.exists() {
            findings.push(PreflightFinding {
                label: "Project path",
                detail: format!("{} does not exist", path.display()),
                blocking: true,
            });
            return findings;
        }
        if !path.is_dir() {
            findings.push(PreflightFinding {
                label: "Project path",
                detail: format!("{} is not a directory", path.display()),
                blocking: true,
            });
            return findings;
        }

        let dot_env = path.join(".env");
        if dot_env.exists() {
            findings.push(PreflightFinding {
                label: ".env present",
                detail: format!("sensitive file in workspace: {}", dot_env.display()),
                blocking: false,
            });
        }

        let report = crate::security::scan_project_fast(path);
        let high_count = report
            .issues
            .iter()
            .filter(|i| {
                matches!(
                    i.severity,
                    crate::security::Severity::High | crate::security::Severity::Critical
                )
            })
            .count();
        if high_count > 0 {
            findings.push(PreflightFinding {
                label: "Security scan",
                detail: format!(
                    "{} HIGH/CRITICAL finding(s) in project scan — run `raios security` first",
                    high_count
                ),
                blocking: true,
            });
        } else if !report.issues.is_empty() {
            findings.push(PreflightFinding {
                label: "Security scan",
                detail: format!("{} low/medium finding(s) detected", report.issues.len()),
                blocking: false,
            });
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_report_warns_when_env_file_exists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".env"), "API_KEY=test").unwrap();

        let shield = AgentShield::init();
        let findings = shield.preflight_report(tmp.path());

        assert!(findings.iter().any(|f| f.label == ".env present" && !f.blocking));
    }

    #[test]
    fn preflight_report_blocks_missing_path() {
        let shield = AgentShield::init();
        let findings = shield.preflight_report(Path::new("/tmp/raios-missing-preflight-path"));

        assert!(findings.iter().any(|f| f.label == "Project path" && f.blocking));
    }
}
