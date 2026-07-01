use super::common::{DepsReport, OutdatedDep};
use raios_core::core::build::embedded::{detect_embedded_kind, EmbeddedKind};
use std::path::Path;
use std::process::Command;

pub fn check_embedded(dir: &Path) -> DepsReport {
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => check_esp_idf_deps(dir),
        Some(EmbeddedKind::PlatformIo) => check_platformio_deps(dir),
        Some(EmbeddedKind::Arduino) | None => DepsReport::empty("Embedded/Arduino"),
    }
}

fn check_esp_idf_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Embedded/ESP-IDF");
    for candidate in &["idf_component.yml", "main/idf_component.yml"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            let deps = parse_idf_dependencies(&content);
            report.outdated_count = deps.len();
            report.outdated = deps;
            report.has_lockfile = true;
            break;
        }
    }
    if Command::new("idf.py").arg("--version").output().is_err() {
        report
            .tool_missing
            .push("idf.py (ESP-IDF not in PATH; source export.sh from ESP-IDF root)".into());
    }
    report
}

fn check_platformio_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Embedded/PlatformIO");
    report.has_lockfile = dir.join(".pio").join("libdeps").exists();
    if Command::new("pio").arg("--version").output().is_err() {
        report
            .tool_missing
            .push("pio (install: pip install platformio)".into());
    }
    report
}

pub(crate) fn parse_idf_dependencies(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        if line.trim() == "dependencies:" {
            in_deps = true;
            continue;
        }
        if in_deps {
            if !line.starts_with(' ') && !line.trim().is_empty() {
                break;
            }
            let trimmed = line.trim();
            if let Some((name, version)) = trimmed.split_once(':') {
                let name = name.trim().to_string();
                let version = version.trim().trim_matches('"').to_string();
                if name != "idf" && !name.is_empty() {
                    deps.push(OutdatedDep {
                        name,
                        current: version,
                        latest: "?".into(),
                        kind: "component".into(),
                    });
                }
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_idf_component_yml() {
        let content = "dependencies:\n  idf: \">=5.0\"\n  espressif/button: \"^2.0.0\"\n  esp_lcd_touch: \"1.0.0\"\n";
        let deps = parse_idf_dependencies(content);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.name == "espressif/button" && d.current == "^2.0.0"));
        assert!(deps
            .iter()
            .any(|d| d.name == "esp_lcd_touch" && d.current == "1.0.0"));
    }

    #[test]
    fn parse_idf_empty_deps_section() {
        let content = "dependencies:\n  idf: \">=5.0\"\n";
        let deps = parse_idf_dependencies(content);
        assert_eq!(deps.len(), 0);
    }
}
