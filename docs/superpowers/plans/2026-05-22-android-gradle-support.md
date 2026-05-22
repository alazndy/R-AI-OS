# Android/Gradle Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Android/Gradle support to `raios build`, `test`, `deps`, and `version-info`/`version-bump` so GT Launcher and similar projects work instead of returning "Unknown project type".

**Architecture:** Add `ProjectType::Android` to the existing detection enum in `src/core/build.rs`. Each subsystem (`build`, `test`, `deps`, `version`) gets Android-specific functions that call `./gradlew.bat` (Windows) or `./gradlew` (Unix). CLI flags `--release`, `--check`, `--instrumented` are added to the `Build`/`Test` commands. No new files needed — all changes are additive to existing files.

**Tech Stack:** Rust, clap (CLI), std::process::Command (Gradle runner), regex (version parsing)

**Test project:** `c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher`

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::Android`; `gradlew_args()`; `build_android()`, `build_android_release()`, `build_android_check()`; `test_android_unit()`, `test_android_instrumented()`; parse helpers |
| `src/core/deps.rs` | Add `ProjectType::Android` arm in `check()`; `check_android()` with libs.versions.toml parse |
| `src/core/version.rs` | Add `read_android_version()`; extend `read_version()` and `write_version()` for Android |
| `src/cli/mod.rs` | `Build`: add `--release`, `--check`; `Test`: add `--instrumented` |
| `src/cli/dev.rs` | Pass new flags through `cmd_build()` and `cmd_test()` |
| `src/security/scanner.rs` | Add Android to `detect_project_type()` |

---

## Task 1 — Android Detection

**Files:** `src/core/build.rs` (lines 1-81), `src/security/scanner.rs` (lines 59-90)

### Step 1: Add a failing test for Android detection

In `src/core/build.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
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
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_android" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `ProjectType::Android` doesn't exist yet.

### Step 3: Add `Android` to `ProjectType` enum in `src/core/build.rs`

Replace:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Unknown,
}

impl ProjectType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Node => "Node",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Unknown => "Unknown",
        }
    }
}
```

With:
```rust
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
```

### Step 4: Add Android detection to `detect_type()` in `src/core/build.rs`

Replace the existing `detect_type` function:
```rust
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
```

### Step 5: Add `Android` arm to `build()` dispatch in `src/core/build.rs`

The current `build()` function has a match on `detect_type(dir)`. Add the Android arm after Go:
```rust
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
            raw_output: "Cannot detect project type (no Cargo.toml, package.json, go.mod, pyproject.toml, gradlew)".into(),
        },
    }
}
```

Also add the Android arm to `test()` dispatch:
```rust
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
```

The Android build/test functions are stubs for now — add them as `todo!()` placeholders so it compiles:
```rust
pub fn build_android(dir: &Path) -> BuildResult { build_android_impl(dir, "assembleDebug") }
pub fn build_android_release(dir: &Path) -> BuildResult { build_android_impl(dir, "assembleRelease") }
pub fn build_android_check(dir: &Path) -> BuildResult { build_android_impl(dir, "compileDebugKotlin") }
pub fn test_android_unit(dir: &Path) -> TestResult { run_android_test(dir, "testDebugUnitTest") }
pub fn test_android_instrumented(dir: &Path) -> TestResult { run_android_test(dir, "connectedAndroidTest") }

