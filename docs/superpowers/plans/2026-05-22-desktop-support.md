# Desktop (.NET & C++) Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add .NET/C# and C++/CMake desktop project support to `raios build`, `test`, `deps`, and `version-info`.

**Architecture:** Add two new `ProjectType` variants: `DotNet` (detected by `*.csproj` or `*.sln` at root) and `Cpp` (detected by `CMakeLists.txt` — after Embedded takes priority for `idf.py`/PlatformIO projects). Build wraps `dotnet build` / `cmake --build`. Test wraps `dotnet test` / `ctest`. Version reads `<Version>` tag from `*.csproj` for .NET; `project(...VERSION...)` from `CMakeLists.txt` for C++ (same as Embedded, but scoped here for non-embedded CMake).

**Prerequisite:** IaC plan must be implemented first (adds `Iac` to enum).

**Tech Stack:** Rust, dotnet CLI, cmake + ctest, std::process::Command

**Test projects:**
- .NET: `dotnet new console -n MyApp` creates a `MyApp.csproj`
- C++: any directory with `CMakeLists.txt` and no `idf.py` / `platformio.ini`

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::DotNet`, `ProjectType::Cpp`; `build_dotnet()`, `test_dotnet()`, `build_cpp()`, `test_cpp()`; parsers |
| `src/core/deps.rs` | Add `DotNet` and `Cpp` arms; `check_dotnet()`, `check_cpp()` |
| `src/core/version.rs` | Add `read_dotnet_version()`, `write_dotnet_version()`, `read_cpp_version()`; wire into `read_version()` |

---

## Task 1 — Desktop Detection

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for .NET and C++ detection

```rust
#[test]
fn detect_dotnet_csproj() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("MyApp.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::DotNet);
}

#[test]
fn detect_dotnet_sln() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("MySolution.sln"), "Microsoft Visual Studio Solution File\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::DotNet);
}

#[test]
fn detect_cpp_cmake() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)\nproject(MyApp)\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Cpp);
}

#[test]
fn embedded_takes_priority_over_cpp_cmake() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::File::create(tmp.path().join("idf.py")).unwrap();
    std::fs::write(tmp.path().join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Embedded);
}

#[test]
fn iac_takes_priority_over_cpp() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("main.tf"), "terraform {}\n").unwrap();
    std::fs::write(tmp.path().join("CMakeLists.txt"), "project(MyApp)\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Iac);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_dotnet\|detect_cpp\|embedded_takes_priority_over_cpp\|iac_takes_priority_over_cpp" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — `ProjectType::DotNet` and `ProjectType::Cpp` don't exist yet.

### Step 3: Add `DotNet` and `Cpp` to `ProjectType` enum

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
    Iac,
    DotNet,
    Cpp,
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
            Self::Iac => "IaC",
            Self::DotNet => ".NET",
            Self::Cpp => "C++",
            Self::Unknown => "Unknown",
        }
    }
}
```

### Step 4: Add detection to `detect_type()` — after IaC, before Unknown

```rust
    // .NET: *.csproj or *.sln at root
    if std::fs::read_dir(dir).ok().map_or(false, |entries| {
        entries.flatten().any(|e| {
            matches!(
                e.path().extension().and_then(|s| s.to_str()),
                Some("csproj" | "sln")
            )
        })
    }) {
        return ProjectType::DotNet;
    }
    // C++: CMakeLists.txt not already caught by Embedded/IaC
    if dir.join("CMakeLists.txt").exists() {
        return ProjectType::Cpp;
    }
    ProjectType::Unknown
```

### Step 5: Add `DotNet` and `Cpp` arms to `build()` and `test()` dispatch

```rust
// In build():
ProjectType::DotNet => build_dotnet(dir),
ProjectType::Cpp => build_cpp(dir),

// In test():
ProjectType::DotNet => test_dotnet(dir),
ProjectType::Cpp => test_cpp(dir),
```

Stub functions:
```rust
pub fn build_dotnet(dir: &Path) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: ".NET".into(), command: "dotnet build".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_dotnet(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: ".NET".into(), command: "dotnet test".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}

