use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;
use super::FileEntry;

pub fn get_master_rule_files(master_md: &Path) -> Vec<FileEntry> {
    let h = super::home();
    vec![
        FileEntry::new("MASTER.md (Constitution)", master_md.to_path_buf()).readonly(),
        FileEntry::new("CLAUDE.md (Global)", h.join("CLAUDE.md")),
        FileEntry::new(
            "hardware-rules.md",
            h.join(".claude/rules/hardware-rules.md"),
        ),
        FileEntry::new("ui-rules.md", h.join(".claude/rules/ui-rules.md")),
        FileEntry::new("web-rules.md", h.join(".claude/rules/web-rules.md")),
    ]
}

pub fn get_agent_config_files() -> Vec<FileEntry> {
    let h = super::home();
    vec![
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")),
        FileEntry::new("Claude hooks", h.join(".agents/hooks")),
    ]
}

/// Policy files — includes MASTER.md whose path comes from config.
pub fn get_policy_files() -> Vec<FileEntry> {
    let h = super::home();
    vec![
        FileEntry::new("Claude settings.json", h.join(".claude/settings.json")).readonly(),
    ]
}

/// MemPalace files — dev_ops_path comes from config.
pub fn get_mempalace_files(dev_ops: &Path) -> Vec<FileEntry> {
    let mut entries = vec![
        FileEntry::new("mempalace.yaml", dev_ops.join("mempalace.yaml")),
        FileEntry::new("entities.json", dev_ops.join("entities.json")),
    ];
    entries.extend(discover_memory_files(dev_ops, 6));
    entries
}

pub fn discover_memory_files(base: &Path, limit: usize) -> Vec<FileEntry> {
    let skip_dirs: &[&str] = &[
        "node_modules", "target", "dist", "build", ".next", "__pycache__",
        "vendor", ".turbo", "out", "coverage", "cache", "tmp", "temp",
        "logs", "log", "runs", "test", "tests", "__tests__", "e2e",
        "fixtures", "examples", "samples", "gradle", ".gradle",
        "cmake-build-debug", "cmake-build-release", ".idea", ".vscode",
    ];

    let walker = WalkDir::new(base)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
            !skip_dirs.contains(&name.as_ref())
        });

    let mut found: Vec<(PathBuf, SystemTime)> = Vec::new();
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().to_lowercase() == "memory.md" {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((entry.path().to_path_buf(), modified));
        }
    }

    found.sort_by_key(|a| a.0.components().count());

    let mut accepted_dirs: Vec<PathBuf> = Vec::new();
    let mut deduped: Vec<(PathBuf, SystemTime)> = Vec::new();
    for (path, mtime) in found {
        let proj_dir = match path.parent() {
            Some(d) => d.to_path_buf(),
            None => continue,
        };
        let is_nested = accepted_dirs.iter().any(|a| proj_dir.starts_with(a));
        if !is_nested {
            accepted_dirs.push(proj_dir);
            deduped.push((path, mtime));
        }
    }

    deduped.sort_by_key(|a| Reverse(a.1));

    deduped
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

pub fn load_file_content(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| format!("# Error\n\nCould not read:\n  {}\n\n{}", path.display(), e))
}

pub fn save_file_content(path: &Path, content: &str) -> std::io::Result<()> {
    crate::safe_io::safe_write(path, content).map_err(std::io::Error::other)
}

pub fn find_file_by_name(query: &str, master_md: &Path) -> Option<FileEntry> {
    let q = query.to_lowercase();
    let lists = [
        get_master_rule_files(master_md),
        get_agent_config_files(),
        get_policy_files(),
    ];
    for list in &lists {
        for entry in list {
            if entry.name.to_lowercase().contains(&q)
                || entry.path.to_string_lossy().to_lowercase().contains(&q)
            {
                return Some(entry.clone());
            }
        }
    }
    let p = PathBuf::from(query);
    if p.exists() {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        return Some(FileEntry::new(name, p));
    }
    None
}
