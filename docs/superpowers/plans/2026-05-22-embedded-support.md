# Embedded (ESP/Arduino/STM) Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add embedded project support to `raios build`, `test`, `deps`, and `version-info` for three toolchains: ESP-IDF (`idf.py`), PlatformIO (`platformio.ini`), and Arduino (`*.ino` + `arduino-cli`).

**Architecture:** Add `ProjectType::Embedded` to the enum. A secondary `EmbeddedKind` enum (internal to `build.rs`) distinguishes the toolchain (`EspIdf`, `PlatformIo`, `Arduino`). Detection order: `idf.py` → `platformio.ini` → any `*.ino` file at root. Detection runs **after** Android so Android projects with embedded subdirs aren't misidentified. All three wrap external CLI tools; if the tool is missing, `Command::output()` returns `Err` and the caller receives a descriptive failure result.

**Prerequisite:** Flutter plan must be implemented first (adds `Ios`, `Flutter` to enum).

**Tech Stack:** Rust, idf.py (ESP-IDF), platformio CLI, arduino-cli, std::process::Command

**Test projects:**
- ESP-IDF: project with `idf.py` at root
- PlatformIO: project with `platformio.ini`
- Arduino: any `.ino` sketch directory

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::Embedded`, `EmbeddedKind`; `detect_embedded_kind()`; `build_embedded()`, `test_embedded()`; per-toolchain helpers; parsers |
| `src/core/deps.rs` | Add `ProjectType::Embedded` arm; `check_embedded()` with per-toolchain parsing; `parse_idf_dependencies()` |
| `src/core/version.rs` | Add `read_embedded_version()`; wire into `read_version()` |

---

## Task 1 — Embedded Detection

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for embedded detection

```rust
#[test]
fn detect_esp_idf_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("idf.py")).unwrap();
    std::fs::File::create(tmp.path().join("CMakeLists.txt")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Embedded);
}

#[test]
fn detect_platformio_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("platformio.ini"), "[env:esp32dev]\nplatform = espressif32\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Embedded);
}

#[test]
fn detect_arduino_ino_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("mysketch.ino"), "void setup() {}\nvoid loop() {}\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Embedded);
}

#[test]
fn android_takes_priority_over_embedded() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("gradlew")).unwrap();
    std::fs::File::create(tmp.path().join("build.gradle")).unwrap();
    std::fs::File::create(tmp.path().join("idf.py")).unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Android);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_esp_idf\|detect_platformio\|detect_arduino\|android_takes_priority_over_embedded" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — `ProjectType::Embedded` doesn't exist yet.

### Step 3: Add `Embedded` to `ProjectType` enum

Replace enum and label():
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
    Embedded,
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
            Self::Embedded => "Embedded",
            Self::Unknown => "Unknown",
        }
    }
}
```

Add `EmbeddedKind` (not serialized, internal):
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum EmbeddedKind {
    EspIdf,
    PlatformIo,
    Arduino,
}
```

Add `detect_embedded_kind()`:
```rust
fn detect_embedded_kind(dir: &Path) -> Option<EmbeddedKind> {
    if dir.join("idf.py").exists() {
        return Some(EmbeddedKind::EspIdf);
    }
    if dir.join("platformio.ini").exists() {
        return Some(EmbeddedKind::PlatformIo);
    }
    if std::fs::read_dir(dir).ok().map_or(false, |entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("ino"))
    }) {
        return Some(EmbeddedKind::Arduino);
    }
    None
}
```

### Step 4: Add Embedded detection to `detect_type()` — after Android, before Unknown

```rust
    if detect_embedded_kind(dir).is_some() {
        return ProjectType::Embedded;
    }
    ProjectType::Unknown
```

### Step 5: Add `Embedded` arm to `build()` and `test()` dispatch with stubs

In `build()`:
```rust
ProjectType::Embedded => build_embedded(dir),
```

In `test()`:
```rust
ProjectType::Embedded => test_embedded(dir),
```

Stub functions:
```rust
pub fn build_embedded(dir: &Path) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: "Embedded".into(), command: "—".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_embedded(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: "Embedded".into(), command: "—".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 6: Add `Embedded` arm to `deps::check()`

```rust
ProjectType::Embedded => check_embedded(dir),
```

Stub:
```rust
fn check_embedded(dir: &Path) -> DepsReport {
    let _ = dir;
    DepsReport::empty("Embedded")
}
```

### Step 7: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 8: Run detection tests

```powershell
cargo test "detect_esp_idf\|detect_platformio\|detect_arduino\|android_takes_priority_over_embedded" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: all pass.

