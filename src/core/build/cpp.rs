use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{failed_result, failed_test, BuildResult, TestResult};

pub(crate) fn parse_cmake_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Built target") || output.contains("[100%]");
    let errors = output
        .lines()
        .filter(|l| {
            (l.contains(": error:") || l.starts_with("error:"))
                && (l.contains(".cpp") || l.contains(".c") || l.contains(".cc"))
        })
        .count();
    (ok && errors == 0, errors)
}

pub(crate) fn parse_ctest_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.contains("tests passed") || line.contains("tests failed") {
            let total = line
                .split("out of")
                .nth(1)
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let failed = line
                .split(',')
                .find(|s| s.contains("failed"))
                .and_then(|s| s.split_whitespace().find(|w| w.parse::<usize>().is_ok()))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            return (total.saturating_sub(failed), failed);
        }
    }
    (0, 0)
}

pub fn build_cpp(dir: &Path) -> BuildResult {
    let build_dir = dir.join("build");
    let _ = std::fs::create_dir_all(&build_dir);
    let _ = Command::new("cmake")
        .args([".."])
        .current_dir(&build_dir)
        .output();
    let cmd_str = "cmake --build build";
    let start = Instant::now();
    let output = Command::new("cmake")
        .args(["--build", "build"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("C++", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_cmake_build_output(&raw);
            BuildResult {
                ok,
                project_type: "C++".into(),
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

pub fn test_cpp(dir: &Path) -> TestResult {
    let build_dir = dir.join("build");
    let cmd_str = "ctest --test-dir build";
    let start = Instant::now();
    let output = Command::new("ctest")
        .args([
            "--test-dir",
            build_dir.to_str().unwrap_or("build"),
            "--output-on-failure",
        ])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_test("C++", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_ctest_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "C++".into(),
                command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored: 0,
                failures: raw
                    .lines()
                    .filter(|l| l.trim_start().starts_with("FAILED"))
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
    fn parse_cmake_build_success() {
        let output = "[100%] Linking CXX executable MyApp\n[100%] Built target MyApp\n";
        let (ok, errors) = parse_cmake_build_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_cmake_build_failure() {
        let output = "main.cpp:5:3: error: 'undeclared_var' was not declared\nCMakeFiles/MyApp.dir/build.make:89: recipe for target failed\n";
        let (ok, errors) = parse_cmake_build_output(output);
        assert!(!ok);
        assert!(errors >= 1);
    }

    #[test]
    fn parse_ctest_results() {
        let output =
            "100% tests passed, 0 tests failed out of 5\n\nTotal Test time (real) =   0.15 sec";
        let (passed, failed) = parse_ctest_output(output);
        assert_eq!(passed, 5);
        assert_eq!(failed, 0);
    }
}
