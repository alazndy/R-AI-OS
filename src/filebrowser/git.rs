use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct RecentProject {
    pub name: String,
    pub rel_path: String,
    pub changes: Vec<String>,
    pub git_dirty: Option<bool>,
    pub git_branch: Option<String>,
}

pub fn load_recent_projects(base: &Path) -> Vec<RecentProject> {
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

    let walker = walkdir::WalkDir::new(base)
        .max_depth(5)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
                && name != "node_modules"
                && name != "target"
                && name != "dist"
                && name != ".next"
        });

    for entry in walker.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy() == "memory.md" {
            let t = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((entry.path().to_path_buf(), t));
        }
    }

    files.sort_by_key(|a| Reverse(a.1));

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
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let changes = extract_changes(&path);
            let project_dir = path.parent().unwrap_or(&path).to_path_buf();
            let git_dirty = git_is_dirty(&project_dir);
            let git_branch = git_current_branch(&project_dir);
            RecentProject {
                name,
                rel_path: rel,
                changes,
                git_dirty,
                git_branch,
            }
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
            } else if (line.starts_with("##") || line.starts_with("# "))
                && !line.contains("Claude")
                && !changes.is_empty()
            {
                break;
            }
        }
    }
    changes
}

pub fn git_get_remote_url(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    } else {
        None
    }
}

pub fn git_is_dirty(dir: &Path) -> Option<bool> {
    let out = Command::new("git")
        .args(["status", "--short"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        Some(!out.stdout.is_empty())
    } else {
        None
    }
}

fn git_current_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() {
            None
        } else {
            Some(branch)
        }
    } else {
        None
    }
}

pub fn get_git_log(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["log", "--oneline", "-20", "--no-color"])
        .current_dir(dir)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::to_owned)
            .collect(),
        _ => vec!["(not a git repo or no history)".into()],
    }
}

#[derive(Debug)]
pub struct GitCommitResult {
    pub committed: bool,
    pub pushed: bool,
    pub message: String,
}

pub fn git_commit(dir: &Path, msg: &str) -> GitCommitResult {
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output();

    if add.map(|o| !o.status.success()).unwrap_or(true) {
        return GitCommitResult {
            committed: false,
            pushed: false,
            message: "git add failed".into(),
        };
    }

    let commit = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .output();

    match commit {
        Ok(o) if o.status.success() => GitCommitResult {
            committed: true,
            pushed: false,
            message: "ok".into(),
        },
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let nothing_to_commit =
                stderr.contains("nothing to commit") || stderr.contains("nothing added");
            if nothing_to_commit {
                GitCommitResult {
                    committed: false,
                    pushed: false,
                    message: "nothing to commit".into(),
                }
            } else {
                GitCommitResult {
                    committed: false,
                    pushed: false,
                    message: stderr,
                }
            }
        }
        Err(e) => GitCommitResult {
            committed: false,
            pushed: false,
            message: e.to_string(),
        },
    }
}

pub fn git_push(dir: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["push", "origin", "HEAD"])
        .current_dir(dir)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
