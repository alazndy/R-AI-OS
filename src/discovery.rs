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

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub category: &'static str,
    #[allow(dead_code)] pub description: String,
    #[allow(dead_code)] pub version: String,
    #[allow(dead_code)] pub is_active: bool,
}

pub fn discover_agents() -> Vec<AgentInfo> {
    let h = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let npm = h.join("AppData/Roaming/npm");

    let c_path = npm.join("claude.cmd");
    let g_path = npm.join("gemini.cmd");
    let a_path = h.join("AppData/Local/Programs/cursor/Cursor.exe");

    vec![
        AgentInfo { name: "Claude Code", exists: c_path.exists(), path: c_path },
        AgentInfo { name: "Gemini CLI",  exists: g_path.exists(), path: g_path },
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

    // Also scan Antigravity's global skills if they exist
    if let Some(home) = dirs::home_dir() {
        let ag_skills = home.join(".gemini").join("antigravity").join("skills");
        if let Ok(entries) = std::fs::read_dir(ag_skills) {
            scan_dir_for_skills(entries, "Global AI", &mut skills);
        }
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
                is_active: true
            });
        } else if name.ends_with(".md") {
            skills.push(SkillInfo {
                name: name.trim_end_matches(".md").to_string(),
                category: cat,
                description: "Global context/instruction file".to_string(),
                version: "1.0.0".to_string(),
                is_active: true
            });
        }
    }
}

pub fn open_in_editor(path: &PathBuf) -> anyhow::Result<()> {
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &path.to_string_lossy()])
        .spawn()?;
    Ok(())
}
