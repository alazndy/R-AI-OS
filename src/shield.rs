use regex_lite::Regex;
use std::path::Path;

pub struct AgentShield {
    dangerous_patterns: Vec<Regex>,
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
        let mut warnings = Vec::new();
        let dot_env = path.join(".env");
        if dot_env.exists() {
            warnings.push(format!("⚠️ Found sensitive file: {}", dot_env.display()));
        }

        // Add more checks (hardcoded keys etc via existing security.rs logic if needed)
        warnings
    }
}