fn build_android_impl(_dir: &Path, _task: &str) -> BuildResult {
    BuildResult { ok: false, project_type: "Android".into(), command: "—".into(), duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}
fn run_android_test(_dir: &Path, _task: &str) -> TestResult {
    TestResult { ok: false, project_type: "Android".into(), command: "—".into(), duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 6: Add Android to `deps::check()` in `src/core/deps.rs`

`deps.rs` imports `ProjectType` from `build.rs`. Add the Android arm:
```rust
pub fn check(dir: &Path) -> DepsReport {
    use crate::core::build::detect_type;
    use crate::core::build::ProjectType;

    match detect_type(dir) {
        ProjectType::Rust => check_rust(dir),
        ProjectType::Node => check_node(dir),
        ProjectType::Python => check_python(dir),
        ProjectType::Go => check_go(dir),
        ProjectType::Android => check_android(dir),
        ProjectType::Unknown => {
            let mut r = DepsReport::empty("Unknown");
            r.tool_missing.push("Cannot detect project type".into());
            r
        }
    }
}
```

Add stub for `check_android` (full implementation in Task 4):
```rust
fn check_android(dir: &Path) -> DepsReport {
    let _ = dir;
    DepsReport::empty("Android")
}
```

### Step 7: Add Android to `security/scanner.rs` `detect_project_type()`

In `src/security/scanner.rs`, in the `detect_project_type()` function, add before the `Unknown` return:
```rust
    if (path.join("gradlew").exists() || path.join("gradlew.bat").exists())
        && (path.join("build.gradle").exists() || path.join("settings.gradle").exists())
    {
        return ProjectType::Web; // Android maps to Web (no specific security type)
    }
```

Wait — `security/scanner.rs` uses its own `ProjectType` which is defined in `security/mod.rs`, not `core/build.rs`. That enum has: `Rust, NodeJs, Python, Web, Mixed, Unknown`. Android maps to `Web` for security scanning purposes (we scan HTML/JS/TS patterns for the web stack). No code change needed — the existing `Unknown` fallback works.

### Step 8: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 9: Run detection tests

```powershell
cargo test "detect_android\|detect_rust_project\|detect_unknown" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: all 5 tests pass.

### Step 10: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add ProjectType::Android detection for Gradle projects"
```

---

## Task 2 — Android Build Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for Gradle output parser

In `src/core/build.rs`, inside `#[cfg(test)] mod tests`:

```rust
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
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_gradle_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_gradle_build_output` doesn't exist yet.

### Step 3: Add `gradlew_args()` and `parse_gradle_build_output()` to `src/core/build.rs`

Add these private functions (before the `#[cfg(test)]` block):

```rust
/// Returns (program, base_args) for running gradlew on the current platform.
/// On Windows uses `cmd /C gradlew.bat`; on Unix uses `./gradlew`.
fn gradlew_args(dir: &Path) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if dir.join("gradlew.bat").exists() {
            return ("cmd".into(), vec!["/C".into(), "gradlew.bat".into()]);
        }
    }
    #[cfg(not(windows))]
    {
        if dir.join("gradlew").exists() {
            return ("./gradlew".into(), vec![]);
        }
    }
    ("gradle".into(), vec![])
}

/// Parse Gradle build stdout/stderr.
/// Returns (success, error_count).
fn parse_gradle_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("BUILD SUCCESSFUL");
    let errors = output
        .lines()
        .filter(|l| {
            l.trim_start().starts_with("e: ")
                || l.contains(": error:")
                || l.starts_with("error:")
        })
        .count();
    (ok, errors)
}
```

### Step 4: Replace stub Android build functions with real implementations

Replace the three `build_android_*` stubs and `build_android_impl` stub:

```rust
pub fn build_android(dir: &Path) -> BuildResult {
    build_android_impl(dir, "assembleDebug")
}

pub fn build_android_release(dir: &Path) -> BuildResult {
    build_android_impl(dir, "assembleRelease")
}

pub fn build_android_check(dir: &Path) -> BuildResult {
    build_android_impl(dir, "compileDebugKotlin")
}

fn build_android_impl(dir: &Path, task: &str) -> BuildResult {
    let (prog, mut base_args) = gradlew_args(dir);
    base_args.push(task.to_string());
    let cmd_str = format!("gradlew {}", task);

    let start = Instant::now();
    let output = Command::new(&prog)
        .args(&base_args)
        .current_dir(dir)
        .output();
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
```

### Step 5: Run parser tests

```powershell
cargo test "parse_gradle_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: both `parse_gradle_build_success` and `parse_gradle_build_failure_counts_errors` pass.

### Step 6: Add CLI flags to `Build` in `src/cli/mod.rs`

Change:
```rust
    /// Build a project (auto-detects Rust/Node/Python/Go)
    Build { project: Option<String> },
```

To:
```rust
    /// Build a project (auto-detects Rust/Node/Python/Go/Android)
    Build {
        project: Option<String>,
        /// Android: assembleRelease instead of assembleDebug
        #[arg(long)] release: bool,
        /// Android: compileDebugKotlin (type-check only, no APK)
        #[arg(long)] check: bool,
    },
```

### Step 7: Update dispatch in `src/cli/mod.rs` `run()` and `src/cli/dev.rs` `cmd_build()`

In `src/cli/mod.rs`, change the dispatch arm:
```rust
        Commands::Build { project, release, check } => {
            dev::cmd_build(project, release, check, &cfg.dev_ops_path, cli.json)
        }
```

In `src/cli/dev.rs`, replace `cmd_build`:
```rust
pub(super) fn cmd_build(project: Option<String>, release: bool, check: bool, dev_ops: &Path, json: bool) {
    use crate::core::build::{self, detect_type, ProjectType};
    let path = super::resolve_project_path(project, dev_ops);

    let result = match detect_type(&path) {
        ProjectType::Android if check => build::build_android_check(&path),
        ProjectType::Android if release => build::build_android_release(&path),
        ProjectType::Android => build::build_android(&path),
        _ => build::build(&path),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
        return;
    }
    let status = if result.ok { "✓ OK" } else { "✗ FAILED" };
    println!("{} {} — {} in {}ms  ({} warnings, {} errors)", status, result.project_type, result.command, result.duration_ms, result.warnings, result.errors);
    for d in &result.diagnostics {
        let loc = d.line.map(|l| format!(":{}", l)).unwrap_or_default();
        println!("  [{}] {}{} — {}", d.level.to_uppercase(), d.file, loc, d.message);
    }
    if !result.ok && result.diagnostics.is_empty() {
        println!("{}", result.raw_output);
    }
}
```

### Step 8: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 9: Smoke test build against GT Launcher

```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
$R = ".\target\debug\raios.exe"
cargo build --bin raios -q 2>&1
$GT = "c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher"
& $R build $GT 2>&1 | Select-Object -Last 5
& $R build $GT --check 2>&1 | Select-Object -Last 3
```

Expected: shows `✓ OK Android — gradlew assembleDebug in XXXXms` or `✗ FAILED` with error count. No longer shows "Unknown project type".

### Step 10: Commit

```powershell
git add src/core/build.rs src/cli/mod.rs src/cli/dev.rs
git commit -m "feat: Android build — assembleDebug/Release/Check with gradlew, --release and --check flags"
```

---

## Task 3 — Android Test Functions

**Files:** `src/core/build.rs`, `src/cli/mod.rs`, `src/cli/dev.rs`

### Step 1: Add failing test for Gradle test output parser

In `src/core/build.rs`, inside `#[cfg(test)] mod tests`:

```rust
#[test]
fn parse_gradle_test_output_success() {
    let output = "Tests run: 47, Failures: 2, Errors: 0, Skipped: 1\nBUILD SUCCESSFUL in 12s";
    let (passed, failed) = parse_gradle_test_output(output);
    assert_eq!(passed, 45);
    assert_eq!(failed, 2);
}

#[test]
fn parse_gradle_test_all_pass() {
    let output = "Tests run: 20, Failures: 0, Errors: 0, Skipped: 0\nBUILD SUCCESSFUL";
    let (passed, failed) = parse_gradle_test_output(output);
    assert_eq!(passed, 20);
    assert_eq!(failed, 0);
}

#[test]
fn parse_gradle_test_build_failed_no_tests() {
    let output = "BUILD FAILED in 5s\nCould not connect to emulator";
    let (passed, failed) = parse_gradle_test_output(output);
    assert_eq!(passed, 0);
    assert_eq!(failed, 0);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_gradle_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_gradle_test_output` doesn't exist yet.

### Step 3: Add `parse_gradle_test_output()` to `src/core/build.rs`

Add after `parse_gradle_build_output`:

```rust
/// Parse Gradle test output. Returns (passed, failed).
/// Looks for: "Tests run: N, Failures: M"
fn parse_gradle_test_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Tests run:") {
            // Format: "Tests run: 47, Failures: 2, Errors: 0, Skipped: 1"
            let total = extract_num_after(trimmed, "Tests run:").unwrap_or(0);
            let failures = extract_num_after(trimmed, "Failures:").unwrap_or(0);
            let errors = extract_num_after(trimmed, "Errors:").unwrap_or(0);
            let failed = failures + errors;
            let passed = total.saturating_sub(failed);
            return (passed, failed);
        }
    }
    (0, 0)
}

/// Extract the number immediately after a label like "Tests run: 47, ..."
fn extract_num_after(s: &str, label: &str) -> Option<usize> {
    let idx = s.find(label)?;
    let rest = s[idx + label.len()..].trim_start();
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|n| n.parse().ok())
}
```

### Step 4: Replace stub Android test functions with real implementations

Replace `run_android_test` stub:

```rust
pub fn test_android_unit(dir: &Path) -> TestResult {
    run_android_test(dir, "testDebugUnitTest")
}

pub fn test_android_instrumented(dir: &Path) -> TestResult {
    run_android_test(dir, "connectedAndroidTest")
}

fn run_android_test(dir: &Path, task: &str) -> TestResult {
    let (prog, mut base_args) = gradlew_args(dir);
    base_args.push(task.to_string());
    let cmd_str = format!("gradlew {}", task);

    let start = Instant::now();
    let output = Command::new(&prog)
        .args(&base_args)
        .current_dir(dir)
        .output();
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
```

### Step 5: Run parser tests

```powershell
cargo test "parse_gradle_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: all 3 tests pass.

### Step 6: Add `--instrumented` flag to `Test` in `src/cli/mod.rs`

Find:
```rust
    Test {
        project: Option<String>,
        #[arg(long)] all: bool,
    },
```

Replace with:
```rust
    /// Run tests (auto-detects Rust/Node/Python/Go/Android)
    Test {
        project: Option<String>,
        /// Test all projects in portfolio
        #[arg(long)] all: bool,
        /// Android: run connectedAndroidTest (requires device/emulator)
        #[arg(long)] instrumented: bool,
    },
```

### Step 7: Update dispatch and `cmd_test` in `src/cli/mod.rs` and `src/cli/dev.rs`

In `src/cli/mod.rs` dispatch:
```rust
        Commands::Test { project, all, instrumented } => {
            dev::cmd_test(project, all, instrumented, &cfg.dev_ops_path, cli.json)
        }
```

In `src/cli/dev.rs`, replace `cmd_test` signature and single-project branch:
```rust
pub(super) fn cmd_test(project: Option<String>, all: bool, instrumented: bool, dev_ops: &Path, json: bool) {
    use crate::core::build::{self, detect_type, ProjectType};
    if all {
        // existing all-projects logic — unchanged
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                let mut total_pass = 0usize;
                let mut total_fail = 0usize;
                for p in &projects {
                    let path = std::path::Path::new(&p.path);
                    if !path.exists() { continue; }
                    let r = build::test(path);
                    total_pass += r.passed; total_fail += r.failed;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    } else {
                        let status = if r.ok { "✓" } else { "✗" };
                        println!("{} {:<30} {}/{} tests  {}ms", status, p.name, r.passed, r.passed + r.failed, r.duration_ms);
                        for f in &r.failures { println!("    ↳ {}", f); }
                    }
                }
                if !json { println!("\nTotal: {} passed, {} failed", total_pass, total_fail); }
            }
        }
        return;
    }

    let path = super::resolve_project_path(project, dev_ops);
    let result = match detect_type(&path) {
        ProjectType::Android if instrumented => build::test_android_instrumented(&path),
        ProjectType::Android => build::test_android_unit(&path),
        _ => build::test(&path),
    };

    if json { println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default()); return; }
    let status = if result.ok { "✓" } else { "✗" };
    println!("{} {} — {} passed, {} failed, {} ignored  ({}ms)", status, result.command, result.passed, result.failed, result.ignored, result.duration_ms);
    for f in &result.failures { println!("  ↳ {}", f); }
    if !result.ok && result.failures.is_empty() { println!("{}", result.raw_output); }
}
```

### Step 8: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 9: Smoke test against GT Launcher

```powershell
cargo build --bin raios -q 2>&1
$R = ".\target\debug\raios.exe"
$GT = "c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher"
& $R test $GT 2>&1 | Select-Object -Last 5
```

Expected: `✓ gradlew testDebugUnitTest — N passed, 0 failed` (or FAILED with error output).

### Step 10: Commit

```powershell
git add src/core/build.rs src/cli/mod.rs src/cli/dev.rs
git commit -m "feat: Android test — unit and instrumented test runners with --instrumented flag"
```

---

## Task 4 — Android Deps (Version Catalog)

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for version catalog parser

In `src/core/deps.rs`, add a `#[cfg(test)] mod tests` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_version_catalog_entries_basic() {
        let toml = "[versions]\nkotlin = \"2.0.0\"\ncompose = \"1.7.8\"\nretrofit = \"2.11.0\"\n\n[libraries]\nretrofit-core = { group = \"com.squareup\", version.ref = \"retrofit\" }\n";
        let count = count_catalog_versions(toml);
        assert_eq!(count, 3);
    }

    #[test]
    fn count_version_catalog_empty_versions() {
        let toml = "[libraries]\nsome = \"x:y:1.0\"\n";
        let count = count_catalog_versions(toml);
        assert_eq!(count, 0);
    }

    #[test]
    fn check_android_finds_version_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("gradle")).unwrap();
        std::fs::write(
            tmp.path().join("gradle/libs.versions.toml"),
            "[versions]\nkotlin = \"2.0.0\"\ncompose = \"1.7.8\"\n",
        ).unwrap();
        std::fs::File::create(tmp.path().join("gradlew")).unwrap();
        std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
        let report = check_android(tmp.path());
        assert_eq!(report.project_type, "Android");
        assert!(report.has_lockfile);
        assert_eq!(report.outdated_count, 2); // version catalog entries
        assert!(report.tool_missing.iter().any(|m| m.contains("OWASP")));
    }
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "deps::tests" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — functions don't exist yet.

