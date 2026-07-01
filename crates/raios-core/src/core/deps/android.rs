use super::common::DepsReport;
use std::path::Path;

pub fn check_android(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Android");

    let catalog_path = dir.join("gradle").join("libs.versions.toml");
    if catalog_path.exists() {
        report.has_lockfile = true;
        if let Ok(content) = std::fs::read_to_string(&catalog_path) {
            report.outdated_count = count_catalog_versions(&content);
        }
    }

    report
        .tool_missing
        .push("OWASP CVE scan: add `id 'org.owasp.dependencycheck'` plugin to build.gradle".into());
    report
}

/// Count entries in the [versions] section of a Gradle version catalog TOML.
/// Counts non-comment lines containing `=` while inside the [versions] section.
pub(crate) fn count_catalog_versions(toml: &str) -> usize {
    let mut in_versions = false;
    let mut count = 0;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[versions]" {
            in_versions = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_versions = false;
        }
        if in_versions && trimmed.contains('=') && !trimmed.starts_with('#') {
            count += 1;
        }
    }
    count
}
