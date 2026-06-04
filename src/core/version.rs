use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BumpType {
    Major,
    Minor,
    Patch,
}

impl BumpType {
    pub fn parse(s: &str) -> Option<Self> {
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
    Some(VersionInfo {
        current,
        project_type,
        version_file,
        last_tag,
        commits_since_tag,
    })
}

pub fn bump(dir: &Path, bump_type: &BumpType, update_changelog: bool, tag: bool) -> BumpResult {
    let Some((current, project_type, version_file)) = read_version(dir) else {
        return BumpResult {
            ok: false,
            old_version: String::new(),
            new_version: String::new(),
            version_file: String::new(),
            changelog_entry: String::new(),
            message: "Cannot detect version file (Cargo.toml / package.json / pyproject.toml)"
                .into(),
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
            ok: false,
            old_version: current,
            new_version,
            version_file,
            changelog_entry: String::new(),
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
            .args([
                "tag",
                "-a",
                &tag_name,
                "-m",
                &format!("Release {}", tag_name),
            ])
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
    if parts.len() < 3 {
        return None;
    }
    let (major, minor, patch) = (parts[0], parts[1], parts[2]);
    Some(match bump {
        BumpType::Major => format!("{}.0.0", major + 1),
        BumpType::Minor => format!("{}.{}.0", major, minor + 1),
        BumpType::Patch => format!("{}.{}.{}", major, minor, patch + 1),
    })
}

// ─── Version file readers / writers ──────────────────────────────────────────

fn read_version(dir: &Path) -> Option<(String, String, String)> {
    if let Some(v) = read_cargo_version(dir) {
        return Some((v, "Rust".into(), "Cargo.toml".into()));
    }
    if let Some(v) = read_npm_version(dir) {
        return Some((v, "Node".into(), "package.json".into()));
    }
    if let Some(v) = read_pyproject_version(dir) {
        return Some((v, "Python".into(), "pyproject.toml".into()));
    }
    if let Some((v, _build)) = read_flutter_version(dir) {
        return Some((v, "Flutter".into(), "pubspec.yaml".into()));
    }
    if let Some(v) = read_ios_version(dir) {
        return Some((v, "iOS".into(), "Info.plist".into()));
    }
    if let Some(v) = read_embedded_version(dir) {
        return Some((v, "Embedded".into(), "version.h / CMakeLists.txt".into()));
    }
    if let Some((name, _code)) = read_android_version(dir) {
        return Some((name, "Android".into(), "app/build.gradle".into()));
    }
    None
}

fn read_cargo_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("version") && line.contains('=') {
            let val = line.split('=').nth(1)?.trim().trim_matches('"').to_string();
            if looks_like_semver(&val) {
                return Some(val);
            }
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
            if looks_like_semver(&val) {
                return Some(val);
            }
        }
    }
    None
}

/// Read version from embedded projects: version.h (#define APP_VERSION), CMakeLists.txt (VERSION), platformio.ini.
pub(crate) fn read_embedded_version(dir: &Path) -> Option<String> {
    for candidate in &["version.h", "src/version.h", "main/version.h", "include/version.h"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with("#define") && (t.contains("VERSION") || t.contains("version")) {
                    let parts: Vec<&str> = t.splitn(3, ' ').collect();
                    if parts.len() == 3 {
                        let val = parts[2].trim().trim_matches('"').trim_matches('\'');
                        if looks_like_semver(val) {
                            return Some(val.to_string());
                        }
                    }
                }
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(dir.join("CMakeLists.txt")) {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("project(") && t.contains("VERSION") {
                let after = t.split("VERSION").nth(1).unwrap_or("").trim();
                let version = after.split([' ', ')', '\n']).next().unwrap_or("").trim();
                if looks_like_semver(version) {
                    return Some(version.to_string());
                }
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(dir.join("platformio.ini")) {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("version") && t.contains('=') {
                let val = t.split('=').nth(1).unwrap_or("").trim().trim_matches('"');
                if looks_like_semver(val) {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Read (versionName, versionCode) from app/build.gradle (Groovy DSL).
pub(crate) fn read_android_version(dir: &Path) -> Option<(String, u64)> {
    let content = std::fs::read_to_string(dir.join("app").join("build.gradle")).ok()?;
    let name = parse_version_name(&content)?;
    let code = parse_version_code(&content)?;
    Some((name, code))
}

fn parse_version_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("versionName") {
            let val = trimmed
                .trim_start_matches("versionName")
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            if looks_like_semver(val) {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_version_code(content: &str) -> Option<u64> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("versionCode") {
            let val = trimmed.trim_start_matches("versionCode").trim();
            if let Ok(n) = val.parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

/// Read (semver, build_number) from pubspec.yaml.
/// Format: `version: 1.2.3+7` — strips the `+N` build suffix for the semver component.
fn read_flutter_version(dir: &Path) -> Option<(String, u64)> {
    let content = std::fs::read_to_string(dir.join("pubspec.yaml")).ok()?;
    parse_pubspec_version(&content)
}

fn parse_pubspec_version(content: &str) -> Option<(String, u64)> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version:") {
            let val = trimmed
                .trim_start_matches("version:")
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            if let Some(plus_pos) = val.find('+') {
                let semver = &val[..plus_pos];
                let build: u64 = val[plus_pos + 1..].parse().unwrap_or(0);
                if looks_like_semver(semver) {
                    return Some((semver.to_string(), build));
                }
            } else if looks_like_semver(val) {
                return Some((val.to_string(), 0));
            }
        }
    }
    None
}

/// Write new version into pubspec.yaml, incrementing the build number.
pub(crate) fn write_flutter_version(dir: &Path, new_version: &str, new_build: u64) -> Result<(), String> {
    let path = dir.join("pubspec.yaml");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("version:") {
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}version: {new_version}+{new_build}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let final_content = if content.ends_with('\n') {
        format!("{updated}\n")
    } else {
        updated
    };
    std::fs::write(&path, final_content).map_err(|e| e.to_string())
}

/// Read CFBundleShortVersionString from Info.plist (checks common locations).
pub(crate) fn read_ios_version(dir: &Path) -> Option<String> {
    for candidate in &["Info.plist", "Sources/Info.plist", "App/Info.plist", "Resources/Info.plist"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            if let Some(v) = extract_plist_key(&content, "CFBundleShortVersionString") {
                if looks_like_semver(&v) {
                    return Some(v);
                }
            }
        }
    }
    None
}

pub(crate) fn extract_plist_key(content: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{}</key>", key);
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.trim() == key_tag {
            if let Some(value_line) = lines.next() {
                let trimmed = value_line.trim();
                if trimmed.starts_with("<string>") && trimmed.ends_with("</string>") {
                    return Some(
                        trimmed
                            .trim_start_matches("<string>")
                            .trim_end_matches("</string>")
                            .to_string(),
                    );
                }
            }
        }
    }
    None
}

/// Update CFBundleShortVersionString in Info.plist.
pub(crate) fn write_ios_version(dir: &Path, old_version: &str, new_version: &str) -> Result<(), String> {
    for candidate in &["Info.plist", "Sources/Info.plist", "App/Info.plist"] {
        let path = dir.join(candidate);
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if !content.contains(&format!("<string>{}</string>", old_version)) {
            continue;
        }
        let updated = content.replace(
            &format!("<string>{}</string>", old_version),
            &format!("<string>{}</string>", new_version),
        );
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err(format!(
        "Info.plist with version {} not found in expected locations",
        old_version
    ))
}

/// Write new versionName and versionCode into app/build.gradle (line-by-line replacement).
pub(crate) fn write_android_version(dir: &Path, new_name: &str, new_code: u64) -> Result<(), String> {
    let path = dir.join("app").join("build.gradle");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("versionName") {
                let quote = if trimmed.contains('\'') { '\'' } else { '"' };
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}versionName {quote}{new_name}{quote}")
            } else if trimmed.starts_with("versionCode") {
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}versionCode {new_code}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let final_content = if content.ends_with('\n') {
        format!("{updated}\n")
    } else {
        updated
    };
    std::fs::write(&path, final_content).map_err(|e| e.to_string())
}

fn write_version(path: &Path, project_type: &str, old: &str, new: &str) -> Result<(), String> {
    if project_type == "Flutter" {
        let dir = path
            .parent()
            .ok_or_else(|| "Cannot resolve project dir from pubspec.yaml".to_string())?;
        let (_, old_build) = read_flutter_version(dir)
            .ok_or_else(|| "Cannot read current build number from pubspec.yaml".to_string())?;
        return write_flutter_version(dir, new, old_build + 1);
    }
    if project_type == "iOS" {
        let dir = path
            .parent()
            .ok_or_else(|| "Cannot resolve project dir from Info.plist".to_string())?;
        return write_ios_version(dir, old, new);
    }
    if project_type == "Android" {
        // path is app/build.gradle; dir is path.parent().parent()
        let dir = path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| "Cannot resolve project dir from app/build.gradle".to_string())?;
        let (_, old_code) = read_android_version(dir)
            .ok_or_else(|| "Cannot read current versionCode from app/build.gradle".to_string())?;
        return write_android_version(dir, new, old_code + 1);
    }
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

fn prepend_changelog(dir: &Path, entry: &str) {
    let path = dir.join("CHANGELOG.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = if existing.starts_with("# Changelog") {
        let rest = existing.strip_prefix("# Changelog").unwrap_or(&existing).trim_start();
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
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn last_git_tag(dir: &Path) -> Option<String> {
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

fn count_commits_since_tag(dir: &Path, tag: Option<&str>) -> usize {
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_android_version_parses_groovy_dsl() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        fs::write(
            tmp.path().join("app/build.gradle"),
            "android {\n    defaultConfig {\n        versionCode 42\n        versionName '4.2.15'\n    }\n}\n",
        )
        .unwrap();
        let (name, code) = read_android_version(tmp.path()).unwrap();
        assert_eq!(name, "4.2.15");
        assert_eq!(code, 42);
    }

    #[test]
    fn read_android_version_returns_none_if_no_app_gradle() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_android_version(tmp.path()).is_none());
    }

    #[test]
    fn write_android_version_updates_both_fields() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        fs::write(
            tmp.path().join("app/build.gradle"),
            "    versionCode 42\n    versionName '4.2.15'\n",
        )
        .unwrap();
        write_android_version(tmp.path(), "4.2.16", 43).unwrap();
        let content = fs::read_to_string(tmp.path().join("app/build.gradle")).unwrap();
        assert!(content.contains("versionCode 43"), "versionCode not updated: {content}");
        assert!(
            content.contains("versionName '4.2.16'"),
            "versionName not updated: {content}"
        );
    }

    #[test]
    fn write_android_version_preserves_other_content() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        let original = "android {\n    compileSdk 34\n    defaultConfig {\n        applicationId \"com.example\"\n        versionCode 10\n        versionName '1.0.0'\n        minSdk 26\n    }\n}\n";
        fs::write(tmp.path().join("app/build.gradle"), original).unwrap();
        write_android_version(tmp.path(), "1.0.1", 11).unwrap();
        let content = fs::read_to_string(tmp.path().join("app/build.gradle")).unwrap();
        assert!(content.contains("compileSdk 34"));
        assert!(content.contains("applicationId \"com.example\""));
        assert!(content.contains("versionCode 11"));
        assert!(content.contains("versionName '1.0.1'"));
        assert!(content.contains("minSdk 26"));
    }

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
    fn bump_type_parse() {
        assert_eq!(BumpType::parse("patch"), Some(BumpType::Patch));
        assert_eq!(BumpType::parse("MINOR"), Some(BumpType::Minor));
        assert_eq!(BumpType::parse("xyz"), None);
    }

    #[test]
    fn parse_pubspec_version_with_build_number() {
        let content = "name: my_app\nversion: 2.3.1+7\ndescription: test\n";
        let (semver, build) = parse_pubspec_version(content).unwrap();
        assert_eq!(semver, "2.3.1");
        assert_eq!(build, 7);
    }

    #[test]
    fn parse_pubspec_version_without_build_number() {
        let content = "name: my_app\nversion: 1.0.0\n";
        let (semver, build) = parse_pubspec_version(content).unwrap();
        assert_eq!(semver, "1.0.0");
        assert_eq!(build, 0);
    }

    #[test]
    fn parse_pubspec_version_invalid_returns_none() {
        assert!(parse_pubspec_version("name: my_app\n").is_none());
        assert!(parse_pubspec_version("version: not-semver+1\n").is_none());
        assert!(parse_pubspec_version("version: 1.2\n").is_none());
    }

    #[test]
    fn write_flutter_version_updates_version_line() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("pubspec.yaml"),
            "name: my_app\nversion: 1.0.0+1\nsdkVersion: '>=3.0.0'\n",
        )
        .unwrap();
        write_flutter_version(tmp.path(), "1.0.1", 2).unwrap();
        let content = fs::read_to_string(tmp.path().join("pubspec.yaml")).unwrap();
        assert!(content.contains("version: 1.0.1+2"), "version not updated: {content}");
    }

    #[test]
    fn extract_plist_key_finds_value() {
        let content = "<key>CFBundleShortVersionString</key>\n<string>1.2.3</string>";
        assert_eq!(
            extract_plist_key(content, "CFBundleShortVersionString"),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn extract_plist_key_returns_none_for_missing_key() {
        let content = "<key>OtherKey</key>\n<string>value</string>";
        assert_eq!(extract_plist_key(content, "CFBundleShortVersionString"), None);
    }

    #[test]
    fn read_ios_version_from_info_plist() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("Info.plist"),
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n<key>CFBundleShortVersionString</key>\n<string>2.4.1</string>\n</dict>\n</plist>",
        ).unwrap();
        assert_eq!(read_ios_version(tmp.path()), Some("2.4.1".to_string()));
    }

    #[test]
    fn write_ios_version_updates_plist() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("Info.plist"),
            "<key>CFBundleShortVersionString</key>\n<string>1.0.0</string>\n",
        ).unwrap();
        write_ios_version(tmp.path(), "1.0.0", "1.0.1").unwrap();
        let content = fs::read_to_string(tmp.path().join("Info.plist")).unwrap();
        assert!(content.contains("<string>1.0.1</string>"), "not updated: {content}");
        assert!(!content.contains("<string>1.0.0</string>"), "old version still present");
    }

