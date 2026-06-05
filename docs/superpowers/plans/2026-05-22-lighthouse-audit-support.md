# Lighthouse Web Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `raios audit <url>` command that runs Google Lighthouse and reports
Performance, Accessibility, Best Practices, SEO, and PWA scores. Supports a
`--threshold <N>` flag that exits non-zero if any score falls below `N`.

**Architecture:** New `src/core/audit.rs` module (distinct from `src/security/audit.rs`)
wraps `npx lighthouse <url> --output json --output-path stdout`. The JSON output is
parsed for `categories.*.score` values (0.0–1.0 floats, ×100 to get 0–100). A new
`src/cli/audit.rs` handler exposes the command and a `mod audit;` + `Commands::Audit`
variant is wired into `src/cli/mod.rs`. No `ProjectType` changes — this is a standalone
command, not a build/test system extension.

**Tech Stack:** Rust, Node.js / npx (Lighthouse runner), `serde_json` (JSON parse), `clap` (CLI)

**Test URL for smoke tests:** `https://example.com`

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/audit.rs` | **CREATE** — `AuditResult`, `run_lighthouse()`, `parse_lighthouse_json()` |
| `src/core/mod.rs` | Add `pub mod audit;` |
| `src/cli/audit.rs` | **CREATE** — `cmd_audit()` handler |
| `src/cli/mod.rs` | Add `mod audit;`, `Commands::Audit` variant, dispatch arm in `run()` |

---

## Task 1 — Core Types and Failing Test

**Files:** `src/core/audit.rs` (new), `src/core/mod.rs`

### Step 1: Write failing test for `run_lighthouse` and score parsing

Create `src/core/audit.rs` with the test module only (no implementation yet):

```rust
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub url: String,
    pub performance: u8,
    pub accessibility: u8,
    pub best_practices: u8,
    pub seo: u8,
    pub pwa: u8,
    pub duration_ms: u128,
    pub lighthouse_missing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lighthouse_json_extracts_all_scores() {
        let json = r#"{
            "categories": {
                "performance":     { "score": 0.95 },
                "accessibility":   { "score": 0.88 },
                "best-practices":  { "score": 1.00 },
                "seo":             { "score": 0.92 },
                "pwa":             { "score": 0.30 }
            }
        }"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance,    95);
        assert_eq!(result.accessibility,  88);
        assert_eq!(result.best_practices, 100);
        assert_eq!(result.seo,            92);
        assert_eq!(result.pwa,            30);
        assert!(!result.lighthouse_missing);
    }

    #[test]
    fn parse_lighthouse_json_handles_null_score() {
        let json = r#"{"categories": {"performance": {"score": null}}}"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance, 0);
    }

    #[test]
    fn parse_lighthouse_json_handles_empty_categories() {
        let json = r#"{"categories": {}}"#;
        let result = parse_lighthouse_json("https://example.com", json, 0);
        assert_eq!(result.performance,    0);
        assert_eq!(result.accessibility,  0);
        assert_eq!(result.best_practices, 0);
        assert_eq!(result.seo,            0);
        assert_eq!(result.pwa,            0);
    }
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "parse_lighthouse" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `parse_lighthouse_json` function does not exist yet.

### Step 3: Implement `parse_lighthouse_json()` in `src/core/audit.rs`

Add the parsing function and a stub `run_lighthouse()` above the `#[cfg(test)]` block:

```rust
fn score_to_u8(v: &serde_json::Value) -> u8 {
    v.as_f64().map(|f| (f * 100.0).round() as u8).unwrap_or(0)
}

pub fn parse_lighthouse_json(url: &str, json: &str, duration_ms: u128) -> AuditResult {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let cats = &v["categories"];
    AuditResult {
        url: url.to_string(),
        performance:    score_to_u8(&cats["performance"]["score"]),
        accessibility:  score_to_u8(&cats["accessibility"]["score"]),
        best_practices: score_to_u8(&cats["best-practices"]["score"]),
        seo:            score_to_u8(&cats["seo"]["score"]),
        pwa:            score_to_u8(&cats["pwa"]["score"]),
        duration_ms,
        lighthouse_missing: false,
    }
}

pub fn run_lighthouse(url: &str) -> AuditResult {
    let start = Instant::now();
    let out = Command::new("npx")
        .args([
            "--yes",
            "lighthouse",
            url,
            "--output", "json",
            "--output-path", "stdout",
            "--chrome-flags=--headless --no-sandbox",
            "--quiet",
        ])
        .output();

    let elapsed = start.elapsed().as_millis();

    match out {
        Err(_) => AuditResult {
            url: url.to_string(),
            performance: 0,
            accessibility: 0,
            best_practices: 0,
            seo: 0,
            pwa: 0,
            duration_ms: elapsed,
            lighthouse_missing: true,
        },
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let json_start = stdout.find('{').unwrap_or(0);
            let json_str = &stdout[json_start..];
            let mut result = parse_lighthouse_json(url, json_str, elapsed);
            result.duration_ms = elapsed;
            result
        }
    }
}
```