pub fn build_cpp(dir: &Path) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: "C++".into(), command: "cmake --build".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_cpp(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: "C++".into(), command: "ctest".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 6: Add `DotNet` and `Cpp` arms to `deps::check()`

```rust
ProjectType::DotNet => check_dotnet(dir),
ProjectType::Cpp => check_cpp(dir),
```

Stubs:
```rust
fn check_dotnet(dir: &Path) -> DepsReport { let _ = dir; DepsReport::empty(".NET") }
fn check_cpp(dir: &Path) -> DepsReport { let _ = dir; DepsReport::empty("C++") }
```

### Step 7: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 8: Run detection tests

```powershell
cargo test "detect_dotnet\|detect_cpp\|embedded_takes_priority_over_cpp\|iac_takes_priority" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: all pass.

### Step 9: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add ProjectType::DotNet and ProjectType::Cpp detection"
```

---

## Task 2 — .NET Build & Test Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for .NET output parsers

```rust
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
    let output = "Passed!  - Failed:     0, Passed:    12, Skipped:     2, Total:    14, Duration: 1 s";
    let (passed, failed, ignored) = parse_dotnet_test_output(output);
    assert_eq!(passed, 12);
    assert_eq!(failed, 0);
    assert_eq!(ignored, 2);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_dotnet_build\|parse_dotnet_test" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement parsers and replace stubs

```rust
fn parse_dotnet_build_output(output: &str) -> (bool, usize, usize) {
    let ok = output.contains("Build succeeded");
    let mut warnings = 0usize;
    let mut errors = 0usize;
    for line in output.lines() {
        let t = line.trim();
        if t.ends_with("Warning(s)") {
            warnings = t.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
        if t.ends_with("Error(s)") {
            errors = t.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
    }
    (ok, warnings, errors)
}

fn parse_dotnet_test_output(output: &str) -> (usize, usize, usize) {
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
    s.split(key).nth(1)?.split([',', ' ']).find(|p| !p.trim().is_empty())?.trim().parse().ok()
}

pub fn build_dotnet(dir: &Path) -> BuildResult {
    let cmd_str = "dotnet build";
    let start = Instant::now();
    let output = Command::new("dotnet").arg("build").current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result(".NET", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, warnings, errors) = parse_dotnet_build_output(&raw);
            BuildResult { ok, project_type: ".NET".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings, errors, diagnostics: vec![], raw_output: raw }
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
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (passed, failed, ignored) = parse_dotnet_test_output(&raw);
            TestResult {
                ok: o.status.success(), project_type: ".NET".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, passed, failed, ignored,
                failures: raw.lines().filter(|l| l.contains("Failed") && !l.contains("Failed:")).map(|l| l.to_string()).collect(),
                raw_output: raw,
            }
        }
    }
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_dotnet_build\|parse_dotnet_test" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement dotnet build and test wrappers"
```

---

## Task 3 — C++ Build & Test Functions

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for C++ output parsers

```rust
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
    let output = "100% tests passed, 0 tests failed out of 5\n\nTotal Test time (real) =   0.15 sec";
    let (passed, failed) = parse_ctest_output(output);
    assert_eq!(passed, 5);
    assert_eq!(failed, 0);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_cmake_build\|parse_ctest" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement parsers and replace stubs

```rust
fn parse_cmake_build_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Built target") || output.contains("[100%]");
    let errors = output
        .lines()
        .filter(|l| {
            (l.contains(": error:") || l.contains("error:"))
                && (l.contains(".cpp") || l.contains(".c") || l.contains(".cc"))
        })
        .count();
    (ok && errors == 0, errors)
}

fn parse_ctest_output(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.contains("tests passed") || line.contains("tests failed") {
            // "100% tests passed, 0 tests failed out of 5"
            let total = line.split("out of").nth(1).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0);
            let failed = extract_after(line, "failed out of")
                .or_else(|| {
                    line.split(',').find(|s| s.contains("failed"))
                        .and_then(|s| s.split_whitespace().find(|w| w.parse::<usize>().is_ok()))
                        .and_then(|n| n.parse().ok())
                })
                .unwrap_or(0);
            return (total.saturating_sub(failed), failed);
        }
    }
    (0, 0)
}

pub fn build_cpp(dir: &Path) -> BuildResult {
    // cmake configure + build
    let build_dir = dir.join("build");
    let _ = std::fs::create_dir_all(&build_dir);
    let _ = Command::new("cmake").args([".."])
        .current_dir(&build_dir).output();
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
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_cmake_build_output(&raw);
            BuildResult { ok, project_type: "C++".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}

pub fn test_cpp(dir: &Path) -> TestResult {
    let build_dir = dir.join("build");
    let cmd_str = "ctest --test-dir build";
    let start = Instant::now();
    let output = Command::new("ctest")
        .args(["--test-dir", build_dir.to_str().unwrap_or("build"), "--output-on-failure"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_test("C++", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (passed, failed) = parse_ctest_output(&raw);
            TestResult {
                ok: o.status.success(), project_type: "C++".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, passed, failed, ignored: 0,
                failures: raw.lines().filter(|l| l.trim_start().starts_with("FAILED")).map(|l| l.to_string()).collect(),
                raw_output: raw,
            }
        }
    }
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_cmake_build\|parse_ctest" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement C++ build (cmake) and test (ctest) wrappers"
```

---

## Task 4 — Desktop Dependency Check

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for .NET PackageReference parser

```rust
#[test]
fn parse_dotnet_csproj_packages() {
    let xml = r#"<Project Sdk="Microsoft.NET.Sdk">
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
    <PackageReference Include="Serilog" Version="3.1.1" />
  </ItemGroup>
</Project>"#;
    let deps = parse_csproj_packages(xml);
    assert_eq!(deps.len(), 2);
    assert!(deps.iter().any(|d| d.name == "Newtonsoft.Json" && d.current == "13.0.3"));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_dotnet_csproj" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `check_dotnet()` and `parse_csproj_packages()`

```rust
fn check_dotnet(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty(".NET");
    // Find first *.csproj in root
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let deps = parse_csproj_packages(&content);
                    report.outdated_count = deps.len();
                    report.outdated = deps;
                    report.has_lockfile = dir.join("packages.lock.json").exists();
                }
                break;
            }
        }
    }
    if Command::new("dotnet").arg("--version").output().is_err() {
        report.tool_missing.push("dotnet (.NET SDK not found; install from https://dotnet.microsoft.com)".into());
    }
    report
}

