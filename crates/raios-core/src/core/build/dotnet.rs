use super::common::{failed_result, failed_test, BuildResult, TestResult};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn parse_dotnet_build_output(output: &str) -> (bool, usize, usize) {
    let ok = output.contains("Build succeeded");
    let mut warnings = 0usize;
    let mut errors = 0usize;
    for line in output.lines() {
        let t = line.trim();
        if t.ends_with("Warning(s)") {
            warnings = t
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        }
        if t.ends_with("Error(s)") {
            errors = t
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        }
    }
    (ok, warnings, errors)
}

pub(crate) fn parse_dotnet_test_output(output: &str) -> (usize, usize, usize) {
    for line in output.lines() {
        if line.contains("Passed:") && line.contains("Failed:") {
            let passed = extract_after(line, "Passed:").unwrap_or(0);
            let failed = extract_after(line, "Failed:").unwrap_or(0);
            let skipped = extract_after(line, "Skipped:").unwrap_or(0);
            return (passed, failed, skipped);
        }
    }
    (0, 0, 0)
}

fn extract_after(s: &str, key: &str) -> Option<usize> {
    s.split(key)
        .nth(1)?
        .split([',', ' '])
        .find(|p| !p.trim().is_empty())?
        .trim()
        .parse()
        .ok()
}

pub fn build_dotnet(dir: &Path) -> BuildResult {
    let cmd_str = "dotnet build";
    let start = Instant::now();
    let output = Command::new("dotnet")
        .arg("build")
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result(".NET", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, warnings, errors) = parse_dotnet_build_output(&raw);
            BuildResult {
                ok,
                project_type: ".NET".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_dotnet(dir: &Path) -> TestResult {
    let cmd_str = "dotnet test";
    let start = Instant::now();
    let output = Command::new("dotnet").arg("test").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_test(".NET", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed, ignored) = parse_dotnet_test_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: ".NET".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored,
                failures: raw
                    .lines()
                    .filter(|l| l.contains("Failed") && !l.contains("Failed:"))
                    .map(|l| l.to_string())
                    .collect(),
                raw_output: raw,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotnet_build_success() {
        let output = "Build succeeded.\n  0 Warning(s)\n  0 Error(s)\n\nTime Elapsed 00:00:02.456";
        let (ok, warnings, errors) = parse_dotnet_build_output(output);
        assert!(ok);
        assert_eq!(warnings, 0);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_dotnet_build_failure() {
        let output = "error CS0246: The type or namespace name 'Foo' could not be found\nBuild FAILED.\n  0 Warning(s)\n  1 Error(s)";
        let (ok, _, errors) = parse_dotnet_build_output(output);
        assert!(!ok);
        assert_eq!(errors, 1);
    }

    #[test]
    fn parse_dotnet_test_results() {
        let output =
            "Passed!  - Failed:     0, Passed:    12, Skipped:     2, Total:    14, Duration: 1 s";
        let (passed, failed, ignored) = parse_dotnet_test_output(output);
        assert_eq!(passed, 12);
        assert_eq!(failed, 0);
        assert_eq!(ignored, 2);
    }
}
