use std::path::Path;
use std::process::Command;
use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BumpType {
    Major,
    Minor,
    Patch,
}

impl BumpType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "patch" => Some(Self::Patch),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current: String,
    pub project_type: String,
    pub version_file: String,
    pub last_tag: Option<String>,
    pub commits_since_tag: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BumpResult {
    pub ok: bool,
    pub old_version: String,
    pub new_version: String,
    pub version_file: String,
    pub changelog_entry: String,
    pub message: String,
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn info(dir: &Path) -> Option<VersionInfo> {
    let (current, project_type, version_file) = read_version(dir)?;
    let last_tag = last_git_tag(dir);
    let commits_since_tag = count_commits_since_tag(dir, last_tag.as_deref());
    Some(VersionInfo { current, project_type, version_file, last_tag, commits_since_tag })
}

pub fn bump(dir: &Path, bump_type: &BumpType, update_changelog: bool, tag: bool) -> BumpResult {
    let Some((current, project_type, version_file)) = read_version(dir) else {
        return BumpResult {
            ok: false,
            old_version: String::new(),
            new_version: String::new(),
            version_file: String::new(),
            changelog_entry: String::new(),
            message: "Cannot detect version file (Cargo.toml / package.json / pyproject.toml)".into(),
        };
    };

    let Some(new_version) = bump_semver(&current, bump_type) else {
        return BumpResult {
            ok: false,
            old_version: current,
            new_version: String::new(),
            version_file,
            changelog_entry: String::new(),
            message: "Cannot parse current version as semver (expected X.Y.Z)".into(),
        };
    };

    let vfile_path = dir.join(&version_file);
    if let Err(e) = write_version(&vfile_path, &project_type, &current, &new_version) {
        return BumpResult {
            ok: false, old_version: current, new_version,
            version_file, changelog_entry: String::new(),
            message: format!("Failed to write version: {}", e),
        };
    }

    let last_tag = last_git_tag(dir);
    let changelog_entry = build_changelog_entry(dir, &new_version, last_tag.as_deref());

    if update_changelog {
        prepend_changelog(dir, &changelog_entry);
    }

    if tag {
        let tag_name = format!("v{}", new_version);
        let _ = Command::new("git")
            .args(["tag", "-a", &tag_name, "-m", &format!("Release {}", tag_name)])
            .current_dir(dir)
            .output();
    }

    BumpResult {
        ok: true,
        old_version: current,
        new_version,
        version_file,
        changelog_entry,
        message: "ok".into(),
    }
}

pub fn changelog(dir: &Path) -> String {
    let version = read_version(dir)
        .map(|(v, _, _)| v)
        .unwrap_or_else(|| "unreleased".into());
    let last_tag = last_git_tag(dir);
    build_changelog_entry(dir, &version, last_tag.as_deref())
}

// ─── Semver ──────────────────────────────────────────────────────────────────

fn bump_semver(version: &str, bump: &BumpType) -> Option<String> {
    let clean = version.trim_start_matches('v');
    let parts: Vec<u64> = clean.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() < 3 { return None; }
    let (major, minor, patch) = (parts[0], parts[1], parts[2]);
    Some(match bump {
        BumpType::Major => format!("{}.0.0", major + 1),
        BumpType::Minor => format!("{}.{}.0", major, minor + 1),
        BumpType::Patch => format!("{}.{}.{}", major, minor, patch + 1),
    })
}

// ─── Version file readers / writers ──────────────────────────────────────────

fn read_version(dir: &Path) -> Option<(String, String, String)> {
    if let Some(v) = read_cargo_version(dir)    { return Some((v, "Rust".into(),   "Cargo.toml".into())); }
    if let Some(v) = read_npm_version(dir)      { return Some((v, "Node".into(),   "package.json".into())); }
    if let Some(v) = read_pyproject_version(dir){ return Some((v, "Python".into(), "pyproject.toml".into())); }
    None
}

fn read_cargo_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("version") && line.contains('=') {
            let val = line.split('=').nth(1)?.trim().trim_matches('"').to_string();
            if looks_like_semver(&val) { return Some(val); }
        }
    }
    None
}

fn read_npm_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v["version"].as_str().map(str::to_string)
}

fn read_pyproject_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("pyproject.toml")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("version") && line.contains('=') {
            let val = line.split('=').nth(1)?.trim().trim_matches('"').to_string();
            if looks_like_semver(&val) { return Some(val); }
        }
    }
    None
}

fn write_version(path: &Path, project_type: &str, old: &str, new: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let updated = if project_type == "Node" {
        let mut v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        v["version"] = serde_json::Value::String(new.to_string());
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    } else {
        // Replace first occurrence of the quoted version string
        content.replacen(&format!("\"{}\"", old), &format!("\"{}\"", new), 1)
    };
    std::fs::write(path, updated).map_err(|e| e.to_string())
}