fn check_cpp(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("C++");
    // vcpkg.json (vcpkg manifest mode)
    if let Ok(content) = std::fs::read_to_string(dir.join("vcpkg.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(deps) = v["dependencies"].as_array() {
                report.outdated = deps.iter()
                    .filter_map(|d| d.as_str().or_else(|| d["name"].as_str()))
                    .map(|name| OutdatedDep { name: name.to_string(), current: "?".into(), latest: "?".into(), kind: "vcpkg".into() })
                    .collect();
                report.outdated_count = report.outdated.len();
                report.has_lockfile = dir.join("vcpkg.json").exists();
            }
        }
    }
    if Command::new("cmake").arg("--version").output().is_err() {
        report.tool_missing.push("cmake (install from https://cmake.org/download)".into());
    }
    report
}

fn parse_csproj_packages(xml: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    for line in xml.lines() {
        let t = line.trim();
        if t.starts_with("<PackageReference") {
            let name = extract_xml_attr(t, "Include").unwrap_or_default();
            let version = extract_xml_attr(t, "Version").unwrap_or_default();
            if !name.is_empty() {
                deps.push(OutdatedDep { name, current: version, latest: "?".into(), kind: "nuget".into() });
            }
        }
    }
    deps
}

fn extract_xml_attr(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_dotnet_csproj" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/deps.rs
git commit -m "feat: implement desktop dependency check for .NET (csproj) and C++ (vcpkg.json)"
```

---

## Task 5 — Desktop Version Read/Write

**Files:** `src/core/version.rs`

### Step 1: Add failing tests

```rust
#[test]
fn read_dotnet_version_from_csproj() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("MyApp.csproj"),
        "<Project>\n  <PropertyGroup>\n    <Version>3.1.4</Version>\n  </PropertyGroup>\n</Project>\n",
    ).unwrap();
    assert_eq!(read_dotnet_version(tmp.path()), Some("3.1.4".to_string()));
}

#[test]
fn read_cpp_version_from_cmakelists() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("CMakeLists.txt"), "project(MyLib VERSION 2.5.0 LANGUAGES CXX)\n").unwrap();
    assert_eq!(read_cpp_cmake_version(tmp.path()), Some("2.5.0".to_string()));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "read_dotnet_version\|read_cpp_cmake" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `read_dotnet_version()`, `write_dotnet_version()`, `read_cpp_cmake_version()`

