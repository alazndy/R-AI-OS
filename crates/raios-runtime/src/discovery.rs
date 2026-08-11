use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: &'static str,
    pub path: PathBuf,
    pub exists: bool,
}

impl AgentInfo {
    pub fn exists(&self) -> bool {
        self.exists
    }
}

/// Populated by `discover_skills` but currently only `name`/`category` are
/// ever displayed (see TUI skills panel). `description` carries a real
/// (if generic) value; `version`/`is_active` are hardcoded constants at
/// every call site today, not actually-discovered metadata — kept for a
/// UI that shows per-skill detail, not yet built.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub category: &'static str,
    #[allow(dead_code)]
    pub description: String,
    #[allow(dead_code)]
    pub version: String,
    #[allow(dead_code)]
    pub is_active: bool,
}

pub fn discover_agents() -> Vec<AgentInfo> {
    let h = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let npm = h.join("AppData/Roaming/npm");

    let c_path = npm.join("claude.cmd");
    let o_path = raios_core::core::process::resolve_command_path("opencode")
        .unwrap_or_else(|| PathBuf::from("opencode"));
    let a_path = h.join("AppData/Local/Programs/cursor/Cursor.exe");

    vec![
        AgentInfo {
            name: "Claude Code",
            exists: c_path.exists(),
            path: c_path,
        },
        AgentInfo {
            name: "OpenCode",
            exists: raios_core::core::process::resolve_command_path("opencode").is_some(),
            path: o_path,
        },
        AgentInfo {
            name: "Antigravity (Cursor)",
            exists: a_path.exists(),
            path: a_path,
        },
    ]
}

/// skills_path comes from config.
pub fn discover_skills(skills_path: &Path) -> Vec<SkillInfo> {
    let mut skills = Vec::new();

    if let Ok(entries) = std::fs::read_dir(skills_path) {
        scan_dir_for_skills(entries, "Local", &mut skills);
    }

    skills
}

fn scan_dir_for_skills(entries: std::fs::ReadDir, cat: &'static str, skills: &mut Vec<SkillInfo>) {
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            let desc = if path.join("SKILL.md").exists() {
                "Self-contained agent skill folder"
            } else {
                "Custom local skill"
            };

            skills.push(SkillInfo {
                name,
                category: cat,
                description: desc.to_string(),
                version: "1.0.0".to_string(),
                is_active: true,
            });
        } else if name.ends_with(".md") {
            skills.push(SkillInfo {
                name: name.trim_end_matches(".md").to_string(),
                category: cat,
                description: "Global context/instruction file".to_string(),
                version: "1.0.0".to_string(),
                is_active: true,
            });
        }
    }
}

pub fn open_in_editor(path: &Path) -> anyhow::Result<()> {
    raios_core::core::process::open_in_system_editor(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_skills_finds_directories_and_markdown_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("my-skill")).unwrap();
        fs::create_dir_all(dir.path().join("full-skill")).unwrap();
        fs::write(dir.path().join("full-skill").join("SKILL.md"), "# skill").unwrap();
        fs::write(dir.path().join("rules.md"), "# rules").unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

        let skills = discover_skills(dir.path());
        let mut names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["full-skill", "my-skill", "rules"]);

        let full = skills.iter().find(|s| s.name == "full-skill").unwrap();
        assert_eq!(full.description, "Self-contained agent skill folder");
        let bare = skills.iter().find(|s| s.name == "my-skill").unwrap();
        assert_eq!(bare.description, "Custom local skill");
        let md = skills.iter().find(|s| s.name == "rules").unwrap();
        assert_eq!(md.description, "Global context/instruction file");
        assert!(skills.iter().all(|s| s.category == "Local"));
        assert!(skills.iter().all(|s| s.version == "1.0.0"));
        assert!(skills.iter().all(|s| s.is_active));
    }

    #[test]
    fn discover_skills_ignores_missing_path() {
        let skills = discover_skills(Path::new("/nonexistent/raios/skills/path"));
        assert!(skills.is_empty());
    }

    #[test]
    fn discover_skills_skips_non_skill_extensions() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "readme").unwrap();
        fs::write(dir.path().join("data.json"), "{}").unwrap();
        fs::write(dir.path().join("script.sh"), "#!/bin/sh").unwrap();
        let skills = discover_skills(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "README");
    }

    #[test]
    fn discover_skills_treats_nested_subdirs_as_skills() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("nested/skill/sub")).unwrap();
        let skills = discover_skills(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "nested");
    }

    #[test]
    fn discover_agents_returns_three_standard_entries() {
        let agents = discover_agents();
        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].name, "Claude Code");
        assert_eq!(agents[1].name, "OpenCode");
        assert_eq!(agents[2].name, "Antigravity (Cursor)");
    }
}
