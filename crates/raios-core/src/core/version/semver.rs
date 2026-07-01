use std::path::Path;

use super::types::BumpType;

// ─── Semver ──────────────────────────────────────────────────────────────────

pub(super) fn bump_semver(version: &str, bump: &BumpType) -> Option<String> {
    let (major, minor, patch) = parse_semver_triplet(version)?;
    Some(match bump {
        BumpType::Major => format!("{}.0.0", major + 1),
        BumpType::Minor => format!("{}.{}.0", major, minor + 1),
        BumpType::Patch => format!("{}.{}.{}", major, minor, patch + 1),
    })
}

// ─── Version file readers / writers ──────────────────────────────────────────

pub(super) fn read_version(dir: &Path) -> Option<(String, String, String)> {
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
    if let Some(v) = read_iac_version(dir) {
        return Some((v, "IaC".into(), "main.tf / docker-compose.yml".into()));
    }
    if let Some(v) = read_dotnet_version(dir) {
        return Some((v, ".NET".into(), "*.csproj".into()));
    }
    if let Some(v) = read_cpp_cmake_version(dir) {
        return Some((v, "C++".into(), "CMakeLists.txt".into()));
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

pub(super) fn read_dotnet_version(dir: &Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let t = line.trim();
                        if t.starts_with("<Version>") && t.ends_with("</Version>") {
                            let v = t
                                .trim_start_matches("<Version>")
                                .trim_end_matches("</Version>");
                            if looks_like_semver(v) {
                                return Some(v.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub(super) fn write_dotnet_version(
    dir: &Path,
    old_version: &str,
    new_version: &str,
) -> Result<(), String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let old_tag = format!("<Version>{}</Version>", old_version);
                let new_tag = format!("<Version>{}</Version>", new_version);
                if content.contains(&old_tag) {
                    std::fs::write(&path, content.replace(&old_tag, &new_tag))
                        .map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
    }
    Err(format!(
        "No *.csproj with <Version>{}</Version> found",
        old_version
    ))
}

pub(super) fn read_cpp_cmake_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("CMakeLists.txt")).ok()?;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("project(") && t.contains("VERSION") {
            let after = t.split("VERSION").nth(1)?.trim();
            let version = after.split([' ', ')', '\n']).next()?.trim();
            if looks_like_semver(version) {
                return Some(version.to_string());
            }
        }
    }
    None
}

pub(super) fn read_iac_version(dir: &Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("tf") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let t = line.trim();
                        if t.starts_with("required_version") && t.contains('=') {
                            let val = t
                                .split_once('=')
                                .map(|x| x.1)
                                .unwrap_or("")
                                .trim()
                                .trim_matches('"');
                            let version = val
                                .split_whitespace()
                                .last()
                                .unwrap_or("")
                                .trim_matches('"');
                            if looks_like_semver(version) {
                                return Some(version.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(dir.join("docker-compose.yml"))
        .or_else(|_| std::fs::read_to_string(dir.join("docker-compose.yaml")))
    {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("version:") {
                let val = t
                    .trim_start_matches("version:")
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub(super) fn read_embedded_version(dir: &Path) -> Option<String> {
    for candidate in &[
        "version.h",
        "src/version.h",
        "main/version.h",
        "include/version.h",
    ] {
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

pub(super) fn read_android_version(dir: &Path) -> Option<(String, u64)> {
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

pub(super) fn read_flutter_version(dir: &Path) -> Option<(String, u64)> {
    let content = std::fs::read_to_string(dir.join("pubspec.yaml")).ok()?;
    parse_pubspec_version(&content)
}

pub(super) fn parse_pubspec_version(content: &str) -> Option<(String, u64)> {
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

pub(super) fn write_flutter_version(
    dir: &Path,
    new_version: &str,
    new_build: u64,
) -> Result<(), String> {
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

pub(super) fn read_ios_version(dir: &Path) -> Option<String> {
    for candidate in &[
        "Info.plist",
        "Sources/Info.plist",
        "App/Info.plist",
        "Resources/Info.plist",
    ] {
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

pub(super) fn extract_plist_key(content: &str, key: &str) -> Option<String> {
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

pub(super) fn write_ios_version(
    dir: &Path,
    old_version: &str,
    new_version: &str,
) -> Result<(), String> {
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

pub(super) fn write_android_version(
    dir: &Path,
    new_name: &str,
    new_code: u64,
) -> Result<(), String> {
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

pub(super) fn write_version(path: &Path, project_type: &str, old: &str, new: &str) -> Result<(), String> {
    if project_type == "Flutter" {
        let dir = path
            .parent()
            .ok_or_else(|| "Cannot resolve project dir from pubspec.yaml".to_string())?;
        let (_, old_build) = read_flutter_version(dir)
            .ok_or_else(|| "Cannot read current build number from pubspec.yaml".to_string())?;
        return write_flutter_version(dir, new, old_build + 1);
    }
    if project_type == ".NET" {
        let dir = path
            .parent()
            .ok_or_else(|| "Cannot resolve project dir from *.csproj".to_string())?;
        return write_dotnet_version(dir, old, new);
    }
    if project_type == "iOS" {
        let dir = path
            .parent()
            .ok_or_else(|| "Cannot resolve project dir from Info.plist".to_string())?;
        return write_ios_version(dir, old, new);
    }
    if project_type == "Android" {
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
        content.replacen(&format!("\"{}\"", old), &format!("\"{}\"", new), 1)
    };
    std::fs::write(path, updated).map_err(|e| e.to_string())
}

pub(super) fn parse_semver_triplet(s: &str) -> Option<(u64, u64, u64)> {
    let clean = s.trim().trim_start_matches('v');
    let core = clean.split_once('-').map(|(head, _)| head).unwrap_or(clean);
    let core = core.split_once('+').map(|(head, _)| head).unwrap_or(core);

    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some((major, minor, patch))
}

pub(super) fn looks_like_semver(s: &str) -> bool {
    parse_semver_triplet(s).is_some()
}
