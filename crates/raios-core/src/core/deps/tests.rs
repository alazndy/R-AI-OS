use super::android::count_catalog_versions;
use super::common::cvss_to_severity;
use super::ios::parse_package_resolved;
use super::*;

#[test]
fn cvss_to_severity_ranges() {
    assert_eq!(cvss_to_severity("9.8"), "critical");
    assert_eq!(cvss_to_severity("7.5"), "high");
    assert_eq!(cvss_to_severity("5.0"), "medium");
    assert_eq!(cvss_to_severity("2.0"), "low");
    assert_eq!(cvss_to_severity(""), "unknown");
    assert_eq!(cvss_to_severity("abc"), "unknown");
}

#[test]
fn check_rust_project_runs_without_panic() {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !root.join("Cargo.lock").exists() {
        if let Some(p) = root.parent() {
            if let Some(gp) = p.parent() {
                if gp.join("Cargo.lock").exists() {
                    root = gp.to_path_buf();
                }
            }
        }
    }
    let r = check(&root);
    assert_eq!(r.project_type, "Rust");
    assert!(r.has_lockfile, "Cargo.lock should exist");
    // cargo-audit veya cargo-outdated yüklü değilse tool_missing'e düşer, panic olmaz
}

#[test]
fn check_unknown_project() {
    let tmp = std::env::temp_dir().join("raios_deps_unknown");
    let _ = std::fs::create_dir_all(&tmp);
    let r = check(&tmp);
    assert_eq!(r.project_type, "Unknown");
    assert!(!r.tool_missing.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn count_version_catalog_entries_basic() {
    let toml = "[versions]\nkotlin = \"2.0.0\"\ncompose = \"1.7.8\"\nretrofit = \"2.11.0\"\n\n[libraries]\nretrofit-core = { group = \"com.squareup\", version.ref = \"retrofit\" }\n";
    let count = count_catalog_versions(toml);
    assert_eq!(count, 3);
}

#[test]
fn count_version_catalog_no_versions_section() {
    let toml = "[libraries]\nsome = \"x:y:1.0\"\n";
    let count = count_catalog_versions(toml);
    assert_eq!(count, 0);
}

#[test]
fn check_android_finds_version_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("gradle")).unwrap();
    std::fs::write(
        tmp.path().join("gradle/libs.versions.toml"),
        "[versions]\nkotlin = \"2.0.0\"\ncompose = \"1.7.8\"\n",
    )
    .unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    let report = android::check_android(tmp.path());
    assert_eq!(report.project_type, "Android");
    assert!(report.has_lockfile);
    assert_eq!(report.outdated_count, 2);
    assert!(report.tool_missing.iter().any(|m| m.contains("OWASP")));
}

#[test]
fn parse_package_resolved_v2() {
    let json = r#"{"pins":[{"identity":"swift-argument-parser","location":"https://github.com/apple/swift-argument-parser","state":{"version":"1.3.0"}}],"version":2}"#;
    let deps = parse_package_resolved(json);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "swift-argument-parser");
    assert_eq!(deps[0].current, "1.3.0");
}

#[test]
fn parse_package_resolved_v1() {
    let json = r#"{"object":{"pins":[{"package":"Alamofire","state":{"version":"5.8.1"}}]}}"#;
    let deps = parse_package_resolved(json);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "Alamofire");
    assert_eq!(deps[0].current, "5.8.1");
}

#[test]
fn parse_package_resolved_empty_json() {
    let deps = parse_package_resolved("{}");
    assert_eq!(deps.len(), 0);
}