### Step 3: Implement `check_android()` in `src/core/deps.rs`

Add after the existing `check_go` function (before `fn cvss_to_severity`):

```rust
pub(crate) fn check_android(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Android");

    let catalog_path = dir.join("gradle").join("libs.versions.toml");
    if catalog_path.exists() {
        report.has_lockfile = true;
        if let Ok(content) = std::fs::read_to_string(&catalog_path) {
            let entry_count = count_catalog_versions(&content);
            report.outdated_count = entry_count;
        }
    } else {
        report.has_lockfile = false;
    }

    report.tool_missing.push(
        "OWASP CVE scan: add `id 'org.owasp.dependencycheck'` plugin to build.gradle".into(),
    );
    report
}

/// Count entries in the [versions] section of a Gradle version catalog TOML.
fn count_catalog_versions(toml: &str) -> usize {
    let mut in_versions = false;
    let mut count = 0;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[versions]" {
            in_versions = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_versions = false;
        }
        if in_versions && trimmed.contains('=') && !trimmed.starts_with('#') {
            count += 1;
        }
    }
    count
}
```

### Step 4: Run tests

```powershell
cargo test "deps::tests" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: all 3 tests pass.

### Step 5: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 6: Smoke test against GT Launcher

```powershell
cargo build --bin raios -q 2>&1
$R = ".\target\debug\raios.exe"
$GT = "c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher"
& $R deps $GT 2>&1
```

Expected output:
```
── Android ──  lockfile: ✓
  ✓  All deps up to date
  ℹ  Tool not found: OWASP CVE scan: add ...