### Step 9: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add ProjectType::Embedded detection for ESP-IDF, PlatformIO, Arduino"
```

---

## Task 2 — Embedded Build Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for embedded build output parsers

```rust
#[test]
fn parse_idf_build_success() {
    let output = "Project build complete. To flash, run this command:\nidf.py -p PORT flash";
    let (ok, errors) = parse_idf_build_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_idf_build_failure() {
    let output = "error: 'undefined_var' undeclared\nCMake Error at CMakeLists.txt:10\nninja: build stopped: subcommand failed.";
    let (ok, errors) = parse_idf_build_output(output);
    assert!(!ok);
    assert!(errors >= 1);
}

#[test]
fn parse_pio_build_success() {
    let output = "Environment esp32dev    [SUCCESS]\n=== 1 succeeded in 00:23.456 ===";
    let (ok, _) = parse_pio_output(output);
    assert!(ok);
}

#[test]
fn parse_pio_build_failure() {
    let output = "Environment esp32dev    [FAILED]\n=== 0 succeeded in 00:05.123, 1 failed ===";
    let (ok, _) = parse_pio_output(output);
    assert!(!ok);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_idf_build\|parse_pio_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement parsers and `build_embedded()`

```rust
fn parse_idf_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Project build complete") || output.contains("BUILD SUCCESSFUL");
    let errors = output
        .lines()
        .filter(|l| {
            (l.contains("error:") || l.starts_with("error:") || l.contains("CMake Error"))
                && !l.trim_start().starts_with("//")
        })
        .count();
    (ok, errors)
}

fn parse_pio_output(output: &str) -> (bool, usize) {
    let ok = output.contains("[SUCCESS]") && !output.contains("[FAILED]");
    let errors = if !ok { 1 } else { 0 };
    (ok, errors)
}

fn parse_arduino_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Sketch uses") || output.contains("used by sketch");
    let errors = if !ok && (output.contains("error:") || output.contains("Error:")) { 1 } else { 0 };
    (ok, errors)
}

pub fn build_embedded(dir: &Path) -> BuildResult {
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => build_esp_idf(dir),
        Some(EmbeddedKind::PlatformIo) => build_platformio(dir),
        Some(EmbeddedKind::Arduino) => build_arduino(dir),
        None => failed_result(
            "Embedded",
            "—",
            std::time::Duration::ZERO,
            "No embedded toolchain found (idf.py, platformio.ini, or *.ino)".into(),
        ),
    }
}

fn build_esp_idf(dir: &Path) -> BuildResult {
    let cmd_str = "idf.py build";
    let start = Instant::now();
    let output = Command::new("idf.py").arg("build").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/ESP-IDF", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_idf_build_output(&raw);
            BuildResult { ok, project_type: "Embedded/ESP-IDF".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}

fn build_platformio(dir: &Path) -> BuildResult {
    let cmd_str = "pio run";
    let start = Instant::now();
    let output = Command::new("pio").arg("run").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/PlatformIO", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_pio_output(&raw);
            BuildResult { ok, project_type: "Embedded/PlatformIO".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}

fn build_arduino(dir: &Path) -> BuildResult {
    let fqbn = "arduino:avr:uno";
    let cmd_str = format!("arduino-cli compile --fqbn {}", fqbn);
    let start = Instant::now();
    let output = Command::new("arduino-cli")
        .args(["compile", "--fqbn", fqbn])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("Embedded/Arduino", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_arduino_output(&raw);
            BuildResult { ok, project_type: "Embedded/Arduino".into(), command: cmd_str,
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "parse_idf_build\|parse_pio_build\|parse_pio_output" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: `test result: ok. 4 passed`

### Step 5: cargo check + commit

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement embedded build for ESP-IDF, PlatformIO, Arduino"
```

---

## Task 3 — Embedded Test Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing test for PlatformIO test output parser

```rust
#[test]
fn parse_pio_test_all_pass() {
    let output = "PASSED (1 test, 5 assertions)\nTest      Assertions  Passed  Failed\nexample   5           5       0";
    let (passed, failed) = parse_pio_test_output(output);
    assert!(passed >= 1);
    assert_eq!(failed, 0);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_pio_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `test_embedded()` and `parse_pio_test_output()`

```rust
fn parse_pio_test_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.trim_start().starts_with("PASSED") {
            let passed = line.split('(').nth(1)
                .and_then(|s| s.split(' ').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1);
            return (passed, 0);
        }
        if line.trim_start().starts_with("FAILED") {
            return (0, 1);
        }
    }
    (0, 0)
}

