use std::path::Path;
use std::process::Command;
use super::common::{DepsReport, OutdatedDep};

pub fn check_flutter(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Flutter");
    let lock_path = dir.join("pubspec.lock");
    report.has_lockfile = lock_path.exists();

    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        let deps = parse_pubspec_lock(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }

    if Command::new("flutter").arg("--version").output().is_err() {
        report.tool_missing.push(
            "flutter (install from https://docs.flutter.dev/get-started/install)".into(),
        );
    }

    report
}

pub(crate) fn parse_pubspec_lock(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut in_packages = false;
    let mut current_name = String::new();
    let mut current_version = String::new();
    let mut is_sdk = false;

    for line in content.lines() {
        if line.trim() == "packages:" {
            in_packages = true;
            continue;
        }
        if !in_packages {
            continue;
        }
        // Top-level package entry: exactly 2-space indent + "name:"
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            if !current_name.is_empty() && !is_sdk && !current_version.is_empty() {
                deps.push(OutdatedDep {
                    name: current_name.clone(),
                    current: current_version.clone(),
                    latest: "?".into(),
                    kind: "direct".into(),
                });
            }
            current_name = line.trim().trim_end_matches(':').to_string();
            current_version.clear();
            is_sdk = false;
        }
        if line.trim_start().starts_with("version:") {
            current_version = line
                .split(':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if line.trim_start().starts_with("dependency:") && line.contains("sdk") {
            is_sdk = true;
        }
    }
    if !current_name.is_empty() && !is_sdk && !current_version.is_empty() {
        deps.push(OutdatedDep {
            name: current_name,
            current: current_version,
            latest: "?".into(),
            kind: "direct".into(),
        });
    }
    deps
}
