# Flutter Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Flutter/Dart support to `raios build`, `test`, `deps`, and `version-info`/`version-bump` so Flutter projects (pubspec.yaml) are detected and handled instead of returning "Unknown project type".

**Architecture:** Add `ProjectType::Flutter` to the enum in `src/core/build.rs`. Detection triggers on `pubspec.yaml`. Each subsystem gets Flutter-specific functions that wrap the `flutter` CLI. `detect_type()` checks `pubspec.yaml` **before** the iOS check so a Flutter project with an `ios/Runner.xcworkspace` isn't misidentified. Version lives in the `version:` field of `pubspec.yaml`.

**Tech Stack:** Rust, clap (CLI), std::process::Command (flutter CLI), serde_json

**Test project:** Any Flutter app — create a minimal one with `flutter create /tmp/testapp`

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::Flutter`; `build_flutter()`, `test_flutter()`; `parse_flutter_build_output()`, `parse_flutter_test_output()` |
| `src/core/deps.rs` | Add `ProjectType::Flutter` arm in `check()`; `check_flutter()` parsing `pubspec.lock` |
| `src/core/version.rs` | Add `read_flutter_version()`, `write_flutter_version()`; extend `read_version()` and `write_version()` |

---

## Task 1 — Flutter Detection

**Files:** `src/core/build.rs`, `src/core/deps.rs`

### Step 1: Add a failing test for Flutter detection

In `src/core/build.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn detect_flutter_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\nversion: 1.0.0+1\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn flutter_takes_priority_over_ios_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    std::fs::create_dir_all(tmp.path().join("ios/Runner.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn node_takes_priority_over_flutter() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), "{\"name\":\"x\"}").unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Node);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_flutter\|flutter_takes" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `ProjectType::Flutter` doesn't exist yet.

### Step 3: Add `Flutter` to `ProjectType` enum in `src/core/build.rs`

Replace the enum and its `label()` impl:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Flutter,
    Ios,
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
            Self::Flutter => "Flutter",
            Self::Ios => "iOS",
            Self::Android => "Android",
            Self::Unknown => "Unknown",
        }
    }
}
```

> Note: `Ios` is added here as a placeholder (non-functional stub) until the iOS plan is implemented. It must exist so all match arms remain exhaustive.

### Step 4: Add Flutter detection to `detect_type()` — insert after `Go`, before `Android`

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
    // Flutter before iOS: Flutter projects contain an ios/ folder with .xcworkspace
    if dir.join("pubspec.yaml").exists() {
        return ProjectType::Flutter;
    }
    // iOS: .xcodeproj/.xcworkspace at root level, or Package.swift (SPM)
    if dir.join("Package.swift").exists()
        || std::fs::read_dir(dir).ok().map_or(false, |entries| {
            entries.flatten().any(|e| {
                matches!(
                    e.path().extension().and_then(|s| s.to_str()),
                    Some("xcodeproj" | "xcworkspace")
                )
            })
        })
    {
        return ProjectType::Ios;
    }
    if (dir.join("gradlew").exists() || dir.join("gradlew.bat").exists())
        && (dir.join("build.gradle").exists() || dir.join("settings.gradle").exists())
    {
        return ProjectType::Android;
    }
    ProjectType::Unknown
}
```

### Step 5: Add `Flutter` and `Ios` stubs to `build()` and `test()` dispatch

In `build()`, add after the `Go` arm:
```rust
ProjectType::Flutter => build_flutter(dir),
ProjectType::Ios => BuildResult {
    ok: false,
    project_type: "iOS".into(),
    command: "xcodebuild".into(),
    duration_ms: 0,
    warnings: 0,
    errors: 1,
    diagnostics: vec![],
    raw_output: "iOS support not yet implemented (see 2026-05-22-ios-xcode-support.md)".into(),
},
```

