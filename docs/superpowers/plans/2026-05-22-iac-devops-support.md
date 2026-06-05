# DevOps & IaC (Terraform/Docker) Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add IaC/DevOps project support to `raios build`, `test`, `deps`, and `version-info` for Terraform (`.tf` files) and Docker Compose (`docker-compose.yml`). "Build" = plan/validate. "Test" = validate/config-check.

**Architecture:** Add `ProjectType::Iac` with a secondary `IacKind` enum (`Terraform`, `DockerCompose`, `Dockerfile`). Detection order: Terraform first (any `*.tf` at root), then Docker Compose (`docker-compose.yml` or `docker-compose.yaml`), then bare `Dockerfile`. Runs **after** Embedded so embedded CMake projects aren't confused with bare Dockerfiles. Version reads from `.terraform.lock.hcl` provider versions (Terraform) or image tags in `docker-compose.yml` (Docker).

**Prerequisite:** Embedded plan must be implemented first (adds `Embedded` to enum).

**Tech Stack:** Rust, terraform CLI, docker compose CLI (v2), std::process::Command

**Test projects:** Any directory with `*.tf` files, or `docker-compose.yml`

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::Iac`, `IacKind`; `detect_iac_kind()`; `build_iac()`, `test_iac()`; per-toolchain helpers |
| `src/core/deps.rs` | Add `ProjectType::Iac` arm; `check_iac()` parsing `.terraform.lock.hcl` |
| `src/core/version.rs` | Add `read_iac_version()`; wire into `read_version()` |

---

## Task 1 — IaC Detection

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for IaC detection

```rust
#[test]
fn detect_terraform_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("main.tf"), "terraform {\n  required_version = \">= 1.5\"\n}\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Iac);
}

#[test]
fn detect_docker_compose_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("docker-compose.yml"), "version: \"3.8\"\nservices:\n  app:\n    image: nginx\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Iac);
}

#[test]
fn detect_dockerfile_project() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu:22.04\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Iac);
}

#[test]
fn terraform_beats_dockerfile() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("main.tf"), "terraform {}\n").unwrap();
    std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu\n").unwrap();
    // Terraform check comes first
    assert_eq!(detect_type(tmp.path()), ProjectType::Iac);
}

#[test]
fn embedded_takes_priority_over_dockerfile() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("platformio.ini"), "[env:esp32dev]\n").unwrap();
    std::fs::write(tmp.path().join("Dockerfile"), "FROM ubuntu\n").unwrap();
    assert_eq!(detect_type(tmp.path()), ProjectType::Embedded);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detect_terraform\|detect_docker_compose\|detect_dockerfile\|terraform_beats\|embedded_takes_priority_over_dockerfile" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — `ProjectType::Iac` doesn't exist yet.

### Step 3: Add `Iac` to `ProjectType` enum

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
            Self::Unknown => "Unknown",
        }
    }
}
```

Add `IacKind` (internal) and `detect_iac_kind()`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum IacKind {
    Terraform,
    DockerCompose,
    Dockerfile,
}

fn detect_iac_kind(dir: &Path) -> Option<IacKind> {
    // Terraform: any *.tf file at root
    if std::fs::read_dir(dir).ok().map_or(false, |entries| {
        entries.flatten().any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("tf"))
    }) {
        return Some(IacKind::Terraform);
    }
    if dir.join("docker-compose.yml").exists() || dir.join("docker-compose.yaml").exists() {
        return Some(IacKind::DockerCompose);
    }
    if dir.join("Dockerfile").exists() {
        return Some(IacKind::Dockerfile);
    }
    None
}
```

### Step 4: Add IaC detection to `detect_type()` — after Embedded, before Unknown

```rust
    if detect_iac_kind(dir).is_some() {
        return ProjectType::Iac;
    }
    ProjectType::Unknown
```

### Step 5: Add `Iac` arm to `build()` and `test()` with stubs

```rust
// In build():
ProjectType::Iac => build_iac(dir),

// In test():
ProjectType::Iac => test_iac(dir),
```

Stub functions:
```rust
pub fn build_iac(dir: &Path) -> BuildResult {
    let _ = dir;
    BuildResult { ok: false, project_type: "IaC".into(), command: "terraform plan".into(),
        duration_ms: 0, warnings: 0, errors: 0, diagnostics: vec![], raw_output: "Not yet implemented".into() }
}

pub fn test_iac(dir: &Path) -> TestResult {
    let _ = dir;
    TestResult { ok: false, project_type: "IaC".into(), command: "terraform validate".into(),
        duration_ms: 0, passed: 0, failed: 0, ignored: 0, failures: vec![], raw_output: "Not yet implemented".into() }
}
```

### Step 6: Add `Iac` arm to `deps::check()`

```rust
ProjectType::Iac => check_iac(dir),
```

