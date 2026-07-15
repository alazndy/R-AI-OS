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

pub fn load_file_content(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| format!("# Error\n\nCould not read:\n  {}\n\n{}", path.display(), e))
}

pub fn save_file_content(path: &Path, content: &str) -> std::io::Result<()> {
    raios_core::safe_io::safe_write(path, content).map_err(std::io::Error::other)
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

/// Writes `new_content` to `path`, first backing up any existing content to
/// `<path>.bak.<unix_timestamp>` and pruning backups beyond the 5 most recent.
/// Used exclusively for constitution files (global + per-project) so a bad
/// edit to the single file every agent reads is always recoverable.
pub fn save_constitution_file(path: &Path, new_content: &str) -> std::io::Result<()> {
    if path.exists() {
        let existing = fs::read_to_string(path)?;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let backup_path = PathBuf::from(format!("{}.bak.{}", path.display(), ts));
        fs::write(&backup_path, existing)?;
        prune_old_backups(path)?;
    }
    fs::write(path, new_content)
}

fn prune_old_backups(path: &Path) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let prefix = format!("{}.bak.", file_name);

    let mut backups: Vec<(u128, PathBuf)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ts_str = name.strip_prefix(&prefix)?;
            let ts: u128 = ts_str.parse().ok()?;
            Some((ts, e.path()))
        })
        .collect();

    backups.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    for (_, old_path) in backups.into_iter().skip(5) {
        let _ = fs::remove_file(old_path);
    }
    Ok(())
}

#[cfg(test)]
mod constitution_save_tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "raios-save-test-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("CONSTITUTION.md")
    }

    #[test]
    fn first_save_with_no_prior_file_creates_no_backup() {
        let path = temp_file("first");
        save_constitution_file(&path, "## 1. Hello\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "## 1. Hello\n");
        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert!(backups.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn second_save_backs_up_the_previous_content() {
        let path = temp_file("second");
        save_constitution_file(&path, "version one\n").unwrap();
        save_constitution_file(&path, "version two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "version two\n");

        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 1);
        let backup_content = std::fs::read_to_string(backups[0].path()).unwrap();
        assert_eq!(backup_content, "version one\n");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keeps_only_five_most_recent_backups() {
        let path = temp_file("prune");
        for i in 0..7 {
            save_constitution_file(&path, &format!("version {}\n", i)).unwrap();
            // Force distinct backup timestamps even on fast filesystems/CI.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 5, "expected exactly 5 backups, found {}", backups.len());
        std::fs::remove_dir_all(dir).ok();
    }
}