In `test()`, add after the `Go` arm:
```rust
ProjectType::Flutter => test_flutter(dir),
ProjectType::Ios => TestResult {
    ok: false,
    project_type: "iOS".into(),
    command: "xcodebuild test".into(),
    duration_ms: 0,
    passed: 0,
    failed: 0,
    ignored: 0,
    failures: vec!["iOS support not yet implemented".into()],
    raw_output: String::new(),
},
```

Add temporary stub functions (full impl in Tasks 2 & 3):
```rust
pub fn build_flutter(dir: &Path) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: "Flutter".into(), command: "flutter build".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_flutter(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: "Flutter".into(), command: "flutter test".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 6: Add Flutter + Ios arms to `deps::check()` in `src/core/deps.rs`

```rust
ProjectType::Flutter => check_flutter(dir),
ProjectType::Ios => {
    let mut r = DepsReport::empty("iOS");
    r.tool_missing.push("iOS support not yet implemented".into());
    r
},
```

Add stub:
```rust
fn check_flutter(dir: &Path) -> DepsReport {
    let _ = dir;
    DepsReport::empty("Flutter")
}
```

### Step 7: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 8: Run detection tests

```powershell
cargo test "detect_flutter\|detect_android\|detect_rust" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: all pass.

### Step 9: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add ProjectType::Flutter detection for pubspec.yaml projects"
```

---

## Task 2 — Flutter Build Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for Flutter build output parser

```rust
#[test]
fn parse_flutter_build_success() {
    let output = "Running Gradle task 'assembleRelease'...\nBuilt build/app/outputs/apk/release/app-release.apk (7.4MB)";
    let (ok, errors) = parse_flutter_build_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_flutter_build_failure() {
    let output = "Error: A JDK was not found.\nFailed to execute gradle";
    let (ok, errors) = parse_flutter_build_output(output);
    assert!(!ok);
    assert!(errors >= 1);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_flutter_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_flutter_build_output` doesn't exist yet.

### Step 3: Add `parse_flutter_build_output()` and replace `build_flutter()` stub

```rust
fn parse_flutter_build_output(output: &str) -> (bool, usize) {
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

pub fn build_flutter(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["build", "apk"])
}

pub fn build_flutter_release(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["build", "apk", "--release"])
}

pub fn build_flutter_check(dir: &Path) -> BuildResult {
    build_flutter_impl(dir, &["analyze"])
}

fn build_flutter_impl(dir: &Path, args: &[&str]) -> BuildResult {
    let cmd_str = format!("flutter {}", args.join(" "));
    let start = Instant::now();
    let output = Command::new("flutter")
        .args(args)
        .current_dir(dir)
        .output();
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
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "parse_flutter_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 2 passed`

### Step 5: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 6: Commit

```powershell
git add src/core/build.rs
git commit -m "feat: implement flutter build wrapper with output parser"
```

---

## Task 3 — Flutter Test Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for Flutter test output parser

Flutter test summary format: `MM:SS +passed -failed: message`

```rust
#[test]
fn parse_flutter_test_all_pass() {
    let output = "00:03 +42: All tests passed!\n";
    let (passed, failed) = parse_flutter_test_output(output);
    assert_eq!(passed, 42);
    assert_eq!(failed, 0);
}

#[test]
fn parse_flutter_test_partial_failure() {
    let output = "00:05 +38 -3: Some tests failed.\n";
    let (passed, failed) = parse_flutter_test_output(output);
    assert_eq!(passed, 38);
    assert_eq!(failed, 3);
}

#[test]
fn parse_flutter_test_empty_output() {
    let (passed, failed) = parse_flutter_test_output("");
    assert_eq!(passed, 0);
    assert_eq!(failed, 0);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_flutter_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_flutter_test_output` doesn't exist yet.

### Step 3: Add `parse_flutter_test_output()` and replace `test_flutter()` stub

```rust
fn parse_flutter_test_output(output: &str) -> (usize, usize) {
    // Lines look like: "00:05 +38 -3: Some tests failed." or "00:03 +42: All tests passed!"
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if trimmed.len() > 6 && trimmed.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            let rest = trimmed.splitn(2, ' ').nth(1).unwrap_or("");
            let passed = rest
                .split_whitespace()
                .find(|w| w.starts_with('+'))
                .and_then(|w| w[1..].parse::<usize>().ok())
                .unwrap_or(0);
            let failed = rest
                .split_whitespace()
                .find(|w| w.starts_with('-'))
                .and_then(|w| w[1..].parse::<usize>().ok())
                .unwrap_or(0);
            if passed > 0 || failed > 0 {
                return (passed, failed);
            }
        }
    }
    (0, 0)
}

pub fn test_flutter(dir: &Path) -> TestResult {
    let cmd_str = "flutter test";
    let start = Instant::now();
    let output = Command::new("flutter")
        .args(["test"])
        .current_dir(dir)
        .output();
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
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "parse_flutter_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 3 passed`

### Step 5: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 6: Commit

```powershell
git add src/core/build.rs
git commit -m "feat: implement flutter test wrapper with output parser"
```

---

## Task 4 — Flutter Dependency Check

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for `pubspec.lock` parser

```rust
#[test]
fn parse_pubspec_lock_packages() {
    let content = r#"packages:
  flutter:
    dependency: sdk
    version: "3.19.0"
  http:
    dependency: "direct main"
    version: "1.2.0"
  dio:
    dependency: "direct main"
    version: "5.4.0"
"#;
    let deps = parse_pubspec_lock(content);
    // Excludes sdk packages, includes direct deps
    assert_eq!(deps.len(), 2);
    assert!(deps.iter().any(|d| d.name == "http" && d.current == "1.2.0"));
    assert!(deps.iter().any(|d| d.name == "dio" && d.current == "5.4.0"));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_pubspec_lock" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_pubspec_lock` doesn't exist yet.

### Step 3: Implement `check_flutter()` and `parse_pubspec_lock()`

```rust
fn check_flutter(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Flutter");
    let lock_path = dir.join("pubspec.lock");
    report.has_lockfile = lock_path.exists();

    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        let deps = parse_pubspec_lock(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }

    if Command::new("flutter").arg("--version").output().is_err() {
        report.tool_missing.push(
            "flutter (install from https://docs.flutter.dev/get-started/install)".into(),
        );
    }

    report
}

fn parse_pubspec_lock(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut in_packages = false;
    let mut current_name = String::new();
    let mut current_version = String::new();
    let mut is_sdk = false;

    for line in content.lines() {
        if line.trim() == "packages:" {
            in_packages = true;
            continue;
        }
        if !in_packages {
            continue;
        }
        // Top-level package entry: exactly 2-space indent + "name:"
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            if !current_name.is_empty() && !is_sdk && !current_version.is_empty() {
                deps.push(OutdatedDep {
                    name: current_name.clone(),
                    current: current_version.clone(),
                    latest: "?".into(),
                    kind: "direct".into(),
                });
            }
            current_name = line.trim().trim_end_matches(':').to_string();
            current_version.clear();
            is_sdk = false;
        }
        if line.trim_start().starts_with("version:") {
            current_version = line
                .split(':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if line.trim_start().starts_with("dependency:") && line.contains("sdk") {
            is_sdk = true;
        }
    }
    if !current_name.is_empty() && !is_sdk && !current_version.is_empty() {
        deps.push(OutdatedDep {
            name: current_name,
            current: current_version,
            latest: "?".into(),
            kind: "direct".into(),
        });
    }
    deps
}
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "parse_pubspec_lock" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 1 passed`

### Step 5: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/deps.rs
git commit -m "feat: implement flutter dependency check with pubspec.lock parsing"
```

---

## Task 5 — Flutter Version Read/Write

**Files:** `src/core/version.rs`

### Step 1: Add failing test

```rust
#[test]
fn read_flutter_version_with_build_number() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\nversion: 2.3.1+7\n").unwrap();
    assert_eq!(read_flutter_version(tmp.path()), Some("2.3.1".to_string()));
}

#[test]
fn read_flutter_version_without_build_number() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "version: 1.0.0\n").unwrap();
    assert_eq!(read_flutter_version(tmp.path()), Some("1.0.0".to_string()));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "read_flutter_version" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `read_flutter_version()` and `write_flutter_version()`

