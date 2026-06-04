use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{failed_result, failed_test, BuildResult, TestResult};

pub fn parse_flutter_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Built build/")
        || output.contains("Build complete!")
        || output.contains("Succeeded after");
    let errors = if !ok
        && (output.contains("Error:") || output.contains("error:") || output.contains("Failed"))
    {
        output
            .lines()
            .filter(|l| l.trim_start().starts_with("Error:") || l.contains(": error:"))
            .count()
            .max(1)
    } else {
        0
    };
    (ok, errors)
}

pub fn parse_flutter_test_output(output: &str) -> (usize, usize) {
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if trimmed.len() > 6 && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let rest = trimmed.split_once(' ').map(|x| x.1).unwrap_or("");
            let passed = rest
                .split_whitespace()
                .find(|w| w.starts_with('+'))
                .and_then(|w| w[1..].trim_end_matches(':').parse::<usize>().ok())
                .unwrap_or(0);
            let failed = rest
                .split_whitespace()
                .find(|w| w.starts_with('-'))
                .and_then(|w| w[1..].trim_end_matches(':').parse::<usize>().ok())
                .unwrap_or(0);
            if passed > 0 || failed > 0 {
                return (passed, failed);
            }
        }
    }
    (0, 0)
}

fn build_flutter_impl(dir: &Path, args: &[&str]) -> BuildResult {
    let cmd_str = format!("flutter {}", args.join(" "));
    let start = Instant::now();
    let output = Command::new("flutter").args(args).current_dir(dir).output();
    let elapsed = start.elapsed();

    match output {
        Err(e) => failed_result("Flutter", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_flutter_build_output(&raw);
            let ok = ok && o.status.success();
            BuildResult {
                ok,
                project_type: "Flutter".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                warnings: raw.lines().filter(|l| l.contains("Warning:")).count(),
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn build_flutter(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["build", "apk"])
}

pub fn build_flutter_release(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["build", "apk", "--release"])
}

pub fn build_flutter_check(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["analyze"])
}

pub fn test_flutter(dir: &Path) -> TestResult {
    let cmd_str = "flutter test";
    let start = Instant::now();
    let output = Command::new("flutter").args(["test"]).current_dir(dir).output();
    let elapsed = start.elapsed();

    match output {
        Err(e) => failed_test("Flutter", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_flutter_test_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "Flutter".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored: 0,
                failures: raw
                    .lines()
                    .filter(|l| l.contains("FAILED") || l.contains("✗"))
                    .map(|l| l.trim().to_string())
                    .collect(),
                raw_output: raw,
            }
        }
    }
}