```rust
pub(crate) fn read_dotnet_version(dir: &Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let t = line.trim();
                        if t.starts_with("<Version>") && t.ends_with("</Version>") {
                            let v = t.trim_start_matches("<Version>").trim_end_matches("</Version>");
                            if looks_like_semver(v) {
                                return Some(v.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn write_dotnet_version(dir: &Path, old_version: &str, new_version: &str) -> Result<(), String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("csproj") {
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let old_tag = format!("<Version>{}</Version>", old_version);
                let new_tag = format!("<Version>{}</Version>", new_version);
                if content.contains(&old_tag) {
                    std::fs::write(&path, content.replace(&old_tag, &new_tag)).map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
    }
    Err(format!("No *.csproj with <Version>{}</Version> found", old_version))
}

pub(crate) fn read_cpp_cmake_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("CMakeLists.txt")).ok()?;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("project(") && t.contains("VERSION") {
            let after = t.split("VERSION").nth(1)?.trim();
            let version = after.split([' ', ')', '\n']).next()?.trim();
            if looks_like_semver(version) {
                return Some(version.to_string());
            }
        }
    }
    None
}
```

### Step 4: Wire into `read_version()` — add after IaC, before Unknown fallback

```rust
if let Some(v) = read_dotnet_version(dir) {
    return Some((v, ".NET".into(), "*.csproj".into()));
}
if let Some(v) = read_cpp_cmake_version(dir) {
    return Some((v, "C++".into(), "CMakeLists.txt".into()));
}
```

### Step 5: Wire into `write_version()` — add `.NET` arm

```rust
".NET" => write_dotnet_version(dir, old_version, new_version),
```

### Step 6: Run tests + cargo check + commit

```powershell
cargo test "read_dotnet_version\|read_cpp_cmake" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/version.rs
git commit -m "feat: implement .NET and C++ version read/write"
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

- [ ] **Step 4: Smoke test — .NET detection** (if dotnet installed)

```powershell
# dotnet new console -n TestApp -o C:\Temp\TestApp
# .\target\release\raios.exe build C:\Temp\TestApp
# Expected: project_type: ".NET"
```

- [ ] **Step 5: Smoke test — C++ detection**

```powershell
# mkdir C:\Temp\cpp_test
# echo "cmake_minimum_required(VERSION 3.20)`nproject(TestApp)" > C:\Temp\cpp_test\CMakeLists.txt
# .\target\release\raios.exe build C:\Temp\cpp_test
# Expected: project_type: "C++"
```

- [ ] **Step 6: Regression check**

```powershell
.\target\release\raios.exe build . --json 2>&1 | Select-String "project_type"
```

Expected: `"project_type": "Rust"` — no regression.

- [ ] **Step 7: Commit and push**

```powershell
git add -A
git commit -m "chore: desktop support smoke test and final review"
git push origin master
```

---

## Self-Review Checklist

- [ ] `ProjectType::DotNet` added with `label()` → `".NET"`
- [ ] `ProjectType::Cpp` added with `label()` → `"C++"`
- [ ] .NET detected by `*.csproj` or `*.sln` at root
- [ ] C++ detected by `CMakeLists.txt` AFTER Embedded and IaC checks (priority ordering)
- [ ] `build_dotnet()` wraps `dotnet build`; `test_dotnet()` wraps `dotnet test`
- [ ] `parse_dotnet_test_output()` extracts Passed/Failed/Skipped from `dotnet test` summary line
- [ ] `build_cpp()` runs `cmake ..` + `cmake --build build`; `test_cpp()` runs `ctest`
- [ ] `check_dotnet()` parses `<PackageReference>` tags from `*.csproj`
- [ ] `check_cpp()` reads `vcpkg.json` if present
- [ ] `read_dotnet_version()` reads `<Version>` from `*.csproj`
- [ ] `write_dotnet_version()` updates `<Version>` in-place in `*.csproj`
- [ ] `read_cpp_cmake_version()` reads `project(...VERSION x.y.z...)` from `CMakeLists.txt`
- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] No regression on all prior project types