```rust
pub(crate) fn read_flutter_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("pubspec.yaml")).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version:") {
            let val = trimmed.trim_start_matches("version:").trim();
            // "2.3.1+7" → "2.3.1"
            let semver = val.split('+').next().unwrap_or(val).trim_matches(|c| c == '"' || c == '\'');
            if looks_like_semver(semver) {
                return Some(semver.to_string());
            }
        }
    }
    None
}

pub(crate) fn write_flutter_version(dir: &Path, _old: &str, new_version: &str) -> Result<(), String> {
    let path = dir.join("pubspec.yaml");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated: String = content
        .lines()
        .map(|line| {
            if line.trim().starts_with("version:") {
                let new_val = if let Some(build_part) = line.split('+').nth(1) {
                    let n: u64 = build_part.trim().parse().unwrap_or(0) + 1;
                    format!("{}+{}", new_version, n)
                } else {
                    new_version.to_string()
                };
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{}version: {}", indent, new_val)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let updated = if content.ends_with('\n') { format!("{}\n", updated) } else { updated };
    std::fs::write(&path, updated).map_err(|e| e.to_string())
}
```

### Step 4: Wire into `read_version()` — add before Android check

```rust
if let Some(v) = read_flutter_version(dir) {
    return Some((v, "Flutter".into(), "pubspec.yaml".into()));
}
```