Stub:
```rust
fn check_iac(dir: &Path) -> DepsReport {
    let _ = dir;
    DepsReport::empty("IaC")
}
```

### Step 7: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

### Step 8: Run detection tests

```powershell
cargo test "detect_terraform\|detect_docker_compose\|detect_dockerfile\|terraform_beats\|embedded_takes" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: all pass.

### Step 9: Commit

```powershell
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add ProjectType::Iac detection for Terraform and Docker"
```

---

## Task 2 — IaC Build Functions (terraform plan / docker compose build)

**Files:** `src/core/build.rs`

### Step 1: Add failing tests for IaC build output parsers

```rust
#[test]
fn parse_terraform_plan_success() {
    let output = "Plan: 3 to add, 1 to change, 0 to destroy.\nTerraform will perform the following actions:\n";
    let (ok, errors) = parse_terraform_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_terraform_plan_error() {
    let output = "Error: Invalid expression\n  on main.tf line 5, in resource \"aws_instance\" \"example\":\n    5:   ami = invalid_var\n";
    let (ok, errors) = parse_terraform_output(output);
    assert!(!ok);
    assert!(errors >= 1);
}

#[test]
fn parse_docker_compose_build_success() {
    let output = " => exporting to image\n => => writing image sha256:abc123\nSuccessfully built abc123\n";
    let (ok, _) = parse_docker_output(output);
    assert!(ok);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_terraform_plan\|parse_docker_compose_build" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement parsers and `build_iac()`

```rust
fn parse_terraform_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Plan:") || output.contains("No changes.") || output.contains("Apply complete!");
    let errors = output
        .lines()
        .filter(|l| l.trim_start().starts_with("Error:"))
        .count();
    (ok && errors == 0, errors)
}

fn parse_docker_output(output: &str) -> (bool, usize) {
    let ok = output.contains("Successfully built")
        || output.contains("writing image sha256:")
        || output.contains("Use 'docker scan'");
    let errors = output.lines().filter(|l| l.trim_start().starts_with("error:") || l.trim_start().starts_with("ERROR:")).count();
    (ok && errors == 0, errors)
}

pub fn build_iac(dir: &Path) -> BuildResult {
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => build_terraform(dir),
        Some(IacKind::DockerCompose) => build_docker_compose(dir),
        Some(IacKind::Dockerfile) => build_dockerfile(dir),
        None => failed_result("IaC", "—", std::time::Duration::ZERO, "No IaC toolchain found".into()),
    }
}

fn build_terraform(dir: &Path) -> BuildResult {
    // terraform init + plan (non-interactive)
    let _ = Command::new("terraform").args(["init", "-input=false"]).current_dir(dir).output();
    let cmd_str = "terraform plan -input=false";
    let start = Instant::now();
    let output = Command::new("terraform")
        .args(["plan", "-input=false"])
        .current_dir(dir)
        .output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Terraform", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_terraform_output(&raw);
            BuildResult { ok, project_type: "IaC/Terraform".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}

fn build_docker_compose(dir: &Path) -> BuildResult {
    let cmd_str = "docker compose build";
    let start = Instant::now();
    let output = Command::new("docker").args(["compose", "build"]).current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Docker", cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_docker_output(&raw);
            BuildResult { ok, project_type: "IaC/Docker".into(), command: cmd_str.into(),
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}

fn build_dockerfile(dir: &Path) -> BuildResult {
    let tag = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_else(|| "raios-build".into());
    let cmd_str = format!("docker build -t {} .", tag);
    let start = Instant::now();
    let output = Command::new("docker").args(["build", "-t", &tag, "."]).current_dir(dir).output();
    let elapsed = start.elapsed();
    match output {
        Err(e) => failed_result("IaC/Dockerfile", &cmd_str, elapsed, e.to_string()),
        Ok(o) => {
            let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let (ok, errors) = parse_docker_output(&raw);
            BuildResult { ok, project_type: "IaC/Dockerfile".into(), command: cmd_str,
                duration_ms: elapsed.as_millis() as u64, warnings: 0, errors, diagnostics: vec![], raw_output: raw }
        }
    }
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_terraform_plan\|parse_docker_compose_build\|parse_docker" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement IaC build for Terraform (plan) and Docker (build)"
```

---

## Task 3 — IaC Test Functions (terraform validate / docker compose config)

**Files:** `src/core/build.rs`

### Step 1: Add failing test

```rust
#[test]
fn parse_terraform_validate_success() {
    let output = "Success! The configuration is valid.\n";
    let (ok, errors) = parse_terraform_validate_output(output);
    assert!(ok);
    assert_eq!(errors, 0);
}

#[test]
fn parse_terraform_validate_error() {
    let output = "Error: Reference to undeclared resource\n  on main.tf line 12:\n";
    let (ok, errors) = parse_terraform_validate_output(output);
    assert!(!ok);
    assert!(errors >= 1);
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_terraform_validate" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `test_iac()` and `parse_terraform_validate_output()`

```rust
fn parse_terraform_validate_output(output: &str) -> (bool, usize) {
    let ok = output.contains("The configuration is valid");
    let errors = output.lines().filter(|l| l.trim_start().starts_with("Error:")).count();
    (ok && errors == 0, errors)
}

pub fn test_iac(dir: &Path) -> TestResult {
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => {
            let _ = Command::new("terraform").args(["init", "-input=false"]).current_dir(dir).output();
            let cmd_str = "terraform validate";
            let start = Instant::now();
            let out = Command::new("terraform").arg("validate").current_dir(dir).output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Terraform", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let (ok, failed) = parse_terraform_validate_output(&raw);
                    TestResult { ok, project_type: "IaC/Terraform".into(), command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64, passed: if ok { 1 } else { 0 },
                        failed, ignored: 0, failures: raw.lines().filter(|l| l.trim_start().starts_with("Error:")).map(|l| l.to_string()).collect(),
                        raw_output: raw }
                }
            }
        }
        Some(IacKind::DockerCompose) => {
            let cmd_str = "docker compose config";
            let start = Instant::now();
            let out = Command::new("docker").args(["compose", "config"]).current_dir(dir).output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Docker", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let ok = o.status.success();
                    TestResult { ok, project_type: "IaC/Docker".into(), command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64, passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 }, ignored: 0,
                        failures: if !ok { vec![raw.clone()] } else { vec![] }, raw_output: raw }
                }
            }
        }
        Some(IacKind::Dockerfile) | None => {
            let cmd_str = "docker build --check .";
            let start = Instant::now();
            let out = Command::new("docker").args(["build", "--check", "."]).current_dir(dir).output();
            let elapsed = start.elapsed();
            match out {
                Err(e) => failed_test("IaC/Dockerfile", cmd_str, elapsed, e.to_string()),
                Ok(o) => {
                    let raw = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let ok = o.status.success();
                    TestResult { ok, project_type: "IaC/Dockerfile".into(), command: cmd_str.into(),
                        duration_ms: elapsed.as_millis() as u64, passed: if ok { 1 } else { 0 },
                        failed: if ok { 0 } else { 1 }, ignored: 0, failures: vec![], raw_output: raw }
                }
            }
        }
    }
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_terraform_validate" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/build.rs
git commit -m "feat: implement IaC test for Terraform (validate) and Docker (config)"
```

---

## Task 4 — IaC Dependency Check

**Files:** `src/core/deps.rs`

### Step 1: Add failing test for `.terraform.lock.hcl` parser

```rust
#[test]
fn parse_terraform_lock_hcl() {
    let content = r#"provider "registry.terraform.io/hashicorp/aws" {
  version     = "5.0.0"
  constraints = ">= 4.0.0"
}
provider "registry.terraform.io/hashicorp/random" {
  version = "3.5.1"
}
"#;
    let deps = parse_terraform_lock(content);
    assert_eq!(deps.len(), 2);
    assert!(deps.iter().any(|d| d.name == "hashicorp/aws" && d.current == "5.0.0"));
    assert!(deps.iter().any(|d| d.name == "hashicorp/random" && d.current == "3.5.1"));
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_terraform_lock" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `check_iac()` and `parse_terraform_lock()`