    #[test]
    fn write_ios_version_missing_plist_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let result = write_ios_version(tmp.path(), "1.0.0", "1.0.1");
        assert!(result.is_err());
    }

    #[test]
    fn read_embedded_version_from_version_h() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("main")).unwrap();
        fs::write(
            tmp.path().join("main/version.h"),
            "#pragma once\n#define APP_VERSION \"1.3.0\"\n",
        )
        .unwrap();
        assert_eq!(read_embedded_version(tmp.path()), Some("1.3.0".to_string()));
    }

    #[test]
    fn read_embedded_version_from_cmake() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.16)\nproject(my_app VERSION 2.0.1)\n",
        )
        .unwrap();
        assert_eq!(read_embedded_version(tmp.path()), Some("2.0.1".to_string()));
    }

    #[test]
    fn read_embedded_version_from_platformio_ini() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("platformio.ini"),
            "[env:esp32dev]\nplatform = espressif32\nversion = 0.4.2\n",
        )
        .unwrap();
        assert_eq!(read_embedded_version(tmp.path()), Some("0.4.2".to_string()));
    }

    #[test]
    fn write_flutter_version_preserves_other_content() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("pubspec.yaml"),
            "name: my_app\nversion: 2.0.0+5\ndescription: a flutter app\nenvironment:\n  sdk: '>=3.0.0'\n",
        )
        .unwrap();
        write_flutter_version(tmp.path(), "2.1.0", 6).unwrap();
        let content = fs::read_to_string(tmp.path().join("pubspec.yaml")).unwrap();
        assert!(content.contains("name: my_app"));
        assert!(content.contains("description: a flutter app"));
        assert!(content.contains("sdk: '>=3.0.0'"));
        assert!(content.contains("version: 2.1.0+6"));
    }
}
