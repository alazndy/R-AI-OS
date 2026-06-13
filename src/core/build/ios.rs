use super::common::{failed_result, failed_test, BuildResult, TestResult};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn parse_xcodebuild_output(output: &str) -> (bool, usize) {
    let ok = output.contains("** BUILD SUCCEEDED **");
    let errors = output
        .lines()
        .filter(|l| l.contains(": error:") && !l.trim_start().starts_with("//"))
        .count();
    (ok, errors)
}

pub fn parse_xcodebuild_warnings(output: &str) -> usize {
    output
        .lines()
        .filter(|l| l.contains(": warning:") && !l.trim_start().starts_with("//"))
        .count()
}

pub fn parse_xcodebuild_test_output(output: &str) -> (usize, usize) {
    let passed = output
        .lines()
        .filter(|l| l.contains("passed (") && l.contains("Test Case"))
        .count();
    let failed = output
        .lines()
        .filter(|l| l.contains("failed (") && l.contains("Test Case"))
        .count();
    (passed, failed)
}

pub fn build_ios(dir: &Path) -> BuildResult {
    build_ios_impl(dir, "iphonesimulator")
}

pub fn build_ios_release(dir: &Path) -> BuildResult {
    build_ios_impl(dir, "iphoneos")
}

pub fn build_ios_check(dir: &Path) -> BuildResult {
    if dir.join("Package.swift").exists() {
        let cmd_str = "swift build";
        let start = Instant::now();
        let out = Command::new("swift")
            .args(["build"])
            .current_dir(dir)
            .output();
        let elapsed = start.elapsed();
        return match out {
            Err(e) => failed_result("iOS", cmd_str, elapsed, e.to_string()),
            Ok(o) => {
                let raw = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                BuildResult {
                    ok: o.status.success(),
                    project_type: "iOS".into(),
                    command: cmd_str.into(),
                    duration_ms: elapsed.as_millis() as u64,
                    warnings: parse_xcodebuild_warnings(&raw),
                    errors: if o.status.success() { 0 } else { 1 },
                    diagnostics: vec![],
                    raw_output: raw,
                }
            }
        };
    }
    build_ios_impl(dir, "iphonesimulator")
}

fn build_ios_impl(dir: &Path, sdk: &str) -> BuildResult {
    let cmd_str = format!("xcodebuild -sdk {} build", sdk);
    let start = Instant::now();
    let output = Command::new("xcodebuild")
        .args(["-sdk", sdk, "build"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("iOS", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_xcodebuild_output(&raw);
            BuildResult {
                ok,
                project_type: "iOS".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                warnings: parse_xcodebuild_warnings(&raw),
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_ios(dir: &Path) -> TestResult {
    if dir.join("Package.swift").exists() {
        let cmd_str = "swift test";
        let start = Instant::now();
        let out = Command::new("swift")
            .args(["test"])
            .current_dir(dir)
            .output();
        let elapsed = start.elapsed();
        return match out {
            Err(e) => failed_test("iOS", cmd_str, elapsed, e.to_string()),
            Ok(o) => {
                let raw = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                let (passed, failed) = parse_xcodebuild_test_output(&raw);
                TestResult {
                    ok: o.status.success(),
                    project_type: "iOS".into(),
                    command: cmd_str.into(),
                    duration_ms: elapsed.as_millis() as u64,
                    passed,
                    failed,
                    ignored: 0,
                    failures: raw
                        .lines()
                        .filter(|l| l.contains("failed (") && l.contains("Test Case"))
                        .map(|l| l.trim().to_string())
                        .collect(),
                    raw_output: raw,
                }
            }
        };
    }
    let dest = "platform=iOS Simulator,name=iPhone 15";
    let cmd_str = format!("xcodebuild test -destination '{}'", dest);
    let start = Instant::now();
    let output = Command::new("xcodebuild")
        .args(["test", "-destination", dest])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_test("iOS", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_xcodebuild_test_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "iOS".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored: 0,
                failures: raw
                    .lines()
                    .filter(|l| l.contains("failed (") && l.contains("Test Case"))
                    .map(|l| l.trim().to_string())
                    .collect(),
                raw_output: raw,
            }
        }
    }
}
