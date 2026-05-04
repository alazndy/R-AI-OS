use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: &'static str,
    pub path: PathBuf,
}

impl AgentInfo {
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub category: &'static str,
}

pub fn discover_agents() -> Vec<AgentInfo> {
    let h = dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\turha"));
    let npm = h.join("AppData/Roaming/npm");

    vec![
        AgentInfo { name: "Claude Code", path: npm.join("claude.cmd") },
        AgentInfo { name: "Gemini CLI", path: npm.join("gemini.cmd") },
        AgentInfo {
            name: "Antigravity (Cursor)",
            path: h.join("AppData/Local/Programs/cursor/Cursor.exe"),
        },
    ]
}

pub fn discover_skills() -> Vec<SkillInfo> {
    let base = PathBuf::from(r"C:\Users\turha\Desktop\Dev Ops\AI OS\System\.agents\skills");
    let mut skills = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                skills.push(SkillInfo { name, category: "Advanced" });
            } else if name.ends_with(".md") {
                skills.push(SkillInfo {
                    name: name.trim_end_matches(".md").to_string(),
                    category: "Global",
                });
            }
        }
    }
    skills
}

pub fn open_in_editor(path: &PathBuf) -> anyhow::Result<()> {
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &path.to_string_lossy()])
        .spawn()?;
    Ok(())
}
