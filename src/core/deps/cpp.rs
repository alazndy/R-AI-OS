use std::path::Path;
use std::process::Command;
use super::common::{DepsReport, OutdatedDep};

pub fn check_cpp(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("C++");
    if let Ok(content) = std::fs::read_to_string(dir.join("vcpkg.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(deps) = v["dependencies"].as_array() {
                report.outdated = deps
                    .iter()
                    .filter_map(|d| d.as_str().or_else(|| d["name"].as_str()))
                    .map(|name| OutdatedDep {
                        name: name.to_string(),
                        current: "?".into(),
                        latest: "?".into(),
                        kind: "vcpkg".into(),
                    })
                    .collect();
                report.outdated_count = report.outdated.len();
                report.has_lockfile = true;
            }
        }
    }
    if Command::new("cmake").arg("--version").output().is_err() {
        report
            .tool_missing
            .push("cmake (install from https://cmake.org/download)".into());
    }
    report
}
