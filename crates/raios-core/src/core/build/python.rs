use super::common::{extract_num, failed_result, failed_test, BuildResult, TestResult};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn build_python(dir: &Path) -> BuildResult {
    let start = Instant::now();
    let (python, python_args) = raios_core::core::process::python_command();
    let out = Command::new(&python)
        .args(&python_args)
        .args(["-m", "py_compile"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_result(
            "Python",
            &format!("{} -m py_compile", python),
            elapsed,
            e.to_string(),
        ),
        Ok(o) => {
            let raw = String::from_utf8_lossy(&o.stderr).into_owned();
            BuildResult {
                ok: o.status.success(),
                project_type: "Python".into(),
                command: format!("{} -m py_compile", python),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors: if o.status.success() { 0 } else { 1 },
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_python(dir: &Path) -> TestResult {
    let start = Instant::now();
    let (python, python_args) = raios_core::core::process::python_command();
    let out = Command::new(&python)
        .args(&python_args)
        .args(["-m", "pytest", "--tb=short", "-q"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_test("Python", "pytest", elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_pytest_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "Python".into(),
                command: "pytest".into(),
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

pub fn parse_pytest_output(output: &str) -> (usize, usize) {
    for line in output.lines().rev() {
        if line.contains("passed") || line.contains("failed") || line.contains("error") {
            let passed = extract_num(line, "passed").unwrap_or(0);
            let failed = extract_num(line, "failed").unwrap_or(0);
            return (passed, failed);
        }
    }
    (0, 0)
}