### Step 4: Add `pub mod audit;` to `src/core/mod.rs`

Add after the existing `pub mod` declarations:
```rust
pub mod audit;
```

### Step 5: Run tests to verify they pass

```powershell
cargo test "parse_lighthouse" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected output:
```
test core::audit::tests::parse_lighthouse_json_extracts_all_scores ... ok
test core::audit::tests::parse_lighthouse_json_handles_null_score ... ok
test core::audit::tests::parse_lighthouse_json_handles_empty_categories ... ok
test result: ok. 3 passed; 0 failed
```

### Step 6: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 7: Commit

```powershell
git add src/core/audit.rs src/core/mod.rs
git commit -m "feat: add AuditResult type and parse_lighthouse_json() to core/audit.rs"
```

---

## Task 2 — CLI Command `raios audit`

**Files:** `src/cli/audit.rs` (new), `src/cli/mod.rs`

### Step 1: Write failing test for CLI handler

Create `src/cli/audit.rs` with the test module only:

```rust
use crate::core::audit::AuditResult;

pub fn cmd_audit(url: &str, threshold: Option<u8>, json: bool) -> i32 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audit::parse_lighthouse_json;

    #[test]
    fn threshold_fails_when_score_below() {
        let result = AuditResult {
            url: "https://example.com".to_string(),
            performance: 75,
            accessibility: 90,
            best_practices: 88,
            seo: 95,
            pwa: 0,
            duration_ms: 0,
            lighthouse_missing: false,
        };
        assert!(below_threshold(&result, 80));
    }

    #[test]
    fn threshold_passes_when_all_above() {
        let result = AuditResult {
            url: "https://example.com".to_string(),
            performance: 90,
            accessibility: 95,
            best_practices: 92,
            seo: 100,
            pwa: 0,
            duration_ms: 0,
            lighthouse_missing: false,
        };
        assert!(!below_threshold(&result, 80));
    }

    #[test]
    fn threshold_ignores_pwa_score() {
        let result = AuditResult {
            url: "https://example.com".to_string(),
            performance: 92,
            accessibility: 95,
            best_practices: 90,
            seo: 98,
            pwa: 0,
            duration_ms: 0,
            lighthouse_missing: false,
        };
        assert!(!below_threshold(&result, 85));
    }
}
```

### Step 2: Run tests to verify they fail

```powershell
cargo test "threshold_" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected: FAIL — `below_threshold` function and `cmd_audit` are not yet implemented.

### Step 3: Implement `cmd_audit()` and `below_threshold()` in `src/cli/audit.rs`

Replace the file contents with the full implementation:

```rust
use crate::core::audit::{run_lighthouse, AuditResult};

pub(super) fn cmd_audit(url: &str, threshold: Option<u8>, json_out: bool) -> i32 {
    let result = run_lighthouse(url);

    if result.lighthouse_missing {
        eprintln!("error: lighthouse not found. Install via: npm install -g lighthouse");
        eprintln!("       Or ensure npx is available (comes with Node.js).");
        return 1;
    }

    if json_out {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    } else {
        print_audit_table(&result, threshold);
    }

    if let Some(t) = threshold {
        if below_threshold(&result, t) {
            eprintln!("\nFAIL: one or more scores below threshold of {}", t);
            return 1;
        }
    }
    0
}

fn below_threshold(r: &AuditResult, threshold: u8) -> bool {
    r.performance < threshold
        || r.accessibility < threshold
        || r.best_practices < threshold
        || r.seo < threshold
}

fn print_audit_table(r: &AuditResult, threshold: Option<u8>) {
    println!("\n  Lighthouse Audit — {}", r.url);
    println!("  {}", "─".repeat(46));
    print_score("Performance",    r.performance,    threshold);
    print_score("Accessibility",  r.accessibility,  threshold);
    print_score("Best Practices", r.best_practices, threshold);
    print_score("SEO",            r.seo,            threshold);
    print_score("PWA",            r.pwa,            None);
    println!("  {}", "─".repeat(46));
    println!("  Duration: {}ms\n", r.duration_ms);
}

fn print_score(label: &str, score: u8, threshold: Option<u8>) {
    let bar = score_bar(score);
    let flag = match threshold {
        Some(t) if score < t => " ✗",
        _ => "  ",
    };
    println!("  {:<16} {:>3}/100  {}  {}", label, score, bar, flag);
}

fn score_bar(score: u8) -> String {
    let filled = (score as usize) / 10;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audit::AuditResult;

    fn make_result(perf: u8, a11y: u8, bp: u8, seo: u8, pwa: u8) -> AuditResult {
        AuditResult {
            url: "https://example.com".to_string(),
            performance: perf,
            accessibility: a11y,
            best_practices: bp,
            seo,
            pwa,
            duration_ms: 0,
            lighthouse_missing: false,
        }
    }

    #[test]
    fn threshold_fails_when_score_below() {
        let result = make_result(75, 90, 88, 95, 0);
        assert!(below_threshold(&result, 80));
    }

    #[test]
    fn threshold_passes_when_all_above() {
        let result = make_result(90, 95, 92, 100, 0);
        assert!(!below_threshold(&result, 80));
    }

    #[test]
    fn threshold_ignores_pwa_score() {
        let result = make_result(92, 95, 90, 98, 0);
        assert!(!below_threshold(&result, 85));
    }
}
```

