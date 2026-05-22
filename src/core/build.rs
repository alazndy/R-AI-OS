use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Android,
    Unknown,
}

impl ProjectType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Node => "Node",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Android => "Android",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildDiagnostic {
    pub file: String,
    pub line: Option<usize>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub ok: bool,
    pub project_type: String,
    pub command: String,
    pub duration_ms: u64,
    pub warnings: usize,
    pub errors: usize,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub ok: bool,
    pub project_type: String,
    pub command: String,
    pub duration_ms: u64,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub failures: Vec<String>,
    pub raw_output: String,
}

// ─── Project type detection ───────────────────────────────────────────────────

pub fn detect_type(dir: &Path) -> ProjectType {
    if dir.join("Cargo.toml").exists() {
        return ProjectType::Rust;
    }
    if dir.join("package.json").exists() {
        return ProjectType::Node;
    }
    if dir.join("pyproject.toml").exists()
        || dir.join("setup.py").exists()
        || dir.join("requirements.txt").exists()
    {
        return ProjectType::Python;
    }
    if dir.join("go.mod").exists() {
        return ProjectType::Go;
    }
    if (dir.join("gradlew").exists() || dir.join("gradlew.bat").exists())
        && (dir.join("build.gradle").exists() || dir.join("settings.gradle").exists())
    {
        return ProjectType::Android;
    }
    ProjectType::Unknown
}

// ─── Build ───────────────────────────────────────────────────────────────────

pub fn build(dir: &Path) -> BuildResult {
    match detect_type(dir) {
        ProjectType::Rust => build_rust(dir),
        ProjectType::Node => build_node(dir),
        ProjectType::Python => build_python(dir),
        ProjectType::Go => build_go(dir),
        ProjectType::Android => build_android(dir),
        ProjectType::Unknown => BuildResult {
            ok: false,
            project_type: "Unknown".into(),
            command: "—".into(),
            duration_ms: 0,
            warnings: 0,
            errors: 1,
            diagnostics: vec![],
            raw_output:
                "Cannot detect project type (no Cargo.toml, package.json, go.mod, pyproject.toml, gradlew)"
                    .into(),
        },
    }
}

