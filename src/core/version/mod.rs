mod changelog;
mod semver;
pub mod types;

pub use types::*;

use changelog::{build_changelog_entry, count_commits_since_tag, last_git_tag, prepend_changelog};
use semver::{bump_semver, read_version, write_version};
use std::path::Path;
use std::process::Command;

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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::semver::*;

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
        assert!(
            content.contains("versionCode 43"),
            "versionCode not updated: {content}"
        );
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
        assert!(looks_like_semver("2.0.0-alpha"));
        assert!(looks_like_semver("1.2.3+7"));
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
        assert!(
            content.contains("version: 1.0.1+2"),
            "version not updated: {content}"
        );
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
        assert_eq!(
            extract_plist_key(content, "CFBundleShortVersionString"),
            None
        );
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
        )
        .unwrap();
        write_ios_version(tmp.path(), "1.0.0", "1.0.1").unwrap();
        let content = fs::read_to_string(tmp.path().join("Info.plist")).unwrap();
        assert!(
            content.contains("<string>1.0.1</string>"),
            "not updated: {content}"
        );
        assert!(
            !content.contains("<string>1.0.0</string>"),
            "old version still present"
        );
    }

    #[test]
    fn write_ios_version_missing_plist_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let result = write_ios_version(tmp.path(), "1.0.0", "1.0.1");
        assert!(result.is_err());
    }

    #[test]
    fn read_dotnet_version_from_csproj() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("MyApp.csproj"),
            "<Project>\n  <PropertyGroup>\n    <Version>3.1.4</Version>\n  </PropertyGroup>\n</Project>\n",
        ).unwrap();
        assert_eq!(read_dotnet_version(tmp.path()), Some("3.1.4".to_string()));
    }

    #[test]
    fn write_dotnet_version_updates_csproj() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("App.csproj"),
            "<Project>\n  <Version>1.0.0</Version>\n</Project>\n",
        )
        .unwrap();
        write_dotnet_version(tmp.path(), "1.0.0", "1.0.1").unwrap();
        let content = fs::read_to_string(tmp.path().join("App.csproj")).unwrap();
        assert!(content.contains("<Version>1.0.1</Version>"));
        assert!(!content.contains("<Version>1.0.0</Version>"));
    }

    #[test]
    fn read_cpp_cmake_version_from_cmakelists() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("CMakeLists.txt"),
            "project(MyLib VERSION 2.5.0 LANGUAGES CXX)\n",
        )
        .unwrap();
        assert_eq!(
            read_cpp_cmake_version(tmp.path()),
            Some("2.5.0".to_string())
        );
    }

    #[test]
    fn read_iac_version_from_terraform_required_version() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("main.tf"),
            "terraform {\n  required_version = \">= 1.7.0\"\n}\n",
        )
        .unwrap();
        assert_eq!(read_iac_version(tmp.path()), Some("1.7.0".to_string()));
    }

    #[test]
    fn read_iac_version_from_docker_compose() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("docker-compose.yml"),
            "version: \"3.8\"\nservices:\n  app:\n    image: nginx\n",
        )
        .unwrap();
        assert_eq!(read_iac_version(tmp.path()), Some("3.8".to_string()));
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