```
(outdated_count = number of `[versions]` entries in libs.versions.toml, but `print_deps_report` shows it as "outdated" — that's acceptable for now)

### Step 7: Commit

```powershell
git add src/core/deps.rs
git commit -m "feat: Android deps — parse libs.versions.toml version catalog, report OWASP CVE guidance"
```

---

## Task 5 — Android Version Read/Write

**Files:** `src/core/version.rs`

### Step 1: Add failing tests

In `src/core/version.rs`, add tests to the existing `#[cfg(test)] mod tests` block (or create one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_android_version_parses_groovy_dsl() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        fs::write(
            tmp.path().join("app/build.gradle"),
            "android {\n    defaultConfig {\n        versionCode 42\n        versionName '4.2.15'\n    }\n}\n",
        ).unwrap();
        let (name, code) = read_android_version(tmp.path()).unwrap();
        assert_eq!(name, "4.2.15");
        assert_eq!(code, 42);
    }

    #[test]
    fn read_android_version_returns_none_if_no_app_gradle() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_android_version(tmp.path()).is_none());
    }

    #[test]
    fn write_android_version_updates_both_fields() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        fs::write(
            tmp.path().join("app/build.gradle"),
            "    versionCode 42\n    versionName '4.2.15'\n",
        ).unwrap();
        write_android_version(tmp.path(), "4.2.16", 43).unwrap();
        let content = fs::read_to_string(tmp.path().join("app/build.gradle")).unwrap();
        assert!(content.contains("versionCode 43"), "versionCode not updated: {content}");
        assert!(content.contains("versionName '4.2.16'"), "versionName not updated: {content}");
    }

    #[test]
    fn write_android_version_does_not_corrupt_other_content() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("app")).unwrap();
        let original = "android {\n    compileSdk 34\n    defaultConfig {\n        applicationId \"com.example\"\n        versionCode 10\n        versionName '1.0.0'\n        minSdk 26\n    }\n}\n";
        fs::write(tmp.path().join("app/build.gradle"), original).unwrap();
        write_android_version(tmp.path(), "1.0.1", 11).unwrap();
        let content = fs::read_to_string(tmp.path().join("app/build.gradle")).unwrap();
        assert!(content.contains("compileSdk 34"));
        assert!(content.contains("applicationId \"com.example\""));
        assert!(content.contains("versionCode 11"));
        assert!(content.contains("versionName '1.0.1'"));
    }
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "version::tests" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — `read_android_version` and `write_android_version` don't exist yet.