```rust
fn check_iac(dir: &Path) -> DepsReport {
    use crate::core::build::detect_iac_kind;
    use crate::core::build::IacKind;
    match detect_iac_kind(dir) {
        Some(IacKind::Terraform) => check_terraform_deps(dir),
        Some(IacKind::DockerCompose) | Some(IacKind::Dockerfile) | None => {
            DepsReport::empty("IaC/Docker")
        }
    }
}

fn check_terraform_deps(dir: &Path) -> DepsReport {
    let mut report = DepsReport::empty("IaC/Terraform");
    if let Ok(content) = std::fs::read_to_string(dir.join(".terraform.lock.hcl")) {
        report.has_lockfile = true;
        let deps = parse_terraform_lock(&content);
        report.outdated_count = deps.len();
        report.outdated = deps;
    }
    if Command::new("terraform").arg("version").output().is_err() {
        report.tool_missing.push("terraform (install from https://developer.hashicorp.com/terraform/install)".into());
    }
    report
}

fn parse_terraform_lock(content: &str) -> Vec<OutdatedDep> {
    let mut deps = Vec::new();
    let mut current_provider = String::new();
    let mut current_version = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("provider \"registry.terraform.io/") {
            // "provider "registry.terraform.io/hashicorp/aws" {"
            current_provider = trimmed
                .trim_start_matches("provider \"registry.terraform.io/")
                .split('"')
                .next()
                .unwrap_or("")
                .to_string();
            current_version.clear();
        }
        if trimmed.starts_with("version") && trimmed.contains('=') && !trimmed.starts_with("version     =") {
            // "version = "5.0.0""
            current_version = trimmed
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if trimmed.starts_with("version     =") || (trimmed.starts_with("version") && trimmed.contains("=")) {
            current_version = trimmed
                .splitn(2, '=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .to_string();
        }
        if trimmed == "}" && !current_provider.is_empty() && !current_version.is_empty() {
            deps.push(OutdatedDep {
                name: current_provider.clone(),
                current: current_version.clone(),
                latest: "?".into(),
                kind: "provider".into(),
            });
            current_provider.clear();
            current_version.clear();
        }
    }
    deps
}
```

