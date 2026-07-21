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