### Step 3: Add `read_android_version()` and `write_android_version()` to `src/core/version.rs`

Add after `read_pyproject_version`:

```rust
/// Read (versionName, versionCode) from app/build.gradle (Groovy DSL).
/// Returns None if app/build.gradle doesn't exist or lacks either field.
pub(crate) fn read_android_version(dir: &Path) -> Option<(String, u64)> {
    let content = std::fs::read_to_string(dir.join("app").join("build.gradle")).ok()?;
    let name = parse_version_name(&content)?;
    let code = parse_version_code(&content)?;
    Some((name, code))
}

fn parse_version_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("versionName") {
            // versionName '4.2.15'  OR  versionName "4.2.15"
            let val = trimmed
                .trim_start_matches("versionName")
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            if looks_like_semver(val) {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_version_code(content: &str) -> Option<u64> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("versionCode") {
            let val = trimmed
                .trim_start_matches("versionCode")
                .trim();
            if let Ok(n) = val.parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

/// Write new versionName and versionCode into app/build.gradle.
/// Uses regex-free line-by-line replacement.
pub(crate) fn write_android_version(dir: &Path, new_name: &str, new_code: u64) -> Result<(), String> {
    let path = dir.join("app").join("build.gradle");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("versionName") {
                let quote = if trimmed.contains('\'') { '\'' } else { '"' };
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}versionName {quote}{new_name}{quote}")
            } else if trimmed.starts_with("versionCode") {
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}versionCode {new_code}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Preserve trailing newline if original had one
    let final_content = if content.ends_with('\n') {
        format!("{updated}\n")
    } else {
        updated
    };
    std::fs::write(&path, final_content).map_err(|e| e.to_string())
}
```