fn build_rust(dir: &Path) -> BuildResult {
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

fn build_node(dir: &Path) -> BuildResult {
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

fn build_python(dir: &Path) -> BuildResult {
    let start = Instant::now();
    let out = Command::new("python")
        .args(["-m", "py_compile"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match out {
        Err(e) => failed_result("Python", "python -m py_compile", elapsed, e.to_string()),
        Ok(o) => {
            let raw = String::from_utf8_lossy(&o.stderr).into_owned();
            BuildResult {
                ok: o.status.success(),
                project_type: "Python".into(),
                command: "python -m py_compile".into(),
                duration_ms: elapsed.as_millis() as u64,
                warnings: 0,
                errors: if o.status.success() { 0 } else { 1 },
                diagnostics: vec![],
                raw_output: raw,
            }
        }
    }
}

fn build_go(dir: &Path) -> BuildResult {
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

// ─── Test ────────────────────────────────────────────────────────────────────

pub fn test(dir: &Path) -> TestResult {
    match detect_type(dir) {
        ProjectType::Rust => test_rust(dir),
        ProjectType::Node => test_node(dir),
        ProjectType::Python => test_python(dir),
        ProjectType::Go => test_go(dir),
        ProjectType::Android => test_android_unit(dir),
        ProjectType::Unknown => TestResult {
            ok: false,
            project_type: "Unknown".into(),
            command: "—".into(),
            duration_ms: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            failures: vec!["Cannot detect project type".into()],
            raw_output: String::new(),
        },
    }
}

fn test_rust(dir: &Path) -> TestResult {
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

fn test_node(dir: &Path) -> TestResult {
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

fn test_python(dir: &Path) -> TestResult {
    let start = Instant::now();
    let out = Command::new("python")
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

fn test_go(dir: &Path) -> TestResult {
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

// ─── Output parsers ──────────────────────────────────────────────────────────

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

fn parse_rust_test_output(output: &str) -> (usize, usize, usize, Vec<String>) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;
    let mut failures = Vec::new();

    for line in output.lines() {
        if line.starts_with("test result:") {
            // "test result: ok. 22 passed; 0 failed; 0 ignored"
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

fn parse_jest_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        // "Tests:  47 passed, 2 failed, 49 total"
        if line.trim_start().starts_with("Tests:") {
            let passed = extract_num(line, "passed").unwrap_or(0);
            let failed = extract_num(line, "failed").unwrap_or(0);
            return (passed, failed);
        }
    }
    (0, 0)
}

fn parse_pytest_output(output: &str) -> (usize, usize) {
    for line in output.lines().rev() {
        // "47 passed, 2 failed in 1.23s"
        if line.contains("passed") || line.contains("failed") || line.contains("error") {
            let passed = extract_num(line, "passed").unwrap_or(0);
            let failed = extract_num(line, "failed").unwrap_or(0);
            return (passed, failed);
        }
    }
    (0, 0)
}

fn extract_num(s: &str, keyword: &str) -> Option<usize> {
    let idx = s.find(keyword)?;
    s[..idx].split_whitespace().last()?.parse().ok()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn failed_result(ptype: &str, cmd: &str, elapsed: Duration, msg: String) -> BuildResult {
    BuildResult {
        ok: false,
        project_type: ptype.into(),
        command: cmd.into(),
        duration_ms: elapsed.as_millis() as u64,
        warnings: 0,
        errors: 1,
        diagnostics: vec![],
        raw_output: msg,
    }
}

fn failed_test(ptype: &str, cmd: &str, elapsed: Duration, msg: String) -> TestResult {
    TestResult {
        ok: false,
        project_type: ptype.into(),
        command: cmd.into(),
        duration_ms: elapsed.as_millis() as u64,
        passed: 0,
        failed: 1,
        ignored: 0,
        failures: vec![msg.clone()],
        raw_output: msg,
    }
}

// ─── Android ─────────────────────────────────────────────────────────────────

pub fn build_android(dir: &Path) -> BuildResult {
    build_android_impl(dir, "assembleDebug")
}

pub fn build_android_release(dir: &Path) -> BuildResult {
    build_android_impl(dir, "assembleRelease")
}

pub fn build_android_check(dir: &Path) -> BuildResult {
    build_android_impl(dir, "compileDebugKotlin")
}

pub fn test_android_unit(dir: &Path) -> TestResult {
    run_android_test(dir, "testDebugUnitTest")
}

pub fn test_android_instrumented(dir: &Path) -> TestResult {
    run_android_test(dir, "connectedAndroidTest")
}

/// Parse Gradle stdout+stderr. Returns (success, error_count).
fn parse_gradle_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("BUILD SUCCESSFUL");
    let errors = output
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            // Kotlin compiler error: "e: path/file.kt: (42,5): error: ..."
            t.starts_with("e: ")
                // Java/Gradle error starting with a file path
                || (t.contains(": error:") && (t.starts_with('/') || t.starts_with('.') || t.chars().nth(1) == Some(':')))
                // Gradle wrapper / daemon startup errors
                || t.starts_with("error:")
        })
        .count();
    (ok, errors)
}

fn build_android_impl(dir: &Path, task: &str) -> BuildResult {
    let cmd_str = format!("gradlew {}", task);
    let start = Instant::now();

    // On Windows, .bat files cannot be executed directly — they require cmd.exe.
    // Paths containing special cmd characters (& | < >) must be handled carefully.
    // Using `cd /d "<dir>" && .\gradlew.bat <task>` avoids path-in-argument quoting
    // issues: `cd /d` accepts a quoted path, then gradlew is called from cwd.
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        let bat = dir.join("gradlew.bat");
        if bat.exists() {
            // Build raw command line: cd /d "<dir>" && .\gradlew.bat <task>
            // raw_arg bypasses Rust's per-argument quoting so cmd.exe sees the
            // string exactly as constructed here.
            let dir_str = dir.to_string_lossy().into_owned();
            let raw = format!("/C cd /d \"{}\" && .\\gradlew.bat \"{}\"", dir_str, task);
            let mut c = Command::new("cmd");
            c.raw_arg(raw);
            c.output()
        } else {
            Command::new("gradle")
                .arg(task)
                .current_dir(dir)
                .output()
        }
    };

    #[cfg(not(windows))]
    let output = {
        let prog = if dir.join("gradlew").exists() {
            "./gradlew"
        } else {
            "gradle"
        };
        Command::new(prog)
            .arg(task)
            .current_dir(dir)
            .output()
    };

    let elapsed = start.elapsed();

    match output {
        Err(e) => BuildResult {
            ok: false,
            project_type: "Android".into(),
            command: cmd_str,
            duration_ms: elapsed.as_millis() as u64,
            warnings: 0,
            errors: 1,
            diagnostics: vec![],
            raw_output: format!("Failed to launch Gradle: {e}"),
        },
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_gradle_build_output(&raw);
            BuildResult {
                ok,
                project_type: "Android".into(),
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

fn run_android_test(_dir: &Path, _task: &str) -> TestResult {
    TestResult {
        ok: false,
        project_type: "Android".into(),
        command: "—".into(),
        duration_ms: 0,
        passed: 0,
        failed: 1,
        ignored: 0,
        failures: vec![],
        raw_output: "Android test not yet implemented".into(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn raios_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn detect_rust_project() {
        assert_eq!(detect_type(&raios_root()), ProjectType::Rust);
    }

    #[test]
    fn detect_unknown_on_temp() {
        let tmp = std::env::temp_dir().join("raios_build_test_unknown");
        let _ = std::fs::create_dir_all(&tmp);
        assert_eq!(detect_type(&tmp), ProjectType::Unknown);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_rust_test_output_parses_result_line() {
        let output = "test result: ok. 22 passed; 0 failed; 1 ignored; 0 measured";
        let (p, f, i, _) = parse_rust_test_output(output);
        assert_eq!(p, 22);
        assert_eq!(f, 0);
        assert_eq!(i, 1);
    }

    #[test]
    fn parse_rust_test_output_sums_multiple_binaries() {
        let output = "test result: ok. 143 passed; 3 failed; 0 ignored; 0 measured\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured";
        let (p, f, i, _) = parse_rust_test_output(output);
        assert_eq!(p, 143);
        assert_eq!(f, 3);
        assert_eq!(i, 1);
    }

    #[test]
    fn parse_jest_output_extracts_counts() {
        let output = "Tests:  47 passed, 2 failed, 49 total";
        let (p, f) = parse_jest_output(output);
        assert_eq!(p, 47);
        assert_eq!(f, 2);
    }

    #[test]
    fn parse_pytest_output_extracts_counts() {
        let output = "collected 50 items\n\n47 passed, 3 failed in 1.23s";
        let (p, f) = parse_pytest_output(output);
        assert_eq!(p, 47);
        assert_eq!(f, 3);
    }

    #[test]
    fn extract_num_works() {
        assert_eq!(extract_num("22 passed", "passed"), Some(22));
        assert_eq!(extract_num("no number here", "passed"), None);
    }

    #[test]
    fn detect_android_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("gradlew")).unwrap();
        std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
        assert_eq!(detect_type(tmp.path()), ProjectType::Android);
    }

    #[test]
    fn detect_android_with_bat_and_settings() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("gradlew.bat")).unwrap();
        std::fs::File::create(tmp.path().join("settings.gradle")).unwrap();
        assert_eq!(detect_type(tmp.path()), ProjectType::Android);
    }

    #[test]
    fn rust_takes_priority_over_android() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::File::create(tmp.path().join("gradlew")).unwrap();
        std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
        assert_eq!(detect_type(tmp.path()), ProjectType::Rust);
    }

    #[test]
    fn gradlew_alone_does_not_detect_android() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("gradlew")).unwrap();
        assert_eq!(detect_type(tmp.path()), ProjectType::Unknown);
    }

    #[test]
    fn build_gradle_alone_does_not_detect_android() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
        assert_eq!(detect_type(tmp.path()), ProjectType::Unknown);
    }

    #[test]
    fn parse_gradle_build_success() {
        let output = "BUILD SUCCESSFUL in 15s\n5 actionable tasks: 5 executed";
        let (ok, errors) = parse_gradle_build_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }

    #[test]
    fn parse_gradle_build_failure_counts_errors() {
        let output = "e: file.kt: (42, 5): error: unresolved reference: Foo\ne: file.kt: (50, 3): error: type mismatch\nBUILD FAILED in 8s";
        let (ok, errors) = parse_gradle_build_output(output);
        assert!(!ok);
        assert_eq!(errors, 2);
    }

    #[test]
    fn parse_gradle_build_error_prefix_only() {
        // Line starting with "error:" (daemon/wrapper errors)
        let output = "error: Could not find or load main class GradleWrapperMain\nBUILD FAILED";
        let (ok, errors) = parse_gradle_build_output(output);
        assert!(!ok);
        assert_eq!(errors, 1);
    }

    #[test]
    fn parse_gradle_build_no_false_positive_on_log_line() {
        // A log line containing ": error:" that is NOT from a file path — should not be counted
        let output = "NOTE: The Kotlin options have changed: error: is now deprecated\nBUILD SUCCESSFUL";
        let (ok, errors) = parse_gradle_build_output(output);
        assert!(ok);
        assert_eq!(errors, 0);
    }
}
