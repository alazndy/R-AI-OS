use std::path::Path;
use std::process::Command;

pub(super) fn build_changelog_entry(dir: &Path, version: &str, since_tag: Option<&str>) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let commits = git_log_since(dir, since_tag);

    let mut feats = Vec::new();
    let mut fixes = Vec::new();
    let mut chores = Vec::new();
    let mut others = Vec::new();

    for c in &commits {
        let lower = c.to_lowercase();
        if lower.starts_with("feat") {
            feats.push(c);
        } else if lower.starts_with("fix") {
            fixes.push(c);
        } else if lower.starts_with("chore")
            || lower.starts_with("refactor")
            || lower.starts_with("docs")
        {
            chores.push(c);
        } else {
            others.push(c);
        }
    }

    let mut entry = format!("## v{} — {}\n", version, date);
    if !feats.is_empty() {
        entry.push_str("### Features\n");
        for c in &feats {
            entry.push_str(&format!("- {}\n", c));
        }
    }
    if !fixes.is_empty() {
        entry.push_str("### Fixes\n");
        for c in &fixes {
            entry.push_str(&format!("- {}\n", c));
        }
    }
    if !chores.is_empty() {
        entry.push_str("### Chore\n");
        for c in &chores {
            entry.push_str(&format!("- {}\n", c));
        }
    }
    if !others.is_empty() {
        entry.push_str("### Other\n");
        for c in &others {
            entry.push_str(&format!("- {}\n", c));
        }
    }
    if commits.is_empty() {
        entry.push_str("- (no commits since last tag)\n");
    }

    entry
}

pub(super) fn prepend_changelog(dir: &Path, entry: &str) {
    let path = dir.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = if existing.starts_with("# Changelog") {
        let rest = existing
            .strip_prefix("# Changelog")
            .unwrap_or(&existing)
            .trim_start();
        format!("# Changelog\n\n{}\n{}", entry, rest)
    } else {
        format!("# Changelog\n\n{}\n{}", entry, existing)
    };
    let _ = std::fs::write(&path, updated);
}

pub(super) fn git_log_since(dir: &Path, since_tag: Option<&str>) -> Vec<String> {
    let range = since_tag
        .map(|t| format!("{}..HEAD", t))
        .unwrap_or_else(|| "HEAD".into());
    Command::new("git")
        .args(["log", &range, "--pretty=format:%s", "--no-color"])
        .current_dir(dir)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn last_git_tag(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .current_dir(dir)
        .output()
        .ok()?;
    if out.status.success() {
        let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if tag.is_empty() {
            None
        } else {
            Some(tag)
        }
    } else {
        None
    }
}

pub(super) fn count_commits_since_tag(dir: &Path, tag: Option<&str>) -> usize {
    let range = tag
        .map(|t| format!("{}..HEAD", t))
        .unwrap_or_else(|| "HEAD".into());
    Command::new("git")
        .args(["rev-list", "--count", &range])
        .current_dir(dir)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}