### Step 5: Wire into `write_version()` — add `"Flutter"` arm

In the `write_version()` match, add:
```rust
"Flutter" => write_flutter_version(dir, old_version, new_version),
```

### Step 6: Run tests to verify they pass

```powershell
cargo test "read_flutter_version" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 2 passed`

### Step 7: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/version.rs
git commit -m "feat: implement flutter version read/write from pubspec.yaml"
```

---

## Task 6 — Smoke Test & Final Review

- [ ] **Step 1: Run full test suite**

```powershell
cargo test -- --nocapture 2>&1 | Select-Object -Last 15
```

- [ ] **Step 2: Run cargo clippy**

```powershell
cargo clippy -- -D warnings 2>&1 | Select-Object -Last 10
```

- [ ] **Step 3: Build release binary**

```powershell
cargo build --release 2>&1 | Select-Object -Last 5
```

- [ ] **Step 4: Smoke test — current R-AI-OS project should still detect Rust**

```powershell
.\target\release\raios.exe build . --json 2>&1 | Select-String "project_type"
```

Expected: `"project_type": "Rust"` — no regression.

- [ ] **Step 5: Smoke test — detection on a Flutter project**

```powershell
# Create minimal Flutter project (requires flutter installed)
# flutter create C:\Temp\flutter_test_app
# .\target\release\raios.exe version-info C:\Temp\flutter_test_app
```

Expected: `project_type: "Flutter"`, version from pubspec.yaml.

- [ ] **Step 6: Commit and push**

```powershell
git add -A
git commit -m "chore: flutter support smoke test and final review"
git push origin master
```

---

## Self-Review Checklist

- [ ] `ProjectType::Flutter` added with `label()` → `"Flutter"`
- [ ] `detect_type()` checks `pubspec.yaml` before the iOS check
- [ ] `build()` routes Flutter → `build_flutter()`
- [ ] `test()` routes Flutter → `test_flutter()`
- [ ] `deps::check()` routes Flutter → `check_flutter()`
- [ ] `pubspec.lock` parsed for installed package versions
- [ ] `read_version()` reads `version:` field, strips `+build_number`
- [ ] `write_version()` updates `pubspec.yaml` and increments build number
- [ ] `Ios` stub present in enum + all match arms (non-functional placeholder)
- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] No regression on Rust/Node/Python/Go/Android projects
