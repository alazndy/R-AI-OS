# Security & Compliance Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing OWASP scanner in two dimensions:
1. **Secret patterns** — add high-specificity regex patterns for AWS, GitHub, Stripe, and Google credentials that the current generic `api_key` pattern misses.
2. **License scanning** — add a new `src/security/license.rs` module and `raios license` CLI command that reads dependency manifests (`Cargo.lock`, `package.json`, `package-lock.json`) and flags copyleft licenses (GPL, AGPL, LGPL).

**Architecture:**
- `src/security/patterns.rs` — extend the `PATTERNS` array with 4 new high-specificity entries.
- `src/security/license.rs` — **CREATE** — `LicenseDep`, `LicenseReport`, `scan_licenses()`.
- `src/security/mod.rs` — add `pub mod license;` and re-export `LicenseReport`.
- `src/cli/mod.rs` — add `Commands::License` variant and dispatch arm.
- `src/cli/security.rs` — add `cmd_license()` handler.

**Tech Stack:** Rust, `serde_json`, `regex_lite` (already a dependency), `std::fs`

**Test project for smoke tests:** Current R-AI-OS repo (Rust, `Cargo.lock` present)

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/security/patterns.rs` | Add 4 new `Pattern` entries to `PATTERNS` array |
| `src/security/license.rs` | **CREATE** — `LicenseDep`, `LicenseReport`, `scan_licenses()` |
| `src/security/mod.rs` | Add `pub mod license;`, re-export `LicenseReport` |
| `src/cli/security.rs` | Add `cmd_license()` function |
| `src/cli/mod.rs` | Add `Commands::License` variant and dispatch arm |

---

## Task 1 — New Secret Patterns (RED → GREEN)

**Files:** `src/security/patterns.rs`

### Step 1: Write failing tests for the new patterns

In `src/security/patterns.rs`, inside the existing `#[cfg(test)] mod tests_scan_file` block,
add the following tests:

```rust
    #[test]
    fn detects_aws_access_key() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, r#"AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE"#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("AWS")),
            "Should detect AWS access key ID"
        );
    }

    #[test]
    fn detects_github_pat() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, r#"GITHUB_TOKEN=ghp_16C7e42F292c6912E7710c838347Ae178B4a"#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("GitHub")),
            "Should detect GitHub PAT (ghp_ prefix)"
        );
    }

    #[test]
    fn detects_stripe_live_key() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".env").unwrap();
        writeln!(f, r#"STRIPE_SECRET=sk_live_51H2BLkJ3Ow1234567890abcde"#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("Stripe")),
            "Should detect Stripe live secret key"
        );
    }

    #[test]
    fn detects_google_api_key() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
        writeln!(f, r#"const key = "AIzaSyD-9tSrke72I6e0sEh8bT9SfGgfHIqnYjw";"#).unwrap();
        let issues = scan_file(f.path());
        assert!(
            issues.iter().any(|i| i.owasp == "A02" && i.title.contains("Google")),
            "Should detect Google API key (AIza prefix)"
        );
    }
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "detects_aws\|detects_github_pat\|detects_stripe\|detects_google_api" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected: FAIL — the new patterns do not exist in `PATTERNS` yet.

### Step 3: Add 4 new patterns to `PATTERNS` in `src/security/patterns.rs`

In `src/security/patterns.rs`, inside the `PATTERNS` array, add the following 4 entries
**before** the closing `];` and **after** the existing A02 patterns:

```rust
    // A02 — Specific credential formats (high-specificity, low false-positive rate)
    Pattern {
        owasp: "A02",
        title: "AWS Access Key ID (AKIA prefix)",
        severity: Severity::Critical,
        pattern: r"\bAKIA[0-9A-Z]{16}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "sh", "bash", "zsh",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "GitHub Personal Access Token (ghp_ prefix)",
        severity: Severity::Critical,
        pattern: r"\bghp_[a-zA-Z0-9]{36}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "sh", "bash", "zsh",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "Stripe secret key (sk_live_ or sk_test_)",
        severity: Severity::Critical,
        pattern: r"\bsk_(live|test)_[0-9a-zA-Z]{24,}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml", "json",
        ],
    },
    Pattern {
        owasp: "A02",
        title: "Google API key (AIza prefix)",
        severity: Severity::Critical,
        pattern: r"\bAIza[0-9A-Za-z\-_]{35}\b",
        exts: &[
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "env", "toml", "yaml", "yml",
            "json", "html",
        ],
    },
