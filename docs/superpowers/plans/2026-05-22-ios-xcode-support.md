# iOS (Xcode/Swift) Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add native iOS/Xcode/Swift Package Manager support to `raios build`, `test`, `deps`, and `version-info`/`version-bump`.

**Architecture:** Activate the `ProjectType::Ios` stub that was added in the Flutter plan. Detection: root-level `*.xcodeproj` or `*.xcworkspace` directory, or `Package.swift` (SPM). Build/Test wrap `xcodebuild` — if the tool is missing (non-macOS), a clear error is returned via the `Err` branch of `Command::output()`. Deps parse `Package.resolved` (SPM) and detect `Podfile.lock` (CocoaPods). Version reads `CFBundleShortVersionString` from `Info.plist`.

**Prerequisite:** Flutter plan (`2026-05-22-flutter-support.md`) must be implemented first — it adds `ProjectType::Ios` as a stub and updates `detect_type()`.

**Tech Stack:** Rust, xcodebuild (macOS), agvtool (optional), std::process::Command

**Test project:** Any Xcode project or SPM package (macOS only for live smoke test)

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Activate `Ios` detection; `build_ios_impl()`, `test_ios()`; `parse_xcodebuild_output()`, `parse_xcodebuild_test_output()` |
| `src/core/deps.rs` | Replace `Ios` stub with `check_ios()`; `parse_package_resolved()` |
| `src/core/version.rs` | Add `read_ios_version()`, `write_ios_version()`; wire into `read_version()` |

---

## Task 1 — Activate iOS Detection

**Files:** `src/core/build.rs`

The `Ios` variant and its `label()` were added as stubs in the Flutter plan. This task adds the real detection logic and wires up the dispatch arms.

### Step 1: Add failing tests for iOS detection

```rust
#[test]
fn detect_ios_xcodeproj_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("MyApp.xcodeproj")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn detect_ios_xcworkspace_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("MyApp.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn detect_ios_package_swift() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Package.swift"), "// swift-tools-version:5.9\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Ios);
}

#[test]
fn flutter_takes_priority_over_xcworkspace() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("pubspec.yaml"), "name: myapp\n").unwrap();
    // Flutter projects embed .xcworkspace under ios/ — root pubspec.yaml takes priority
    std::fs::create_dir_all(tmp.path().join("MyApp.xcworkspace")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Flutter);
}

#[test]
fn android_does_not_match_ios_check() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Android);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_ios\|flutter_takes_priority_over_xcworkspace" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — the `Ios` stub in `detect_type()` never returns `ProjectType::Ios`.

### Step 3: Replace the `Ios` stub in `detect_type()` with real logic

The iOS detection block (added as stub in Flutter plan) currently reads:
```rust
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
```

This is already correct — verify it's in `detect_type()` between the Flutter check and the Android check. If missing, add it now. No code change needed if the Flutter plan was implemented correctly.

### Step 4: Add real `build_ios` and `test_ios` stubs to replace the "not yet implemented" inline blocks

Replace the inline `BuildResult { ... "iOS support not yet implemented" ... }` blocks in `build()` and `test()` with:

In `build()`:
```rust
ProjectType::Ios => build_ios(dir),
```

In `test()`:
```rust
ProjectType::Ios => test_ios(dir),
```

Add stub functions (full impl in Tasks 2 & 3):
```rust
pub fn build_ios(dir: &Path) -> BuildResult {
    build_ios_impl(dir, "iphonesimulator")
}

