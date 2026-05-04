use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub read_only: bool,
}

impl FileEntry {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self { name: name.into(), path: path.into(), read_only: false }
    }

    pub fn readonly(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\turha"))
}

fn dev_ops() -> PathBuf {
    PathBuf::from(r"C:\Users\turha\Desktop\Dev Ops")
}

pub fn get_master_rule_files() -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("CLAUDE.md (Global)", h.join("CLAUDE.md")),
        FileEntry::new("hardware-rules.md", h.join(".claude/rules/hardware-rules.md")),
        FileEntry::new("ui-rules.md", h.join(".claude/rules/ui-rules.md")),
        FileEntry::new("web-rules.md", h.join(".claude/rules/web-rules.md")),
    ]
}

pub fn get_agent_config_files() -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("GEMINI.md", h.join(".gemini/GEMINI.md")),
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")),
        FileEntry::new(
            "MASTER.md (Vault)",
            PathBuf::from(r"C:\Users\turha\Documents\Obsidian Vaults\Vault101\MASTER.md"),
        ),
        FileEntry::new("Claude hooks", h.join(".agents/hooks")),
    ]
}

pub fn get_policy_files() -> Vec<FileEntry> {
    let h = home();
    vec![
        FileEntry::new("AI OS Policy", h.join(".gemini/policies/ai-os-policy.toml")).readonly(),
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")).readonly(),
    ]
}

pub fn get_mempalace_files() -> Vec<FileEntry> {
    let d = dev_ops();
    let mut entries = vec![
        FileEntry::new("mempalace.yaml", d.join("mempalace.yaml")),
        FileEntry::new("entities.json", d.join("entities.json")),
    ];
    entries.extend(discover_memory_files(6));
    entries
}

pub fn discover_memory_files(limit: usize) -> Vec<FileEntry> {
    let base = dev_ops();
    let mut found: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in WalkDir::new(&base)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name().to_string_lossy().to_lowercase() == "memory.md" {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((entry.path().to_path_buf(), modified));
        }
    }

    found.sort_by(|a, b| b.1.cmp(&a.1));

    found
        .into_iter()
        .take(limit)
        .map(|(path, _)| {
            let proj = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            FileEntry::new(format!("{}/memory.md", proj), path)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct RecentProject {
    pub name: String,
    pub rel_path: String,
    pub changes: Vec<String>,
}

pub fn load_recent_projects() -> Vec<RecentProject> {
    let base = dev_ops();
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in WalkDir::new(&base)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name().to_string_lossy() == "memory.md" {
            let t = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((entry.path().to_path_buf(), t));
        }
    }

    files.sort_by(|a, b| b.1.cmp(&a.1));

    files
        .into_iter()
        .take(3)
        .map(|(path, _)| {
            let name = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(&base)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let changes = extract_changes(&path);
            RecentProject { name, rel_path: rel, changes }
        })
        .collect()
}

fn extract_changes(path: &PathBuf) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return vec![];
    };
    let mut changes = Vec::new();
    let mut collecting = false;

    for line in content.lines() {
        if line.contains("Yaptıkları") || line.contains("Claude") {
            collecting = true;
            continue;
        }
        if collecting {
            let trimmed = line.trim();
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                changes.push(trimmed[2..].to_string());
                if changes.len() >= 3 {
                    break;
                }
            } else if (line.starts_with("##") || line.starts_with("# ")) && !line.contains("Claude") {
                if !changes.is_empty() {
                    break;
                }
            }
        }
    }
    changes
}

pub fn load_file_content(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| format!("# Error\n\nCould not read:\n  {}\n\n{}", path.display(), e))
}

pub fn save_file_content(path: &PathBuf, content: &str) -> anyhow::Result<()> {
    fs::write(path, content)?;
    Ok(())
}

pub fn find_file_by_name(query: &str) -> Option<FileEntry> {
    let q = query.to_lowercase();
    let lists = [
        get_master_rule_files(),
        get_agent_config_files(),
        get_policy_files(),
        get_mempalace_files(),
    ];
    for list in &lists {
        for entry in list {
            if entry.name.to_lowercase().contains(&q) || entry.path.to_string_lossy().to_lowercase().contains(&q) {
                return Some(entry.clone());
            }
        }
    }
    let p = PathBuf::from(query);
    if p.exists() {
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        return Some(FileEntry::new(name, p));
    }
    None
}
