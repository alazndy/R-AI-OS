use std::path::Path;
use std::process::Command;
use super::common::{DepsReport, OutdatedDep};

pub fn check_ios(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("iOS");

    if let Ok(content) = std::fs::read_to_string(dir.join("Package.resolved")) {
        report.has_lockfile = true;
        let deps = parse_package_resolved(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }

    if dir.join("Podfile.lock").exists() {
        report.has_lockfile = true;
    }

    if Command::new("xcodebuild").arg("-version").output().is_err()
        && Command::new("swift").arg("--version").output().is_err()
    {
        report.tool_missing.push(
            "xcodebuild or swift (requires macOS with Xcode or Swift toolchain installed)".into(),
        );
    }

    report
}

pub(crate) fn parse_package_resolved(content: &str) -> Vec<OutdatedDep> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
        return vec![];
    };
    let pins = v["pins"]
        .as_array()
        .or_else(|| v["object"]["pins"].as_array());
    let Some(pins) = pins else {
        return vec![];
    };
    pins.iter()
        .filter_map(|pin| {
            let name = pin["identity"]
                .as_str()
                .or_else(|| pin["package"].as_str())?
                .to_string();
            let version = pin["state"]["version"].as_str().unwrap_or("?").to_string();
            Some(OutdatedDep {
                name,
                current: version,
                latest: "?".into(),
                kind: "spm".into(),
            })
        })
        .collect()
}