### Step 4: Extend `read_version()` to include Android

In `read_version()`, add before the final `None`:

```rust
fn read_version(dir: &Path) -> Option<(String, String, String)> {
    if let Some(v) = read_cargo_version(dir) {
        return Some((v, "Rust".into(), "Cargo.toml".into()));
    }
    if let Some(v) = read_npm_version(dir) {
        return Some((v, "Node".into(), "package.json".into()));
    }
    if let Some(v) = read_pyproject_version(dir) {
        return Some((v, "Python".into(), "pyproject.toml".into()));
    }
    if let Some((name, _code)) = read_android_version(dir) {
        return Some((name, "Android".into(), "app/build.gradle".into()));
    }
    None
}
```

### Step 5: Extend `write_version()` to handle Android

In `write_version()`, add Android arm. The function receives `path = dir.join("app/build.gradle")` for Android. We need to infer `dir` from path (go up one directory from `app/build.gradle`). Add Android branch:

```rust
fn write_version(path: &Path, project_type: &str, old: &str, new: &str) -> Result<(), String> {
    if project_type == "Android" {
        // path is app/build.gradle; dir is path.parent().parent()
        let dir = path.parent().and_then(|p| p.parent())
            .ok_or("Cannot resolve project dir from app/build.gradle")?;
        let (_, old_code) = read_android_version(dir)
            .ok_or("Cannot read current versionCode from app/build.gradle")?;
        return write_android_version(dir, new, old_code + 1);
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let updated = if project_type == "Node" {
        let mut v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        v["version"] = serde_json::Value::String(new.to_string());
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    } else {
        content.replacen(&format!("\"{}\"", old), &format!("\"{}\"", new), 1)
    };
    std::fs::write(path, updated).map_err(|e| e.to_string())
}
```

