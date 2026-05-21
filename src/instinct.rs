use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct InstinctData {
    pub session_count: u64,
    pub preferences: HashMap<String, String>,
    pub learned_rules: Vec<String>,
}

pub struct InstinctEngine {
    path: PathBuf,
    pub data: InstinctData,
}

impl InstinctEngine {
    pub fn init() -> Self {
        let home = dirs::home_dir().expect("Home dir not found");
        let path = home.join(".agents").join("instincts.json");

        let data = if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            InstinctData::default()
        };

        Self { path, data }
    }

    pub fn add_rule(&mut self, rule: String) {
        if !self.data.learned_rules.contains(&rule) {
            self.data.learned_rules.push(rule);
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    /// Generates a system prompt snippet based on learned instincts.
    pub fn get_instinct_prompt(&self) -> String {
        if self.data.learned_rules.is_empty() {
            return String::new();
        }
        let mut prompt = String::from("\n[RAIOS INSTINCTS - GLOBAL LEARNINGS]\n");
        for rule in &self.data.learned_rules {
            prompt.push_str(&format!("- {}\n", rule));
        }
        prompt
    }
}

pub fn suggest_from_health(health: &crate::health::ProjectHealth) -> Vec<String> {
    let mut suggestions = Vec::new();

    if matches!(health.refactor_grade.as_str(), "D" | "F") {
        suggestions.push(format!(
            "Refactor grade {} — high nesting/unwrap chains, clean before new features",
            health.refactor_grade
        ));
    }
    if health.security_critical > 0 {
        suggestions.push(format!(
            "Has {} CRITICAL security issue(s) — run `raios security` before commit",
            health.security_critical
        ));
    }
    if !health.has_memory {
        suggestions.push("No memory.md — add one to track decisions and learnings".into());
    }
    if !health.has_sigmap {
        suggestions.push("No SIGMAP.md — run sigmap to generate context map".into());
    }
    if health.git_dirty == Some(true) {
        suggestions.push("Uncommitted changes detected — commit before context switch".into());
    }
    if health.constitution_issues.len() > 2 {
        suggestions.push(format!(
            "Multiple constitution violations ({}) — review MASTER.md",
            health.constitution_issues.len()
        ));
    }
    suggestions
}

/// Generate instinct suggestions from a SUCCESSFUL Factory job.
/// Returns 0-2 short rule strings based on heuristic pattern matching.
pub fn suggest_from_outcome(description: &str, command: &str, result: &str) -> Vec<String> {
    if result.trim().is_empty() {
        return vec![];
    }

    let desc_lower = description.to_lowercase();
    let result_lower = result.to_lowercase();
    let mut suggestions = Vec::new();

    // Test pass pattern
    if command.contains("test") || desc_lower.contains("test") {
        let has_failures = result_lower.contains("failed") && !result_lower.contains("0 failed");
        if result_lower.contains("passed") && !has_failures {
            suggestions.push(format!(
                "Test suite passes for '{}' — keep TDD discipline before adding features",
                truncate(&desc_lower, 40)
            ));
        }
    }

    // Build success pattern
    if (command.contains("build") || command.contains("cargo build") || command.contains("cargo check"))
        && !result_lower.contains("error") {
            suggestions.push(format!(
                "'{}' builds cleanly — run `cargo check` before submitting PRs",
                truncate(&desc_lower, 40)
            ));
        }

    // Security scan pattern
    if desc_lower.contains("security") || command.contains("security") {
        suggestions.push(
            "Security scan succeeded — run `raios security` before every commit".to_string()
        );
    }

    suggestions.truncate(2);
    suggestions
}

/// Generate instinct suggestions from a FAILED Factory job.
pub fn suggest_from_failure(description: &str, _command: &str, error: &str) -> Vec<String> {
    if error.trim().is_empty() {
        return vec![];
    }

    let error_lower = error.to_lowercase();
    let desc_lower = description.to_lowercase();
    let mut suggestions = Vec::new();

    if error_lower.contains("mismatched types") || error_lower.contains("e0308") {
        suggestions.push(
            "Type mismatch errors — run `cargo check` after every refactor, not just at the end"
                .to_string(),
        );
    }

    if error_lower.contains("borrow") || error_lower.contains("lifetime") {
        suggestions.push(
            "Borrow checker failure — prefer cloning over fighting the borrow checker in hot paths"
                .to_string(),
        );
    }

    if error_lower.contains("permission denied") || error_lower.contains("access is denied") {
        suggestions.push(format!(
            "'{}' failed with permission error — check file locks before running shell commands",
            truncate(&desc_lower, 40)
        ));
    }

    if error_lower.contains("connection refused") || error_lower.contains("failed to connect") {
        suggestions.push(
            "Connection failure — ensure aiosd daemon is running before agent-to-daemon tasks"
                .to_string(),
        );
    }

    suggestions.truncate(2);
    suggestions
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

pub fn append_to_memory_md(project_path: &std::path::Path, rule: &str) -> anyhow::Result<()> {
    let memory_path = project_path.join("memory.md");
    if !memory_path.exists() {
        anyhow::bail!("memory.md not found at {}", memory_path.display());
    }

    let content = std::fs::read_to_string(&memory_path)?;
    if content.contains(rule) {
        return Ok(());
    }

    let new_content = if content.contains("## Instincts") {
        let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
        let insert_pos = lines
            .iter()
            .position(|l| l.trim() == "## Instincts")
            .map(|p| p + 1)
            .unwrap_or(lines.len());
        lines.insert(insert_pos, format!("- {}", rule));
        let joined = lines.join("\n");
        if content.ends_with('\n') {
            format!("{}\n", joined)
        } else {
            joined
        }
    } else {
        format!(
            "{}\n## Instincts\n- {}\n",
            content.trim_end_matches('\n'),
            rule
        )
    };

    std::fs::write(&memory_path, new_content)?;
    Ok(())
}

pub fn load_project_rules(project_path: &std::path::Path) -> Vec<String> {
    let memory_path = project_path.join("memory.md");
    let content = match std::fs::read_to_string(&memory_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut in_section = false;
    let mut rules = Vec::new();
    for line in content.lines() {
        if line.trim() == "## Instincts" {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") {
                break;
            }
            if let Some(rule) = line.trim().strip_prefix("- ") {
                rules.push(rule.to_string());
            }
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::ProjectHealth;
    use tempfile::TempDir;

    fn make_bad_health(tmp: &TempDir) -> ProjectHealth {
        ProjectHealth {
            name: "test-proj".into(),
            path: tmp.path().to_path_buf(),
            status: "active".into(),
            git_dirty: Some(true),
            remote_url: None,
            compliance_score: Some(40),
            compliance_grade: "D".into(),
            has_memory: false,
            has_sigmap: false,
            constitution_issues: vec!["pnpm".into(), "rls".into(), "api_key".into()],
            graphify_done: false,
            graph_report: None,
            security_score: Some(50),
            security_grade: Some("C".into()),
            security_issue_count: 3,
            security_critical: 2,
            refactor_score: 40,
            refactor_grade: "F".into(),
            refactor_high_count: 5,
            refactor_medium_count: 3,
            ci_status: None,
            ci_url: None,
        }
    }

    #[test]
    fn suggest_from_health_returns_suggestions_for_bad_project() {
        let tmp = TempDir::new().unwrap();
        let health = make_bad_health(&tmp);
        let suggestions = suggest_from_health(&health);
        assert!(
            !suggestions.is_empty(),
            "Expected suggestions for bad project"
        );
        assert!(suggestions.iter().any(|s| s.contains("Refactor")));
    }

    #[test]
    fn append_to_memory_md_creates_instincts_section() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("memory.md"),
            "# Project Memory\n\n## Notes\n- note\n",
        )
        .unwrap();
        append_to_memory_md(tmp.path(), "Never use malloc here").unwrap();
        let content = std::fs::read_to_string(tmp.path().join("memory.md")).unwrap();
        assert!(content.contains("## Instincts"));
        assert!(content.contains("Never use malloc here"));
    }

    #[test]
    fn append_to_memory_md_does_not_duplicate() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("memory.md"), "# Memory\n").unwrap();
        append_to_memory_md(tmp.path(), "No duplicates rule").unwrap();
        append_to_memory_md(tmp.path(), "No duplicates rule").unwrap();
        let content = std::fs::read_to_string(tmp.path().join("memory.md")).unwrap();
        assert_eq!(content.matches("No duplicates rule").count(), 1);
    }

    #[test]
    fn successful_cargo_test_job_suggests_tdd_rule() {
        let suggestions = suggest_from_outcome(
            "run tests for auth module",
            "cargo test",
            "117 passed; 0 failed",
        );
        assert!(!suggestions.is_empty());
        let combined = suggestions.join(" ").to_lowercase();
        assert!(combined.contains("test") || combined.contains("pass"));
    }

    #[test]
    fn failed_job_suggests_investigation_rule() {
        let suggestions = suggest_from_failure(
            "refactor auth module",
            "cargo check",
            "error[E0308]: mismatched types",
        );
        assert!(!suggestions.is_empty());
        let combined = suggestions.join(" ").to_lowercase();
        assert!(combined.contains("type") || combined.contains("error") || combined.contains("check"));
    }

    #[test]
    fn empty_result_produces_no_suggestions() {
        let suggestions = suggest_from_outcome("task", "cmd", "");
        assert!(suggestions.is_empty());
    }
}