```

### Step 4: Run tests to verify they pass

```powershell
cargo test "detects_aws\|detects_github_pat\|detects_stripe\|detects_google_api" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected output:
```
test security::patterns::tests_scan_file::detects_aws_access_key ... ok
test security::patterns::tests_scan_file::detects_github_pat ... ok
test security::patterns::tests_scan_file::detects_stripe_live_key ... ok
test security::patterns::tests_scan_file::detects_google_api_key ... ok
test result: ok. 4 passed; 0 failed
```

### Step 5: Verify existing tests still pass

```powershell
cargo test "scan_file\|hardcoded_api_key\|clean_file\|debug_true\|eval_usage" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: All prior pattern tests still pass — no regressions.

### Step 6: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 7: Commit

```powershell
git add src/security/patterns.rs
git commit -m "feat: add AWS, GitHub PAT, Stripe, and Google API key secret patterns"
```

---

## Task 2 — License Scanner Module (RED → GREEN)

**Files:** `src/security/license.rs` (new), `src/security/mod.rs`

### Step 1: Write failing tests for `scan_licenses`

Create `src/security/license.rs` with the test module only:

```rust
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseDep {
    pub name: String,
    pub version: String,
    pub license: String,
    pub is_copyleft: bool,
    pub is_unknown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReport {
    pub project_path: PathBuf,
    pub deps: Vec<LicenseDep>,
    pub copyleft_count: usize,
    pub unknown_count: usize,
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_licenses_detects_gpl_in_cargo_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "gpl-crate"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"
"#;
        let cargo_toml = r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
license = "MIT"

[dependencies]
gpl-crate = "1.0.0"
"#;
        fs::write(tmp.path().join("Cargo.lock"), cargo_lock).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), cargo_toml).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find gpl-crate in Cargo.lock");
    }

    #[test]
    fn scan_licenses_detects_package_json_license() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = r#"{
            "name": "my-app",
            "version": "1.0.0",
            "license": "MIT",
            "dependencies": {
                "react": "18.0.0"
            }
        }"#;
        fs::write(tmp.path().join("package.json"), pkg).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find react dependency");
    }

    #[test]
    fn is_copyleft_identifies_gpl() {
        assert!(is_copyleft("GPL-3.0"));
        assert!(is_copyleft("GPL-2.0-only"));
        assert!(is_copyleft("AGPL-3.0"));
        assert!(is_copyleft("LGPL-2.1"));
        assert!(!is_copyleft("MIT"));
        assert!(!is_copyleft("Apache-2.0"));
        assert!(!is_copyleft("BSD-3-Clause"));
    }

    #[test]
    fn is_unknown_license_identifies_blanks() {
        assert!(is_unknown(""));
        assert!(is_unknown("UNKNOWN"));
        assert!(is_unknown("UNLICENSED"));
        assert!(!is_unknown("MIT"));
        assert!(!is_unknown("GPL-3.0"));
    }
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "scan_licenses\|is_copyleft\|is_unknown_license" -- --nocapture 2>&1 | Select-Object -Last 10
```

Expected: FAIL — `scan_licenses`, `is_copyleft`, and `is_unknown` do not exist yet.

### Step 3: Implement the license scanner in `src/security/license.rs`

Replace the file contents with the full implementation:

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseDep {
    pub name: String,
    pub version: String,
    pub license: String,
    pub is_copyleft: bool,
    pub is_unknown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReport {
    pub project_path: PathBuf,
    pub deps: Vec<LicenseDep>,
    pub copyleft_count: usize,
    pub unknown_count: usize,
    pub total: usize,
}

pub fn scan_licenses(path: &Path) -> LicenseReport {
    let mut deps = Vec::new();

    if path.join("Cargo.lock").exists() {
        deps.extend(scan_cargo_lock(path));
    } else if path.join("package.json").exists() {
        deps.extend(scan_package_json(path));
    }

    let copyleft_count = deps.iter().filter(|d| d.is_copyleft).count();
    let unknown_count = deps.iter().filter(|d| d.is_unknown).count();
    let total = deps.len();

    LicenseReport { project_path: path.to_path_buf(), deps, copyleft_count, unknown_count, total }
}

pub(crate) fn is_copyleft(license: &str) -> bool {
    let l = license.to_uppercase();
    l.contains("GPL") || l.contains("AGPL") || l.contains("LGPL")
}

pub(crate) fn is_unknown(license: &str) -> bool {
    let l = license.trim().to_uppercase();
    l.is_empty() || l == "UNKNOWN" || l == "UNLICENSED"
}

fn scan_cargo_lock(path: &Path) -> Vec<LicenseDep> {
    let content = match std::fs::read_to_string(path.join("Cargo.lock")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if !name.is_empty() {
                let license = lookup_cargo_license(path, &name, &version);
                deps.push(make_dep(name.clone(), version.clone(), license));
            }
            name = String::new();
            version = String::new();
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name = ") {
            name = rest.trim_matches('"').to_string();
        } else if let Some(rest) = trimmed.strip_prefix("version = ") {
            version = rest.trim_matches('"').to_string();
        }
    }
    if !name.is_empty() {
        let license = lookup_cargo_license(path, &name, &version);
        deps.push(make_dep(name, version, license));
    }
    deps
}

fn lookup_cargo_license(project_path: &Path, name: &str, version: &str) -> String {
    let registry_root = dirs::home_dir()
        .map(|h| h.join(".cargo").join("registry").join("src"))
        .unwrap_or_default();

    if let Ok(entries) = std::fs::read_dir(&registry_root) {
        for entry in entries.flatten() {
            let crate_dir = entry.path().join(format!("{}-{}", name, version));
            let manifest = crate_dir.join("Cargo.toml");
            if let Ok(content) = std::fs::read_to_string(&manifest) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("license") {
                        if let Some(eq_rest) = rest.strip_prefix(" = ") {
                            return eq_rest.trim().trim_matches('"').to_string();
                        }
                        if let Some(eq_rest) = rest.strip_prefix("=") {
                            return eq_rest.trim().trim_matches('"').to_string();
                        }
                    }
                }
            }
        }
    }

    if let Ok(content) = std::fs::read_to_string(project_path.join("Cargo.toml")) {
        if content.contains(&format!("name = \"{}\"", name)) {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("license = ") {
                    return rest.trim().trim_matches('"').to_string();
                }
            }
        }
    }

    String::from("UNKNOWN")
}

fn scan_package_json(path: &Path) -> Vec<LicenseDep> {
    let content = match std::fs::read_to_string(path.join("package.json")) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();

    if let Some(dependencies) = json["dependencies"].as_object() {
        for (name, version_val) in dependencies {
            let version = version_val.as_str().unwrap_or("*").to_string();
            let license = lookup_node_license(path, name).unwrap_or_else(|| "UNKNOWN".into());
            deps.push(make_dep(name.clone(), version, license));
        }
    }

    deps
}

fn lookup_node_license(path: &Path, name: &str) -> Option<String> {
    let pkg_json = path.join("node_modules").join(name).join("package.json");
    let content = std::fs::read_to_string(&pkg_json).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json["license"].as_str().map(|s| s.to_string())
}

fn make_dep(name: String, version: String, license: String) -> LicenseDep {
    let copyleft = is_copyleft(&license);
    let unknown = is_unknown(&license);
    LicenseDep { name, version, license, is_copyleft: copyleft, is_unknown: unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_licenses_detects_gpl_in_cargo_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "gpl-crate"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"
"#;
        let cargo_toml = r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
license = "MIT"

[dependencies]
gpl-crate = "1.0.0"
"#;
        fs::write(tmp.path().join("Cargo.lock"), cargo_lock).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), cargo_toml).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find gpl-crate in Cargo.lock");
    }

    #[test]
    fn scan_licenses_detects_package_json_license() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = r#"{
            "name": "my-app",
            "version": "1.0.0",
            "license": "MIT",
            "dependencies": {
                "react": "18.0.0"
            }
        }"#;
        fs::write(tmp.path().join("package.json"), pkg).unwrap();
        let report = scan_licenses(tmp.path());
        assert_eq!(report.total, 1, "Should find react dependency");
    }

    #[test]
    fn is_copyleft_identifies_gpl() {
        assert!(is_copyleft("GPL-3.0"));
        assert!(is_copyleft("GPL-2.0-only"));
        assert!(is_copyleft("AGPL-3.0"));
        assert!(is_copyleft("LGPL-2.1"));
        assert!(!is_copyleft("MIT"));
        assert!(!is_copyleft("Apache-2.0"));
        assert!(!is_copyleft("BSD-3-Clause"));
    }

    #[test]
    fn is_unknown_license_identifies_blanks() {
        assert!(is_unknown(""));
        assert!(is_unknown("UNKNOWN"));
        assert!(is_unknown("UNLICENSED"));
        assert!(!is_unknown("MIT"));
        assert!(!is_unknown("GPL-3.0"));
    }
}
```

