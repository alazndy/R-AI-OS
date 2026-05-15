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
        if content.ends_with('\n') { format!("{}\n", joined) } else { joined }
    } else {
        format!("{}\n## Instincts\n- {}\n", content.trim_end_matches('\n'), rule)
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
            if line.starts_with("## ") { break; }
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
        }
    }

    #[test]
    fn suggest_from_health_returns_suggestions_for_bad_project() {
        let tmp = TempDir::new().unwrap();
        let health = make_bad_health(&tmp);
        let suggestions = suggest_from_health(&health);
        assert!(!suggestions.is_empty(), "Expected suggestions for bad project");
        assert!(suggestions.iter().any(|s| s.contains("Refactor")));
    }

    #[test]
    fn append_to_memory_md_creates_instincts_section() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("memory.md"),
            "# Project Memory\n\n## Notes\n- note\n",
        ).unwrap();
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
}
