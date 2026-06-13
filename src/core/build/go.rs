use super::common::{failed_result, failed_test, BuildResult, TestResult};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn build_go(dir: &Path) -> BuildResult {
    let start = Instant::now();
    let out = Command::new("go")
        .args(["build", "./..."])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_result("Go", "go build ./...", elapsed, e.to_string()),
        Ok(o) => {
            let raw = String::from_utf8_lossy(&o.stderr).into_owned();
            BuildResult {
                ok: o.status.success(),
                project_type: "Go".into(),
                command: "go build ./...".into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors: if o.status.success() { 0 } else { 1 },
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_go(dir: &Path) -> TestResult {
    let start = Instant::now();
    let out = Command::new("go")
        .args(["test", "./...", "-v"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_test("Go", "go test ./...", elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let passed = raw.matches("--- PASS").count();
            let failed = raw.matches("--- FAIL").count();
            TestResult {
                ok: o.status.success(),
                project_type: "Go".into(),
                command: "go test ./...".into(),
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
