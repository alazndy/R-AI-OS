use std::path::Path;
use std::process::Command;
use std::time::Instant;
use super::common::{BuildResult, TestResult};

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

pub fn parse_gradle_test_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Tests run:") {
            let total = extract_num_after(trimmed, "Tests run:").unwrap_or(0);
            let failures = extract_num_after(trimmed, "Failures:").unwrap_or(0);
            let errors = extract_num_after(trimmed, "Errors:").unwrap_or(0);
            let failed = failures + errors;
            return (total.saturating_sub(failed), failed);
        }
    }
    (0, 0)
}

fn extract_num_after(s: &str, label: &str) -> Option<usize> {
    let idx = s.find(label)?;
    let rest = s[idx + label.len()..].trim_start();
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|n| n.parse().ok())
}

pub fn parse_gradle_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("BUILD SUCCESSFUL");
    let errors = output
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("e: ")
                || (t.contains(": error:") && (t.starts_with('/') || t.starts_with('.') || t.chars().nth(1) == Some(':')))
                || t.starts_with("error:")
        })
        .count();
    (ok, errors)
}

fn build_android_impl(dir: &Path, task: &str) -> BuildResult {
    let cmd_str = format!("gradlew {}", task);
    let start = Instant::now();

    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        let bat = dir.join("gradlew.bat");
        if bat.exists() {
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

fn run_android_test(dir: &Path, task: &str) -> TestResult {
    let cmd_str = format!("gradlew {}", task);
    let start = Instant::now();

    #[cfg(windows)]
    let output = {
        let dir_str = dir.to_string_lossy().into_owned();
        let raw = format!("/C cd /d \"{}\" && .\\gradlew.bat \"{}\"", dir_str, task);
        use std::os::windows::process::CommandExt;
        Command::new("cmd").raw_arg(&raw).current_dir(dir).output()
    };
    #[cfg(not(windows))]
    let output = {
        let wrapper = if dir.join("gradlew").exists() {
            "./gradlew"
        } else {
            "gradle"
        };
        Command::new(wrapper).arg(task).current_dir(dir).output()
    };

    let elapsed = start.elapsed();

    match output {
        Err(e) => TestResult {
            ok: false,
            project_type: "Android".into(),
            command: cmd_str,
            duration_ms: elapsed.as_millis() as u64,
            passed: 0,
            failed: 1,
            ignored: 0,
            failures: vec![format!("Failed to launch Gradle: {e}")],
            raw_output: String::new(),
        },
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let ok = raw.contains("BUILD SUCCESSFUL");
            let (passed, failed) = parse_gradle_test_output(&raw);
            TestResult {
                ok,
                project_type: "Android".into(),
                command: cmd_str,
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