### Step 6: Run all version tests

```powershell
cargo test "version" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: all 4 new tests pass, plus any existing version tests.

### Step 7: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 8: Smoke test against GT Launcher

```powershell
cargo build --bin raios -q 2>&1
$R = ".\target\debug\raios.exe"
$GT = "c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher"
& $R version-info $GT 2>&1
```

Expected:
```
Version:  4.2.16 (Android)
File:     app/build.gradle
Last tag: v4.2.14  (N commits since)
```

### Step 9: Full integration test — all commands on GT Launcher

```powershell
$R = ".\target\debug\raios.exe"
$GT = "c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher"
Write-Host "=== build ===" ; & $R build $GT 2>&1 | Select-Object -Last 3
Write-Host "=== build --check ===" ; & $R build $GT --check 2>&1 | Select-Object -Last 3
Write-Host "=== test ===" ; & $R test $GT 2>&1 | Select-Object -Last 3
Write-Host "=== deps ===" ; & $R deps $GT 2>&1
Write-Host "=== version-info ===" ; & $R version-info $GT 2>&1
```

All should return Android-specific output, none should say "Unknown project type".

### Step 10: Commit and push

```powershell
git add src/core/version.rs
git commit -m "feat: Android version — read/write versionName + versionCode in app/build.gradle"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- ✅ Detection: Task 1 adds `ProjectType::Android`, `detect_type()`, `detect_project_type()` in scanner
- ✅ Build 3 modes: Task 2 adds `build_android()`, `build_android_release()`, `build_android_check()` + `--release`/`--check` flags
- ✅ Test 2 modes: Task 3 adds `test_android_unit()`, `test_android_instrumented()` + `--instrumented` flag
- ✅ Deps version catalog: Task 4 adds `check_android()` with `libs.versions.toml` parse + OWASP guidance
- ✅ Version versionName + versionCode: Task 5 adds `read_android_version()`, `write_android_version()`, extends `read_version()` and `write_version()`

**Placeholder scan:** No TBDs or "implement later" phrases. All functions have complete code.

**Type consistency:**
- `read_android_version(dir) -> Option<(String, u64)>` — used consistently in Tasks 5
- `write_android_version(dir, new_name: &str, new_code: u64) -> Result<(), String>` — consistent signature
- `gradlew_args(dir) -> (String, Vec<String>)` — used in both Task 2 and Task 3
- `parse_gradle_build_output(output) -> (bool, usize)` — used only in Task 2
- `parse_gradle_test_output(output) -> (usize, usize)` — used only in Task 3
- All `ProjectType::Android` arms added consistently across `build()`, `test()`, `deps::check()`

**Gap found:** Task 1 Step 7 incorrectly states that `security/scanner.rs` `detect_project_type` maps Android to `Web`. That function uses `security::ProjectType` (separate enum with no Android variant). The existing fallback to `Unknown` in that enum is fine — security scanning still works (static file scan runs regardless of detected type). No code change needed in scanner.rs for this. Fixed above.