### Step 4: Add `pub mod license;` to `src/security/mod.rs`

In `src/security/mod.rs`, add `pub mod license;` after the existing `pub mod audit;` line:

```rust
pub mod audit;
pub mod license;
pub mod patterns;
pub mod scanner;
```

Also add the re-export below the existing `pub use` declarations:

```rust
pub use license::{scan_licenses, LicenseDep, LicenseReport};
```

### Step 5: Run tests to verify they pass

```powershell
cargo test "scan_licenses\|is_copyleft\|is_unknown_license" -- --nocapture 2>&1 | Select-Object -Last 12
```

Expected output:
```
test security::license::tests::scan_licenses_detects_gpl_in_cargo_lock ... ok
test security::license::tests::scan_licenses_detects_package_json_license ... ok
test security::license::tests::is_copyleft_identifies_gpl ... ok
test security::license::tests::is_unknown_license_identifies_blanks ... ok
test result: ok. 4 passed; 0 failed
```

### Step 6: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 7: Commit

```powershell
git add src/security/license.rs src/security/mod.rs
git commit -m "feat: implement license compliance scanner in src/security/license.rs"
```

---

## Task 3 — CLI Command `raios license`

**Files:** `src/cli/security.rs`, `src/cli/mod.rs`

### Step 1: Add `cmd_license()` to `src/cli/security.rs`