### Step 4: Wire `Commands::Audit` into `src/cli/mod.rs`

**4a.** Add `mod audit;` after the existing module declarations at the top of `src/cli/mod.rs`:
```rust
mod audit;
```

**4b.** Add the `Audit` variant to the `Commands` enum (after `Security`):
```rust
    /// Run Google Lighthouse web audit on a URL
    Audit {
        /// URL to audit (e.g. https://example.com)
        url: String,
        /// Fail if any score is below this threshold (0-100)
        #[arg(short, long)]
        threshold: Option<u8>,
    },
```

**4c.** Add the dispatch arm to `run()` (after the `Security` arm):
```rust
        Commands::Audit { url, threshold } => {
            let exit = audit::cmd_audit(&url, threshold, cli.json);
            std::process::exit(exit);
        }
```

### Step 5: Run tests to verify they pass

```powershell
cargo test "threshold_" -- --nocapture 2>&1 | Select-Object -Last 8
```

Expected output:
```
test cli::audit::tests::threshold_fails_when_score_below ... ok
test cli::audit::tests::threshold_passes_when_all_above ... ok
test cli::audit::tests::threshold_ignores_pwa_score ... ok
test result: ok. 3 passed; 0 failed
```

### Step 6: cargo check

```powershell
cargo check --bin raios 2>&1 | Select-Object -Last 5
```

Expected: `Finished` with 0 errors.

### Step 7: Commit

```powershell
git add src/cli/audit.rs src/cli/mod.rs
git commit -m "feat: implement 'raios audit <url>' CLI command with threshold support"
```

---

## Task 3 — Full Test Suite and Smoke Test

**Files:** (none changed — verification only)

### Step 1: Run the full test suite

```powershell
cargo test -- --nocapture 2>&1 | Select-Object -Last 15
```

Expected: All prior tests still pass; the 6 new audit tests pass. Zero failures.

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

### Step 4: Smoke test — help output

```powershell
.\target\release\raios.exe audit --help
```

Expected:
```
Run Google Lighthouse web audit on a URL

Usage: raios audit [OPTIONS] <URL>

Arguments:
  <URL>  URL to audit (e.g. https://example.com)

Options:
  -t, --threshold <THRESHOLD>  Fail if any score is below this threshold (0-100)
  ...
```

### Step 5: Smoke test — live audit (requires Node.js)

```powershell
.\target\release\raios.exe audit https://example.com
```

If Node.js is available:
Expected: Table with 5 score rows printed, duration shown, exit code 0.

If Node.js is not available:
Expected: `error: lighthouse not found. Install via: npm install -g lighthouse`, exit code 1.

### Step 6: Smoke test — threshold flag

```powershell
.\target\release\raios.exe audit https://example.com --threshold 90
echo "Exit code: $LASTEXITCODE"
```

Expected: If any score < 90, prints `FAIL:` line and exits with code 1.

### Step 7: Smoke test — JSON output

```powershell
.\target\release\raios.exe --json audit https://example.com | ConvertFrom-Json | Select-Object url, performance, accessibility, seo
```

Expected: JSON object with `url`, `performance`, `accessibility`, `best_practices`, `seo`, `pwa`, `duration_ms`, `lighthouse_missing` fields.

### Step 8: Commit and push

```powershell
git add -A
git commit -m "chore: verify lighthouse audit smoke tests pass"
git push origin master
```

---

## Implementation Notes

### Why `npx --yes lighthouse` and not `lighthouse` directly?

`npx --yes` installs Lighthouse on-demand if not globally installed. Users who have
Node.js but not `npm install -g lighthouse` still get a working audit. The `--yes`
flag bypasses the interactive prompt asking whether to install.

### Why `below_threshold` excludes PWA?

PWA scores are 0 for non-Progressive Web Apps by design. Treating a PWA score of 0
as a threshold failure would break the audit for every standard website. The threshold
only applies to Performance, Accessibility, Best Practices, and SEO.

### Why `json_start` offset in `run_lighthouse()`?

`npx` sometimes emits deprecation warnings or `npm notice` lines before the JSON
output. The `stdout.find('{')` offset skips any leading non-JSON lines to find the
start of the Lighthouse JSON payload.

### Lighthouse JSON schema

The relevant fields extracted from Lighthouse's JSON output:

```json
{
  "categories": {
    "performance":    { "score": 0.95 },
    "accessibility":  { "score": 0.88 },
    "best-practices": { "score": 1.00 },
    "seo":            { "score": 0.92 },
    "pwa":            { "score": 0.00 }
  }
}
```

All `score` values are floats in range `[0.0, 1.0]`. Multiply by 100 and round for
a 0–100 display value. A `null` score means Lighthouse could not compute the metric.
