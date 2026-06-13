use super::common::{
    extract_num, failed_result, failed_test, BuildDiagnostic, BuildResult, TestResult,
};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn build_rust(dir: &Path) -> BuildResult {
    let start = Instant::now();
    let out = Command::new("cargo")
        .args(["build", "--message-format=json"])
        .current_dir(dir)
        .output();

    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_result("Rust", "cargo build", elapsed, e.to_string()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
            let (warnings, errors, diagnostics) = parse_cargo_json(&stdout);
            let ok = o.status.success();
            BuildResult {
                ok,
                project_type: "Rust".into(),
                command: "cargo build".into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings,
                errors,
                diagnostics,
                raw_output: if ok {
                    stderr
                } else {
                    format!("{}\n{}", stdout, stderr)
                },
            }
        }
    }
}

pub fn test_rust(dir: &Path) -> TestResult {
    let start = Instant::now();
    let out = Command::new("cargo")
        .args(["test"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_test("Rust", "cargo test", elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed, ignored, failures) = parse_rust_test_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "Rust".into(),
                command: "cargo test".into(),
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored,
                failures,
                raw_output: raw,
            }
        }
    }
}

fn parse_cargo_json(stdout: &str) -> (usize, usize, Vec<BuildDiagnostic>) {
    let mut warnings = 0usize;
    let mut errors = 0usize;
    let mut diags = Vec::new();

    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["reason"] != "compiler-message" {
            continue;
        }
        let msg = &v["message"];
        let level = msg["level"].as_str().unwrap_or("").to_string();
        let text = msg["message"].as_str().unwrap_or("").to_string();

        match level.as_str() {
            "warning" => warnings += 1,
            "error" => errors += 1,
            _ => continue,
        }

        let (file, line_no) = msg["spans"]
            .as_array()
            .and_then(|s| s.first())
            .map(|s| {
                (
                    s["file_name"].as_str().unwrap_or("").to_string(),
                    s["line_start"].as_u64().map(|n| n as usize),
                )
            })
            .unwrap_or_default();

        diags.push(BuildDiagnostic {
            file,
            line: line_no,
            level,
            message: text,
        });
    }
    (warnings, errors, diags)
}

pub fn parse_rust_test_output(output: &str) -> (usize, usize, usize, Vec<String>) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;
    let mut failures = Vec::new();

    for line in output.lines() {
        if line.starts_with("test result:") {
            for part in line.split(';') {
                let part = part.trim();
                if let Some(n) = extract_num(part, "passed") {
                    passed += n;
                }
                if let Some(n) = extract_num(part, "failed") {
                    failed += n;
                }
                if let Some(n) = extract_num(part, "ignored") {
                    ignored += n;
                }
            }
        }
        if line.starts_with("FAILED") || line.contains("---- ") && line.contains("FAILED") {
            failures.push(line.trim().to_string());
        }
    }
    (passed, failed, ignored, failures)
}
