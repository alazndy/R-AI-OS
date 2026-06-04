use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{failed_result, failed_test, BuildResult, TestResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedKind {
    EspIdf,
    PlatformIo,
    Arduino,
}

pub fn detect_embedded_kind(dir: &Path) -> Option<EmbeddedKind> {
    if dir.join("idf.py").exists() {
        return Some(EmbeddedKind::EspIdf);
    }
    if dir.join("platformio.ini").exists() {
        return Some(EmbeddedKind::PlatformIo);
    }
    if std::fs::read_dir(dir).ok().is_some_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("ino"))
    }) {
        return Some(EmbeddedKind::Arduino);
    }
    None
}

fn parse_idf_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Project build complete") || output.contains("BUILD SUCCESSFUL");
    let errors = output
        .lines()
        .filter(|l| {
            (l.contains("error:") || l.starts_with("error:") || l.contains("CMake Error"))
                && !l.trim_start().starts_with("//")
        })
        .count();
    (ok, errors)
}

fn parse_pio_output(output: &str) -> (bool, usize) {
    let ok = output.contains("[SUCCESS]") && !output.contains("[FAILED]");
    let errors = if !ok { 1 } else { 0 };
    (ok, errors)
}

fn parse_arduino_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Sketch uses") || output.contains("used by sketch");
    let errors = if !ok && (output.contains("error:") || output.contains("Error:")) {
        1
    } else {
        0
    };
    (ok, errors)
}

pub fn parse_pio_test_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.trim_start().starts_with("PASSED") {
            let passed = line
                .split('(')
                .nth(1)
                .and_then(|s| s.split(' ').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1);
            return (passed, 0);
        }
        if line.trim_start().starts_with("FAILED") {
            return (0, 1);
        }
    }
    (0, 0)
}

pub fn build_embedded(dir: &Path) -> BuildResult {
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => build_esp_idf(dir),
        Some(EmbeddedKind::PlatformIo) => build_platformio(dir),
        Some(EmbeddedKind::Arduino) => build_arduino(dir),
        None => failed_result(
            "Embedded",
            "—",
            std::time::Duration::ZERO,
            "No embedded toolchain found (idf.py, platformio.ini, or *.ino)".into(),
        ),
    }
}

fn build_esp_idf(dir: &Path) -> BuildResult {
    let cmd_str = "idf.py build";
    let start = Instant::now();
    let output = Command::new("idf.py").arg("build").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/ESP-IDF", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_idf_build_output(&raw);
            BuildResult {
                ok,
                project_type: "Embedded/ESP-IDF".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

fn build_platformio(dir: &Path) -> BuildResult {
    let cmd_str = "pio run";
    let start = Instant::now();
    let output = Command::new("pio").arg("run").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/PlatformIO", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_pio_output(&raw);
            BuildResult {
                ok,
                project_type: "Embedded/PlatformIO".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

fn build_arduino(dir: &Path) -> BuildResult {
    let fqbn = "arduino:avr:uno";
    let cmd_str = format!("arduino-cli compile --fqbn {}", fqbn);
    let start = Instant::now();
    let output = Command::new("arduino-cli")
        .args(["compile", "--fqbn", fqbn])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/Arduino", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_arduino_output(&raw);
            BuildResult {
                ok,
                project_type: "Embedded/Arduino".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_embedded(dir: &Path) -> TestResult {
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => {
            let cmd_str = "idf.py build -T test";
            let start = Instant::now();
            let out = Command::new("idf.py")
                .args(["build", "-T", "test"])
                .current_dir(dir)
                .output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("Embedded/ESP-IDF", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let ok = o.status.success();
                    TestResult {
                        ok,
                        project_type: "Embedded/ESP-IDF".into(),
                        command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64,
                        passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 },
                        ignored: 0,
                        failures: vec![],
                        raw_output: raw,
                    }
                }
            }
        }
        Some(EmbeddedKind::PlatformIo) => {
            let cmd_str = "pio test";
            let start = Instant::now();
            let out = Command::new("pio").arg("test").current_dir(dir).output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("Embedded/PlatformIO", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let (passed, failed) = parse_pio_test_output(&raw);
                    TestResult {
                        ok: o.status.success(),
                        project_type: "Embedded/PlatformIO".into(),
                        command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64,
                        passed,
                        failed,
                        ignored: 0,
                        failures: vec![],
                        raw_output: raw,
                    }
                }
            }
        }
        Some(EmbeddedKind::Arduino) | None => TestResult {
            ok: false,
            project_type: "Embedded/Arduino".into(),
            command: "—".into(),
            duration_ms: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            failures: vec![
                "Arduino-CLI does not support unit testing; use PlatformIO or ESP-IDF".into(),
            ],
            raw_output: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_idf_build_success() {
        let output = "Project build complete. To flash, run this command:\nidf.py -p PORT flash";
        let (ok, errors) = parse_idf_build_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_idf_build_failure() {
        let output = "error: 'undefined_var' undeclared\nCMake Error at CMakeLists.txt:10\nninja: build stopped: subcommand failed.";
        let (ok, errors) = parse_idf_build_output(output);
        assert!(!ok);
        assert!(errors >= 1);
    }

    #[test]
    fn parse_pio_build_success() {
        let output = "Environment esp32dev    [SUCCESS]\n=== 1 succeeded in 00:23.456 ===";
        let (ok, _) = parse_pio_output(output);
        assert!(ok);
    }

    #[test]
    fn parse_pio_build_failure() {
        let output = "Environment esp32dev    [FAILED]\n=== 0 succeeded in 00:05.123, 1 failed ===";
        let (ok, _) = parse_pio_output(output);
        assert!(!ok);
    }

    #[test]
    fn parse_pio_test_all_pass() {
        let output =
            "PASSED (1 test, 5 assertions)\nTest      Assertions  Passed  Failed\nexample   5           5       0";
        let (passed, failed) = parse_pio_test_output(output);
        assert!(passed >= 1);
        assert_eq!(failed, 0);
    }

    #[test]
    fn detect_embedded_kind_idf() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("idf.py")).unwrap();
        assert_eq!(detect_embedded_kind(tmp.path()), Some(EmbeddedKind::EspIdf));
    }

    #[test]
    fn detect_embedded_kind_platformio() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("platformio.ini"), "[env]\n").unwrap();
        assert_eq!(
            detect_embedded_kind(tmp.path()),
            Some(EmbeddedKind::PlatformIo)
        );
    }

    #[test]
    fn detect_embedded_kind_arduino() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("sketch.ino"), "void setup(){}\nvoid loop(){}\n").unwrap();
        assert_eq!(detect_embedded_kind(tmp.path()), Some(EmbeddedKind::Arduino));
    }

    #[test]
    fn detect_embedded_kind_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_embedded_kind(tmp.path()), None);
    }
}