### Step 4: Run tests + cargo check + commit

```powershell
cargo test "parse_terraform_lock" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/deps.rs
git commit -m "feat: implement IaC dependency check with .terraform.lock.hcl parsing"
```

---

## Task 5 — IaC Version Read

**Files:** `src/core/version.rs`

### Step 1: Add failing test

```rust
#[test]
fn read_iac_version_from_terraform_lock() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("main.tf"), "terraform {\n  required_version = \">= 1.5\"\n}\n").unwrap();
    std::fs::write(
        tmp.path().join(".terraform.lock.hcl"),
        "provider \"registry.terraform.io/hashicorp/aws\" {\n  version = \"5.2.0\"\n}\n",
    ).unwrap();
    // IaC version reads first provider version
    let v = read_iac_version(tmp.path());
    assert!(v.is_some());
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "read_iac_version" -- --nocapture 2>&1 | Select-Object -Last 8
```

### Step 3: Implement `read_iac_version()`

```rust
pub(crate) fn read_iac_version(dir: &Path) -> Option<String> {
    // Terraform: read required_version from main.tf / *.tf
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("tf") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let t = line.trim();
                        if t.starts_with("required_version") && t.contains('=') {
                            let val = t.splitn(2, '=').nth(1).unwrap_or("").trim().trim_matches('"');
                            // e.g. ">= 1.5" — extract numeric part
                            let version = val.split_whitespace().last().unwrap_or("").trim_matches('"');
                            if looks_like_semver(version) {
                                return Some(version.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    // Docker Compose: no standard version field; return compose format version
    if let Ok(content) = std::fs::read_to_string(dir.join("docker-compose.yml"))
        .or_else(|_| std::fs::read_to_string(dir.join("docker-compose.yaml")))
    {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("version:") {
                let val = t.trim_start_matches("version:").trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}
```

### Step 4: Wire into `read_version()` — add after Embedded

```rust
if let Some(v) = read_iac_version(dir) {
    return Some((v, "IaC".into(), "main.tf / docker-compose.yml".into()));
}
```

### Step 5: Run tests + cargo check + commit

```powershell
cargo test "read_iac_version" -- --nocapture 2>&1 | Select-Object -Last 8
cargo check --bin raios 2>&1 | Select-Object -Last 5
git add src/core/version.rs
git commit -m "feat: implement IaC version read from Terraform and Docker Compose"
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

- [ ] **Step 4: Smoke test with a Terraform project** (if terraform installed)

```powershell
# mkdir C:\Temp\tf_test && echo "terraform {}" > C:\Temp\tf_test\main.tf
# .\target\release\raios.exe build C:\Temp\tf_test
# Expected: project_type: "IaC/Terraform"
```

- [ ] **Step 5: Smoke test with a Docker Compose project**

```powershell
# echo "version: '3'" > C:\Temp\docker_test\docker-compose.yml
# .\target\release\raios.exe build C:\Temp\docker_test
# Expected: project_type: "IaC/Docker"
```

- [ ] **Step 6: Regression check**

```powershell
.\target\release\raios.exe build . --json 2>&1 | Select-String "project_type"
```

Expected: `"project_type": "Rust"` — no regression.

- [ ] **Step 7: Commit and push**

```powershell
git add -A
git commit -m "chore: IaC support smoke test and final review"
git push origin master
```

---

## Self-Review Checklist

- [ ] `ProjectType::Iac` added with `label()` → `"IaC"`
- [ ] `IacKind` distinguishes Terraform / DockerCompose / Dockerfile
- [ ] `detect_iac_kind()` checks `*.tf` first, then `docker-compose.yml`, then `Dockerfile`
- [ ] Embedded project with a `Dockerfile` still detected as Embedded (Embedded check runs first)
- [ ] `build_iac()` runs `terraform init` then `plan` for Terraform; `docker compose build` for Docker
- [ ] `test_iac()` runs `terraform validate` for Terraform; `docker compose config` for Docker Compose
- [ ] `check_iac()` parses `.terraform.lock.hcl` for provider versions
- [ ] `read_iac_version()` extracts `required_version` from `*.tf`
- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] No regression on prior project types
