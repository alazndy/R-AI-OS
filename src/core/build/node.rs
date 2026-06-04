use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{failed_result, failed_test, extract_num, BuildResult, TestResult};

pub fn build_node(dir: &Path) -> BuildResult {
    let pkg = dir.join("package.json");
    let has_build = std::fs::read_to_string(&pkg)
        .map(|c| c.contains("\"build\""))
        .unwrap_or(false);

    if !has_build {
        return BuildResult {
            ok: true,
            project_type: "Node".into(),
            command: "—".into(),
            duration_ms: 0,
            warnings: 0,
            errors: 0,
            diagnostics: vec![],
            raw_output: "No build script found in package.json".into(),
        };
    }

    let pm = if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    };

    let start = Instant::now();
    let out = Command::new(pm)
        .args(["run", "build"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_result("Node", &format!("{} run build", pm), elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let errors = if o.status.success() { 0 } else { 1 };
            BuildResult {
                ok: o.status.success(),
                project_type: "Node".into(),
                command: format!("{} run build", pm),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors,
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

pub fn test_node(dir: &Path) -> TestResult {
    let pm = if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    };

    let start = Instant::now();
    let out = Command::new(pm)
        .args(["test", "--", "--passWithNoTests"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_test("Node", &format!("{} test", pm), elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_jest_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "Node".into(),
                command: format!("{} test", pm),
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

pub fn parse_jest_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.trim_start().starts_with("Tests:") {
            let passed = extract_num(line, "passed").unwrap_or(0);
            let failed = extract_num(line, "failed").unwrap_or(0);
            return (passed, failed);
        }
    }
    (0, 0)
}