fn looks_like_semver(s: &str) -> bool {
    s.split('.').count() == 3 && s.split('.').all(|p| p.parse::<u64>().is_ok())
}

// ─── Changelog ───────────────────────────────────────────────────────────────

fn build_changelog_entry(dir: &Path, version: &str, since_tag: Option<&str>) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let commits = git_log_since(dir, since_tag);

    let mut feats  = Vec::new();
    let mut fixes  = Vec::new();
    let mut chores = Vec::new();
    let mut others = Vec::new();

    for c in &commits {
        let lower = c.to_lowercase();
        if lower.starts_with("feat")     { feats.push(c); }
        else if lower.starts_with("fix") { fixes.push(c); }
        else if lower.starts_with("chore") || lower.starts_with("refactor") || lower.starts_with("docs") {
            chores.push(c);
        }
        else { others.push(c); }
    }

    let mut entry = format!("## v{} — {}\n", version, date);
    if !feats.is_empty()  { entry.push_str("### Features\n");  for c in &feats  { entry.push_str(&format!("- {}\n", c)); } }
    if !fixes.is_empty()  { entry.push_str("### Fixes\n");     for c in &fixes  { entry.push_str(&format!("- {}\n", c)); } }
    if !chores.is_empty() { entry.push_str("### Chore\n");     for c in &chores { entry.push_str(&format!("- {}\n", c)); } }
    if !others.is_empty() { entry.push_str("### Other\n");     for c in &others { entry.push_str(&format!("- {}\n", c)); } }
    if commits.is_empty() { entry.push_str("- (no commits since last tag)\n"); }

    entry
}

fn prepend_changelog(dir: &Path, entry: &str) {
    let path = dir.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = if existing.starts_with("# Changelog") {
        let rest = existing["# Changelog".len()..].trim_start();
        format!("# Changelog\n\n{}\n{}", entry, rest)
    } else {
        format!("# Changelog\n\n{}\n{}", entry, existing)
    };
    let _ = std::fs::write(&path, updated);
}

fn git_log_since(dir: &Path, since_tag: Option<&str>) -> Vec<String> {
    let range = since_tag
        .map(|t| format!("{}..HEAD", t))
        .unwrap_or_else(|| "HEAD".into());
    Command::new("git")
        .args(["log", &range, "--pretty=format:%s", "--no-color"])
        .current_dir(dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
        .unwrap_or_default()
}

fn last_git_tag(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .current_dir(dir)
        .output().ok()?;
    if out.status.success() {
        let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if tag.is_empty() { None } else { Some(tag) }
    } else {
        None
    }
}

fn count_commits_since_tag(dir: &Path, tag: Option<&str>) -> usize {
    let range = tag.map(|t| format!("{}..HEAD", t)).unwrap_or_else(|| "HEAD".into());
    Command::new("git")
        .args(["rev-list", "--count", &range])
        .current_dir(dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0))
        .unwrap_or(0)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_patch() {
        assert_eq!(bump_semver("1.2.3", &BumpType::Patch), Some("1.2.4".into()));
    }

    #[test]
    fn bump_minor_resets_patch() {
        assert_eq!(bump_semver("1.2.3", &BumpType::Minor), Some("1.3.0".into()));
    }

    #[test]
    fn bump_major_resets_all() {
        assert_eq!(bump_semver("1.2.3", &BumpType::Major), Some("2.0.0".into()));
    }

    #[test]
    fn invalid_semver_returns_none() {
        assert_eq!(bump_semver("not-a-version", &BumpType::Patch), None);
        assert_eq!(bump_semver("1.2", &BumpType::Patch), None);
    }

    #[test]
    fn looks_like_semver_checks() {
        assert!(looks_like_semver("1.2.3"));
        assert!(looks_like_semver("0.0.1"));
        assert!(!looks_like_semver("1.2"));
        assert!(!looks_like_semver("abc"));
    }

    #[test]
    fn info_reads_raios_version() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let v = info(&root).expect("should find Cargo.toml");
        assert_eq!(v.project_type, "Rust");
        assert!(looks_like_semver(&v.current));
    }

    #[test]
    fn changelog_entry_has_version() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let entry = changelog(&root);
        assert!(entry.contains("## v"));
        assert!(entry.contains("202")); // year
    }

    #[test]
    fn bump_type_from_str() {
        assert_eq!(BumpType::from_str("patch"), Some(BumpType::Patch));
        assert_eq!(BumpType::from_str("MINOR"), Some(BumpType::Minor));
        assert_eq!(BumpType::from_str("xyz"),   None);
    }
}