pub fn test_embedded(dir: &Path) -> TestResult {
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => {
            let cmd_str = "idf.py build -T test";
            let start = Instant::now();
            let out = Command::new("idf.py")
                .args(["build", "-T", "test"])
                .current_dir(dir)
                .output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("Embedded/ESP-IDF", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let ok = o.status.success();
                    TestResult { ok, project_type: "Embedded/ESP-IDF".into(), command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64, passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 }, ignored: 0, failures: vec![], raw_output: raw }
                }
            }
        }
        Some(EmbeddedKind::PlatformIo) => {
            let cmd_str = "pio test";
            let start = Instant::now();
            let out = Command::new("pio").arg("test").current_dir(dir).output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("Embedded/PlatformIO", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let (passed, failed) = parse_pio_test_output(&raw);
                    TestResult { ok: o.status.success(), project_type: "Embedded/PlatformIO".into(),
                        command: cmd_str.into(), duration_ms: elapsed.as_millis() as u64,
                        passed, failed, ignored: 0, failures: vec![], raw_output: raw }
                }
            }
        }
        Some(EmbeddedKind::Arduino) | None => TestResult {
            ok: false,
            project_type: "Embedded/Arduino".into(),
            command: "—".into(),
            duration_ms: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
            failures: vec!["Arduino-CLI does not support unit testing; use PlatformIO or ESP-IDF for testable firmware".into()],
            raw_output: String::new(),
        },
    }
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_pio_test" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement embedded test for ESP-IDF and PlatformIO"
```

---

## Task 4 — Embedded Dependency Check

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for `idf_component.yml` parser

```rust
#[test]
fn parse_idf_component_yml() {
    let content = "dependencies:\n  idf: \">=5.0\"\n  espressif/button: \"^2.0.0\"\n  esp_lcd_touch: \"1.0.0\"\n";
    let deps = parse_idf_dependencies(content);
    assert_eq!(deps.len(), 2); // excludes idf itself
    assert!(deps.iter().any(|d| d.name == "espressif/button" && d.current == "^2.0.0"));
    assert!(deps.iter().any(|d| d.name == "esp_lcd_touch" && d.current == "1.0.0"));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_idf_component" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `check_embedded()` and `parse_idf_dependencies()`

```rust
fn check_embedded(dir: &Path) -> DepsReport {
    use crate::core::build::detect_embedded_kind;
    use crate::core::build::EmbeddedKind;
    match detect_embedded_kind(dir) {
        Some(EmbeddedKind::EspIdf) => check_esp_idf_deps(dir),
        Some(EmbeddedKind::PlatformIo) => check_platformio_deps(dir),
        Some(EmbeddedKind::Arduino) | None => DepsReport::empty("Embedded/Arduino"),
    }
}

fn check_esp_idf_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Embedded/ESP-IDF");
    for candidate in &["idf_component.yml", "main/idf_component.yml"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            let deps = parse_idf_dependencies(&content);
            report.outdated_count = deps.len();
            report.outdated = deps;
            report.has_lockfile = true;
            break;
        }
    }
    if Command::new("idf.py").arg("--version").output().is_err() {
        report.tool_missing.push("idf.py (ESP-IDF not in PATH; source export.sh from ESP-IDF root)".into());
    }
    report
}

fn check_platformio_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("Embedded/PlatformIO");
    report.has_lockfile = dir.join(".pio").join("libdeps").exists();
    if Command::new("pio").arg("--version").output().is_err() {
        report.tool_missing.push("pio (install: pip install platformio)".into());
    }
    report
}

fn parse_idf_dependencies(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        if line.trim() == "dependencies:" {
            in_deps = true;
            continue;
        }
        if in_deps {
            if !line.starts_with(' ') && !line.trim().is_empty() {
                break; // Left dependencies block
            }
            let trimmed = line.trim();
            if let Some((name, version)) = trimmed.split_once(':') {
                let name = name.trim().to_string();
                let version = version.trim().trim_matches('"').to_string();
                if name != "idf" && !name.is_empty() {
                    deps.push(OutdatedDep { name, current: version, latest: "?".into(), kind: "component".into() });
                }
            }
        }
    }
    deps
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_idf_component" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/deps.rs
git commit -m "feat: implement embedded dependency check for ESP-IDF and PlatformIO"
```

---

## Task 5 — Embedded Version Read

**Files:** `src/core/version.rs`

### Step 1: Add failing tests

```rust
#[test]
fn read_embedded_version_from_version_h() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("main")).unwrap();
    std::fs::write(tmp.path().join("main/version.h"), "#pragma once\n#define APP_VERSION \"1.3.0\"\n").unwrap();
    assert_eq!(read_embedded_version(tmp.path()), Some("1.3.0".to_string()));
}

#[test]
fn read_embedded_version_from_cmake() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.16)\nproject(my_app VERSION 2.0.1)\n").unwrap();
    assert_eq!(read_embedded_version(tmp.path()), Some("2.0.1".to_string()));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "read_embedded_version" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `read_embedded_version()`

```rust
pub(crate) fn read_embedded_version(dir: &Path) -> Option<String> {
    // 1. version.h / src/version.h / main/version.h: #define APP_VERSION "x.y.z"
    for candidate in &["version.h", "src/version.h", "main/version.h", "include/version.h"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(candidate)) {
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with("#define") && (t.contains("VERSION") || t.contains("version")) {
                    let parts: Vec<&str> = t.splitn(3, ' ').collect();
                    if parts.len() == 3 {
                        let val = parts[2].trim().trim_matches('"').trim_matches('\'');
                        if looks_like_semver(val) {
                            return Some(val.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. CMakeLists.txt: project(name VERSION x.y.z) — single-line form
    if let Ok(content) = std::fs::read_to_string(dir.join("CMakeLists.txt")) {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("project(") && t.contains("VERSION") {
                let after = t.split("VERSION").nth(1).unwrap_or("").trim();
                let version = after.split([' ', ')', '\n']).next().unwrap_or("").trim();
                if looks_like_semver(version) {
                    return Some(version.to_string());
                }
            }
        }
    }

    // 3. platformio.ini: version = x.y.z
    if let Ok(content) = std::fs::read_to_string(dir.join("platformio.ini")) {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("version") && t.contains('=') {
                let val = t.split('=').nth(1).unwrap_or("").trim().trim_matches('"');
                if looks_like_semver(val) {
                    return Some(val.to_string());
                }
            }
        }
    }

    None
}
```

### Step 4: Wire into `read_version()` — add after iOS, before Android

```rust
if let Some(v) = read_embedded_version(dir) {
    return Some((v, "Embedded".into(), "version.h / CMakeLists.txt".into()));
}
```

### Step 5: Run tests + cargo check + commit

```powershell
cargo test "read_embedded_version" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/version.rs
git commit -m "feat: implement embedded version read from version.h and CMakeLists.txt"
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

- [ ] **Step 4: Smoke test with embedded projects** (if toolchain available)

```powershell
# PlatformIO: .\target\release\raios.exe build <pio-project-path>
# ESP-IDF:   .\target\release\raios.exe build <esp-idf-project-path>
```

- [ ] **Step 5: Regression check**

```powershell
.\target\release\raios.exe build . --json 2>&1 | Select-String "project_type"
```

Expected: `"project_type": "Rust"` — no regression.

- [ ] **Step 6: Commit and push**

```powershell
git add -A
git commit -m "chore: embedded support smoke test and final review"
git push origin master
```

---

## Self-Review Checklist

- [ ] `ProjectType::Embedded` added with `label()` → `"Embedded"`
- [ ] `EmbeddedKind` distinguishes EspIdf / PlatformIo / Arduino
- [ ] `detect_embedded_kind()` checks `idf.py` first, then `platformio.ini`, then `*.ino`
- [ ] `detect_type()` runs embedded check after Android (Android takes priority)
- [ ] `build_embedded()` dispatches to per-toolchain builder; missing tool → `Err` result
- [ ] `test_embedded()` returns descriptive note for Arduino (no native unit test support)
- [ ] `check_embedded()` parses `idf_component.yml` for ESP-IDF; detects `.pio/libdeps` for PlatformIO
- [ ] `read_embedded_version()` reads from `version.h`, `CMakeLists.txt VERSION`, `platformio.ini`
- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] No regression on Rust/Node/Python/Go/Flutter/iOS/Android projects