fn build_ios_impl(dir: &Path, _sdk: &str) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: "iOS".into(), command: "xcodebuild build".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_ios(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: "iOS".into(), command: "xcodebuild test".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 5: Replace the `Ios` stub in `deps::check()` with real dispatch

```rust
ProjectType::Ios => check_ios(dir),
```

Add stub:
```rust
fn check_ios(dir: &Path) -> DepsReport {
    let _ = dir;
    DepsReport::empty("iOS")
}
```

### Step 6: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 7: Run detection tests

```powershell
cargo test "detect_ios\|detect_flutter\|detect_android\|flutter_takes" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: all pass.

### Step 8: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: activate iOS detection for xcodeproj/xcworkspace/Package.swift"
```

---

## Task 2 — iOS Build Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for `xcodebuild` output parser

```rust
#[test]
fn parse_xcodebuild_build_success() {
    let output = "** BUILD SUCCEEDED **\n\nBuild settings from command line:";
    let (ok, errors) = parse_xcodebuild_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_xcodebuild_build_failure_counts_errors() {
    let output = "/path/File.swift:10:5: error: use of undeclared type 'Foo'\n/path/File.swift:20:1: error: expected expression\n** BUILD FAILED **";
    let (ok, errors) = parse_xcodebuild_output(output);
    assert!(!ok);
    assert_eq!(errors, 2);
}

#[test]
fn parse_xcodebuild_counts_warnings() {
    let output = "/path/File.swift:5:3: warning: result of call is unused\n** BUILD SUCCEEDED **";
    let warnings = parse_xcodebuild_warnings(output);
    assert_eq!(warnings, 1);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_xcodebuild_build\|parse_xcodebuild_counts" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — functions don't exist yet.

### Step 3: Implement `parse_xcodebuild_output()`, `parse_xcodebuild_warnings()`, and replace `build_ios_impl()` stub

```rust
fn parse_xcodebuild_output(output: &str) -> (bool, usize) {
    let ok = output.contains("** BUILD SUCCEEDED **");
    let errors = output
        .lines()
        .filter(|l| l.contains(": error:") && !l.trim_start().starts_with("//"))
        .count();
    (ok, errors)
}

fn parse_xcodebuild_warnings(output: &str) -> usize {
    output
        .lines()
        .filter(|l| l.contains(": warning:") && !l.trim_start().starts_with("//"))
        .count()
}

pub fn build_ios(dir: &Path) -> BuildResult {
    build_ios_impl(dir, "iphonesimulator")
}

pub fn build_ios_release(dir: &Path) -> BuildResult {
    build_ios_impl(dir, "iphoneos")
}

pub fn build_ios_check(dir: &Path) -> BuildResult {
    // swift build --show-bin-path is cross-platform; use for SPM projects
    let has_spm = dir.join("Package.swift").exists();
    if has_spm {
        let cmd_str = "swift build";
        let start = Instant::now();
        let out = Command::new("swift").args(["build"]).current_dir(dir).output();
        let elapsed = start.elapsed();
        return match out {
            Err(e) => failed_result("iOS", cmd_str, elapsed, e.to_string()),
            Ok(o) => {
                let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                BuildResult {
                    ok: o.status.success(),
                    project_type: "iOS".into(),
                    command: cmd_str.into(),
                    duration_ms: elapsed.as_millis() as u64,
                    warnings: parse_xcodebuild_warnings(&raw),
                    errors: if o.status.success() { 0 } else { 1 },
                    diagnostics: vec![],
                    raw_output: raw,
                }
            }
        };
    }
    build_ios_impl(dir, "iphonesimulator")
}

fn build_ios_impl(dir: &Path, sdk: &str) -> BuildResult {
    let cmd_str = format!("xcodebuild -sdk {} build", sdk);
    let start = Instant::now();
    let output = Command::new("xcodebuild")
        .args(["-sdk", sdk, "build"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match output {
        Err(e) => failed_result("iOS", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (ok, errors) = parse_xcodebuild_output(&raw);
            BuildResult {
                ok,
                project_type: "iOS".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                warnings: parse_xcodebuild_warnings(&raw),
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
cargo test "parse_xcodebuild_build\|parse_xcodebuild_counts" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 3 passed`

### Step 5: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement xcodebuild wrapper for iOS build"
```

---

## Task 3 — iOS Test Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for xcodebuild test output parser

```rust
#[test]
fn parse_xcodebuild_test_pass() {
    let output = "Test Case '-[MyTests testExample]' passed (0.001 seconds).\nTest Suite 'All tests' passed at 2026-01-01.\n** TEST SUCCEEDED **";
    let (passed, failed) = parse_xcodebuild_test_output(output);
    assert_eq!(passed, 1);
    assert_eq!(failed, 0);
}

#[test]
fn parse_xcodebuild_test_mixed() {
    let output = "Test Case '-[MyTests testA]' passed (0.001 seconds).\nTest Case '-[MyTests testB]' failed (0.002 seconds).\n** TEST FAILED **";
    let (passed, failed) = parse_xcodebuild_test_output(output);
    assert_eq!(passed, 1);
    assert_eq!(failed, 1);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_xcodebuild_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_xcodebuild_test_output` doesn't exist yet.

### Step 3: Implement `parse_xcodebuild_test_output()` and `test_ios()`

```rust
fn parse_xcodebuild_test_output(output: &str) -> (usize, usize) {
    let passed = output
        .lines()
        .filter(|l| l.contains("passed (") && l.contains("Test Case"))
        .count();
    let failed = output
        .lines()
        .filter(|l| l.contains("failed (") && l.contains("Test Case"))
        .count();
    (passed, failed)
}

pub fn test_ios(dir: &Path) -> TestResult {
    // SPM projects: use `swift test`
    if dir.join("Package.swift").exists() {
        let cmd_str = "swift test";
        let start = Instant::now();
        let out = Command::new("swift").args(["test"]).current_dir(dir).output();
        let elapsed = start.elapsed();
        return match out {
            Err(e) => failed_test("iOS", cmd_str, elapsed, e.to_string()),
            Ok(o) => {
                let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                let (passed, failed) = parse_xcodebuild_test_output(&raw);
                TestResult {
                    ok: o.status.success(),
                    project_type: "iOS".into(),
                    command: cmd_str.into(),
                    duration_ms: elapsed.as_millis() as u64,
                    passed,
                    failed,
                    ignored: 0,
                    failures: raw.lines().filter(|l| l.contains("failed (") && l.contains("Test Case")).map(|l| l.trim().to_string()).collect(),
                    raw_output: raw,
                }
            }
        };
    }

    // Xcode projects: use xcodebuild test
    let dest = "platform=iOS Simulator,name=iPhone 15";
    let cmd_str = format!("xcodebuild test -destination '{}'", dest);
    let start = Instant::now();
    let output = Command::new("xcodebuild")
        .args(["test", "-destination", dest])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();

    match output {
        Err(e) => failed_test("iOS", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let (passed, failed) = parse_xcodebuild_test_output(&raw);
            TestResult {
                ok: o.status.success(),
                project_type: "iOS".into(),
                command: cmd_str,
                duration_ms: elapsed.as_millis() as u64,
                passed,
                failed,
                ignored: 0,
                failures: raw
                    .lines()
                    .filter(|l| l.contains("failed (") && l.contains("Test Case"))
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
cargo test "parse_xcodebuild_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 2 passed`

### Step 5: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement xcodebuild test for iOS (supports SPM and Xcode projects)"
```

---

## Task 4 — iOS Dependency Check

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for `Package.resolved` parser

SPM `Package.resolved` v2 format (Xcode 14+):
```json
{
  "pins": [
    { "identity": "swift-argument-parser", "location": "...", "state": { "version": "1.3.0" } }
  ],
  "version": 2
}
```

Older v1 format:
```json
{
  "object": { "pins": [
    { "package": "swift-argument-parser", "state": { "version": "1.2.0" } }
  ] }
}
```

```rust
#[test]
fn parse_package_resolved_v2() {
    let json = r#"{"pins":[{"identity":"swift-argument-parser","location":"https://github.com/apple/swift-argument-parser","state":{"version":"1.3.0"}}],"version":2}"#;
    let deps = parse_package_resolved(json);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "swift-argument-parser");
    assert_eq!(deps[0].current, "1.3.0");
}

#[test]
fn parse_package_resolved_v1() {
    let json = r#"{"object":{"pins":[{"package":"Alamofire","state":{"version":"5.8.1"}}]}}"#;
    let deps = parse_package_resolved(json);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "Alamofire");
    assert_eq!(deps[0].current, "5.8.1");
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_package_resolved" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `check_ios()` and `parse_package_resolved()`

```rust
fn check_ios(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("iOS");

    // SPM: Package.resolved
    if let Ok(content) = std::fs::read_to_string(dir.join("Package.resolved")) {
        report.has_lockfile = true;
        let deps = parse_package_resolved(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }

    // CocoaPods: Podfile.lock
    if dir.join("Podfile.lock").exists() {
        report.has_lockfile = true;
    }

    // Check toolchain availability
    if Command::new("xcodebuild").arg("-version").output().is_err()
        && Command::new("swift").arg("--version").output().is_err()
    {
        report.tool_missing.push(
            "xcodebuild or swift (requires macOS with Xcode or Swift toolchain installed)".into(),
        );
    }

    report
}

fn parse_package_resolved(content: &str) -> Vec<OutdatedDep> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
        return vec![];
    };

    // v2 format: { "pins": [...] }
    // v1 format: { "object": { "pins": [...] } }
    let pins = v["pins"]
        .as_array()
        .or_else(|| v["object"]["pins"].as_array());

    let Some(pins) = pins else { return vec![] };

    pins.iter()
        .filter_map(|pin| {
            // v2 uses "identity", v1 uses "package"
            let name = pin["identity"]
                .as_str()
                .or_else(|| pin["package"].as_str())?
                .to_string();
            let version = pin["state"]["version"].as_str().unwrap_or("?").to_string();
            Some(OutdatedDep {
                name,
                current: version,
                latest: "?".into(),
                kind: "spm".into(),
            })
        })
        .collect()
}
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "parse_package_resolved" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 2 passed`

### Step 5: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/deps.rs
git commit -m "feat: implement iOS dependency check with Package.resolved parsing"
```

---

## Task 5 — iOS Version Read/Write

**Files:** `src/core/version.rs`

### Step 1: Add failing tests for iOS version parsing

```rust
#[test]
fn read_ios_version_from_info_plist() {
    let tmp = tempfile::tempdir().unwrap();
    let plist_content = "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n<key>CFBundleShortVersionString</key>\n<string>2.4.1</string>\n</dict>\n</plist>";
    std::fs::write(tmp.path().join("Info.plist"), plist_content).unwrap();
    assert_eq!(read_ios_version(tmp.path()), Some("2.4.1".to_string()));
}

#[test]
fn extract_plist_key_finds_value() {
    let content = "<key>CFBundleShortVersionString</key>\n<string>1.2.3</string>";
    assert_eq!(extract_plist_key(content, "CFBundleShortVersionString"), Some("1.2.3".to_string()));
}

#[test]
fn extract_plist_key_returns_none_for_missing_key() {
    let content = "<key>OtherKey</key>\n<string>value</string>";
    assert_eq!(extract_plist_key(content, "CFBundleShortVersionString"), None);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "read_ios_version\|extract_plist_key" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `read_ios_version()` and `extract_plist_key()`

```rust
pub(crate) fn read_ios_version(dir: &Path) -> Option<String> {
    // Check common Info.plist locations
    for candidate in &[
        "Info.plist",
        "Sources/Info.plist",
        "App/Info.plist",
        "Resources/Info.plist",
    ] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            if let Some(v) = extract_plist_key(&content, "CFBundleShortVersionString") {
                if looks_like_semver(&v) {
                    return Some(v);
                }
            }
        }
    }
    None
}

pub(crate) fn extract_plist_key(content: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{}</key>", key);
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.trim() == key_tag {
            if let Some(value_line) = lines.next() {
                let trimmed = value_line.trim();
                if trimmed.starts_with("<string>") && trimmed.ends_with("</string>") {
                    return Some(
                        trimmed
                            .trim_start_matches("<string>")
                            .trim_end_matches("</string>")
                            .to_string(),
                    );
                }
            }
        }
    }
    None
}

pub(crate) fn write_ios_version(dir: &Path, old_version: &str, new_version: &str) -> Result<(), String> {
    // Update CFBundleShortVersionString in Info.plist (line-by-line replacement)
    for candidate in &["Info.plist", "Sources/Info.plist", "App/Info.plist"] {
        let path = dir.join(candidate);
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if !content.contains(&format!("<string>{}</string>", old_version)) {
            continue;
        }
        let updated = content.replace(
            &format!("<string>{}</string>", old_version),
            &format!("<string>{}</string>", new_version),
        );
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err(format!("Info.plist with version {} not found in expected locations", old_version))
}
```

### Step 4: Wire into `read_version()` — add iOS check after Flutter, before Android

```rust
if let Some(v) = read_ios_version(dir) {
    return Some((v, "iOS".into(), "Info.plist".into()));
}
```

### Step 5: Wire into `write_version()` — add `"iOS"` arm

```rust
"iOS" => write_ios_version(dir, old_version, new_version),
```

### Step 6: Run tests to verify they pass

```powershell
cargo test "read_ios_version\|extract_plist_key" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 3 passed`

### Step 7: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/version.rs
git commit -m "feat: implement iOS version read/write from Info.plist"
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

- [ ] **Step 4: Smoke test — iOS detection on a Package.swift project** (requires Swift toolchain)

```powershell
# On macOS: swift build; on Windows: xcodebuild not available, expect Err message
# .\target\release\raios.exe build <path-to-spm-package>
# Expected: project_type: "iOS", command: "swift build"
```

- [ ] **Step 5: Regression check — Rust projects still work**

```powershell
.\target\release\raios.exe build . --json 2>&1 | Select-String "project_type"
```

Expected: `"project_type": "Rust"`

- [ ] **Step 6: Commit and push**

```powershell
git add -A
git commit -m "chore: iOS support smoke test and final review"
git push origin master
```

---

## Self-Review Checklist

- [ ] `ProjectType::Ios` detection active (xcodeproj / xcworkspace / Package.swift)
- [ ] Flutter check comes before iOS check in `detect_type()` — no Flutter regression
- [ ] `build_ios_impl()` calls `xcodebuild -sdk <sdk> build`; missing tool returns `Err` result
- [ ] `build_ios_check()` uses `swift build` for SPM projects (cross-platform)
- [ ] `test_ios()` uses `swift test` for SPM, `xcodebuild test` for Xcode
- [ ] `check_ios()` parses `Package.resolved` v1 and v2 formats
- [ ] `CocoaPods` (`Podfile.lock`) detected as lockfile present
- [ ] `read_ios_version()` reads `CFBundleShortVersionString` from `Info.plist`
- [ ] `write_ios_version()` updates `<string>` tag in `Info.plist`
- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] No regression on Rust/Node/Python/Go/Flutter/Android projects