Open `src/cli/security.rs` and add the following function. Wire the import at the top:

```rust
use crate::security::license::{scan_licenses, LicenseReport};
```

Then add the handler function:

```rust
pub(super) fn cmd_license(project: Option<String>, dev_ops: &Path, json_out: bool) {
    let path = crate::cli::resolve_project_path(project, dev_ops);
    let report = scan_licenses(&path);

    if json_out {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return;
    }

    print_license_report(&report);
}

fn print_license_report(report: &LicenseReport) {
    println!("\n  License Compliance — {}", report.project_path.display());
    println!("  {}", "─".repeat(60));
    println!("  Total dependencies: {}", report.total);

    if report.copyleft_count > 0 {
        println!("  Copyleft (GPL/AGPL/LGPL): {} ⚠", report.copyleft_count);
    } else {
        println!("  Copyleft (GPL/AGPL/LGPL): 0 ✓");
    }

    if report.unknown_count > 0 {
        println!("  Unknown license: {} ⚠", report.unknown_count);
    } else {
        println!("  Unknown license: 0 ✓");
    }

    if report.copyleft_count > 0 || report.unknown_count > 0 {
        println!("\n  Issues:");
        for dep in report.deps.iter().filter(|d| d.is_copyleft || d.is_unknown) {
            let tag = if d.is_copyleft { "COPYLEFT" } else { "UNKNOWN " };
            println!("    [{}] {} {} — {}", tag, dep.name, dep.version, dep.license);
        }
    }
    println!();
}
```

### Step 2: Add `Commands::License` to `src/cli/mod.rs`

**2a.** Add the `License` variant to the `Commands` enum (after `Security`):
```rust
    /// Scan dependency licenses for copyleft (GPL/AGPL/LGPL) and unknown licenses
    License {
        /// Project name or path (omit for current directory)
        project: Option<String>,
    },
```

