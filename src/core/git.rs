use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub dirty: bool,
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogEntry {
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub diff_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOpResult {
    pub ok: bool,
    pub message: String,
}

impl GitOpResult {
    fn ok(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: msg.into(),
        }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: msg.into(),
        }
    }
}

// ─── Status ──────────────────────────────────────────────────────────────────

pub fn status(dir: &Path) -> GitStatus {
    let branch = current_branch(dir);
    let remote = remote_url(dir);
    let (ahead, behind) = ahead_behind(dir);

    let out = run_git(dir, &["status", "--short"]);
    let dirty = !out.trim().is_empty();

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    for line in out.lines() {
        if line.len() < 3 {
            continue;
        }
        let index = line.chars().next().unwrap_or(' ');
        let wt = line.chars().nth(1).unwrap_or(' ');
        let file = line[3..].to_string();

        if index != ' ' && index != '?' {
            staged.push(file.clone());
        }
        if wt == 'M' || wt == 'D' {
            unstaged.push(file.clone());
        }
        if index == '?' && wt == '?' {
            untracked.push(file);
        }
    }

    GitStatus {
        branch,
        remote,
        dirty,
        staged,
        unstaged,
        untracked,
        ahead,
        behind,
    }
}

// ─── Log ─────────────────────────────────────────────────────────────────────

pub fn log(dir: &Path, count: usize) -> Vec<GitLogEntry> {
    let n = count.to_string();
    let out = run_git(
        dir,
        &[
            "log",
            &format!("-{}", n),
            "--pretty=format:%h\x1f%s\x1f%an\x1f%ad",
            "--date=short",
            "--no-color",
        ],
    );

    out.lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '\x1f').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(GitLogEntry {
                short_hash: parts[0].to_string(),
                message: parts[1].to_string(),
                author: parts[2].to_string(),
                date: parts[3].to_string(),
            })
        })
        .collect()
}

// ─── Diff ────────────────────────────────────────────────────────────────────

pub fn diff(dir: &Path, staged: bool) -> GitDiffSummary {
    let args: &[&str] = if staged {
        &["diff", "--cached", "--stat", "--no-color"]
    } else {
        &["diff", "--stat", "--no-color"]
    };

    let stat = run_git(dir, args);

    let diff_args: &[&str] = if staged {
        &["diff", "--cached", "--no-color"]
    } else {
        &["diff", "--no-color"]
    };
    let diff_text = run_git(dir, diff_args);

    let (mut files, mut ins, mut del) = (0usize, 0usize, 0usize);
    for line in stat.lines() {
        if line.contains("changed") {
            // "3 files changed, 45 insertions(+), 12 deletions(-)"
            let nums: Vec<usize> = line
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if !nums.is_empty() {
                files = nums[0];
            }
            if nums.len() > 1 {
                ins = nums[1];
            }
            if nums.len() > 2 {
                del = nums[2];
            }
        }
    }

    GitDiffSummary {
        files_changed: files,
        insertions: ins,
        deletions: del,
        diff_text,
    }
}

// ─── Commit ──────────────────────────────────────────────────────────────────

pub fn commit(dir: &Path, msg: &str, add_all: bool) -> GitOpResult {
    if add_all {
        let add = Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir)
            .output();
        if add.map(|o| !o.status.success()).unwrap_or(true) {
            return GitOpResult::err("git add -A failed");
        }
    }

    let out = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let first_line = stdout.lines().next().unwrap_or("committed").to_string();
            GitOpResult::ok(first_line)
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if stderr.contains("nothing to commit") {
                GitOpResult::err("nothing to commit")
            } else {
                GitOpResult::err(stderr)
            }
        }
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

// ─── Push ────────────────────────────────────────────────────────────────────

pub fn push(dir: &Path) -> GitOpResult {
    let out = Command::new("git")
        .args(["push", "origin", "HEAD"])
        .current_dir(dir)
        .output();

    match out {
        Ok(o) if o.status.success() => GitOpResult::ok("pushed"),
        Ok(o) => GitOpResult::err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

// ─── Pull ────────────────────────────────────────────────────────────────────

pub fn pull(dir: &Path) -> GitOpResult {
    let out = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(dir)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let msg = String::from_utf8_lossy(&o.stdout).trim().to_string();
            GitOpResult::ok(if msg.is_empty() {
                "up to date".to_string()
            } else {
                msg
            })
        }
        Ok(o) => GitOpResult::err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

// ─── Branches ────────────────────────────────────────────────────────────────

pub fn branches(dir: &Path) -> Vec<GitBranch> {
    let out = run_git(dir, &["branch", "-a", "--no-color"]);
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let current = line.starts_with('*');
            let name = line.trim_start_matches('*').trim().to_string();
            let remote = name.starts_with("remotes/");
            let name = name.trim_start_matches("remotes/origin/").to_string();
            GitBranch {
                name,
                current,
                remote,
            }
        })
        .collect()
}

pub fn checkout(dir: &Path, branch: &str) -> GitOpResult {
    let out = Command::new("git")
        .args(["checkout", branch])
        .current_dir(dir)
        .output();

    match out {
        Ok(o) if o.status.success() => GitOpResult::ok(format!("switched to {}", branch)),
        Ok(o) => GitOpResult::err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

pub fn create_branch(dir: &Path, name: &str) -> GitOpResult {
    let out = Command::new("git")
        .args(["checkout", "-b", name])
        .current_dir(dir)
        .output();

    match out {
        Ok(o) if o.status.success() => GitOpResult::ok(format!("created and switched to {}", name)),
        Ok(o) => GitOpResult::err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn current_branch(dir: &Path) -> Option<String> {
    let out = run_git(dir, &["branch", "--show-current"]);
    let b = out.trim().to_string();
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}

pub fn remote_url(dir: &Path) -> Option<String> {
    let out = run_git(dir, &["remote", "get-url", "origin"]);
    let u = out.trim().to_string();
    if u.is_empty() {
        None
    } else {
        Some(u)
    }
}

pub fn is_dirty(dir: &Path) -> bool {
    !run_git(dir, &["status", "--short"]).trim().is_empty()
}

fn ahead_behind(dir: &Path) -> (usize, usize) {
    let out = run_git(
        dir,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    );
    let parts: Vec<&str> = out.split_whitespace().collect();
    if parts.len() == 2 {
        let ahead = parts[0].parse().unwrap_or(0);
        let behind = parts[1].parse().unwrap_or(0);
        (ahead, behind)
    } else {
        (0, 0)
    }
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn raios_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn status_returns_branch() {
        let s = status(&raios_root());
        assert!(s.branch.is_some(), "should detect branch in a git repo");
    }

    #[test]
    fn log_returns_entries() {
        let entries = log(&raios_root(), 5);
        assert!(!entries.is_empty(), "should have commit history");
        assert!(!entries[0].short_hash.is_empty());
        assert!(!entries[0].message.is_empty());
    }

    #[test]
    fn branches_contains_master_or_main() {
        let bs = branches(&raios_root());
        assert!(!bs.is_empty());
        let has_main = bs.iter().any(|b| b.name == "master" || b.name == "main");
        assert!(has_main);
    }

    #[test]
    fn diff_stat_on_clean_repo() {
        let d = diff(&raios_root(), false);
        // sadece struct dönüyor, panic etmemeli
        let _ = d.files_changed;
    }

    #[test]
    fn is_dirty_does_not_panic() {
        let _ = is_dirty(&raios_root());
    }
}