**2b.** Add the dispatch arm to `run()` (after the `Security` arm):
```rust
        Commands::License { project } => security::cmd_license(project, &cfg.dev_ops_path, cli.json),
```

### Step 3: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 4: Commit

```powershell
git add src/cli/security.rs src/cli/mod.rs
git commit -m "feat: add 'raios license' command for dependency license compliance"
```

---

## Task 4 — Full Test Suite and Smoke Test

**Files:** (none changed — verification only)

### Step 1: Run the full test suite

```powershell
cargo test -- --nocapture 2>&1 | Select-Object -Last 15
```

Expected: All prior tests pass; all 8 new security tests pass. Zero failures.

### Step 2: cargo clippy

```powershell
cargo clippy -- -D warnings 2>&1 | Select-Object -Last 10
```

Expected: `Finished` — no warnings treated as errors.

### Step 3: Build release binary

```powershell
cargo build --release 2>&1 | Select-Object -Last 5
```

Expected: `Finished release` — binary at `target/release/raios.exe`.

### Step 4: Smoke test — secret pattern detection

Create a test file and run the scanner:

```powershell
"AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE" | Out-File -FilePath "$env:TEMP\test_secrets.env" -Encoding utf8
.\target\release\raios.exe security --target "$env:TEMP"
```

Expected: Report includes an issue for "AWS Access Key ID (AKIA prefix)" with CRITICAL severity.

### Step 5: Smoke test — license command on current project

```powershell
.\target\release\raios.exe license
```

Expected: Report listing the R-AI-OS project's Cargo.lock dependencies with their licenses. The `serde`, `clap`, `tokio` etc. dependencies should show MIT/Apache-2.0. Copyleft count should be 0 for a clean Rust project.

### Step 6: Smoke test — JSON output

```powershell
.\target\release\raios.exe --json license | ConvertFrom-Json | Select-Object total, copyleft_count, unknown_count
```

Expected: JSON object with `project_path`, `deps`, `copyleft_count`, `unknown_count`, `total` fields.

### Step 7: Smoke test — license help

```powershell
.\target\release\raios.exe license --help
```

Expected:
```
Scan dependency licenses for copyleft (GPL/AGPL/LGPL) and unknown licenses

Usage: raios license [PROJECT]

Arguments:
  [PROJECT]  Project name or path (omit for current directory)
```

### Step 8: Commit and push

```powershell
git add -A
git commit -m "chore: verify security compliance expansion smoke tests pass"
git push origin master
```

---

## Implementation Notes

### Pattern specificity: why not just add to the generic API key pattern?

The existing generic pattern (`api_key|api_secret|...\s*[=:]\s*['"][a-zA-Z0-9_-]{16,}['"]`)
catches variable-name-prefixed secrets. The new 4 patterns catch **value-format** secrets — they
match regardless of variable name. An AWS key hardcoded as `const FOO = "AKIAIOSFODNN7EXAMPLE"`
would not match the generic pattern because the variable name `FOO` is not in the keyword list.
The new patterns specifically match the credential's structural format.

### License scanner: why read `~/.cargo/registry` instead of running `cargo metadata`?

`cargo metadata` is authoritative but slow — it involves cargo resolving the full dependency
graph. For the offline/fast scanning use case, reading the registry cache directly is faster
and works without network access. The fallback to `UNKNOWN` is safe because copyleft detection
only flags known-copyleft identifiers.

### Copyleft definition

The scanner flags any license containing "GPL", "AGPL", or "LGPL" (case-insensitive). This covers:
- `GPL-2.0`, `GPL-2.0-only`, `GPL-2.0-or-later`
- `GPL-3.0`, `GPL-3.0-only`, `GPL-3.0-or-later`
- `LGPL-2.0`, `LGPL-2.1`, `LGPL-3.0`
- `AGPL-3.0`, `AGPL-3.0-only`

MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, MPL-2.0, and EUPL-1.2 are **not** flagged.

### `dirs` crate dependency

The license scanner uses `dirs::home_dir()` to locate `~/.cargo/registry`. The `dirs`
crate is already a dependency of R-AI-OS (used in `src/cli/mod.rs` for `dirs::desktop_dir()`),
so no new `Cargo.toml` change is needed.
