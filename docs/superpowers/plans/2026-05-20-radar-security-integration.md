# Radar → Security & Compliance Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `security::scan_project()` and `compliance::check_file()` findings flow through the Radar whisper stream so connected agents receive `RadarWhisper` events for security vulnerabilities and compliance violations in real time.

**Architecture:** Two daemon workers gain awareness of the Radar channel. `health.rs` already runs `check_project()` (which internally calls `security::scan_project()`); after each scan it converts CRITICAL/HIGH `SecurityIssue`s to `Whisper::security_vuln()` calls. `validation.rs` already fires on `FileChanged` events and calls `validate_file()`; compliance violations are now also emitted as `Whisper::arch_violation()` calls. No changes to `radar.rs`, `security.rs`, or `compliance.rs`.

**Tech Stack:** `crate::radar::{RadarChannel, Whisper}`, `crate::health::ProjectHealth`, existing daemon workers.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Modify | `src/daemon/health.rs` | Emit Radar whispers after `check_project()` |
| Modify | `src/daemon/validation.rs` | Emit Radar whispers after `validate_file()` |
| No change | `src/radar.rs` | Already has all whisper types needed |
| No change | `src/security.rs` | Unchanged |
| No change | `src/compliance.rs` | Unchanged |

---

### Task 1: Emit security whispers from the health worker

**Files:**
- Modify: `src/daemon/health.rs`

Key facts:
- `check_project(proj)` returns `crate::health::ProjectHealth`
- `ProjectHealth` has: `security_critical: usize`, `security_score: Option<u8>`, `name: String`, `path: PathBuf`
- `crate::security::scan_project(&proj.local_path)` returns `SecurityReport` with `issues: Vec<SecurityIssue>`
- `SecurityIssue` has: `owasp: &str`, `title: &str`, `severity: Severity`, `file: Option<PathBuf>`, `line: Option<usize>`
- `crate::radar::RadarChannel::new(tx)` creates a channel
- `Whisper::security_vuln(project, file, message, cve)` builds a whisper

- [ ] **Step 1: Write failing test for whisper emission**

Add at the bottom of `src/daemon/health.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::RadarChannel;

    #[test]
    fn security_issues_produce_whispers() {
        use crate::security::{SecurityIssue, SecurityReport, Severity as SecSev, ProjectType};

        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);

        let report = SecurityReport {
            score: 40,
            grade: "F",
            issues: vec![
                SecurityIssue {
                    owasp: "A02",
                    title: "Hardcoded password",
                    severity: SecSev::Critical,
                    file: Some(std::path::PathBuf::from("src/config.rs")),
                    line: Some(12),
                    snippet: None,
                },
            ],
            audit_output: None,
            project_type: ProjectType::Rust,
            checks_run: 1,
        };

        emit_security_whispers("my-proj", &report, &radar);

        let msg = rx.try_recv().unwrap();
        let val: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(val["event"], "RadarWhisper");
        assert_eq!(val["kind"], "security_vuln");
        assert_eq!(val["project"], "my-proj");
        assert_eq!(val["severity"], "critical");
    }
}
```

- [ ] **Step 2: Run test — confirm compile error**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test daemon::health::tests::security_issues_produce_whispers 2>&1 | Select-Object -Last 8
```

Expected: compile error (`emit_security_whispers` not defined)

- [ ] **Step 3: Add imports and `emit_security_whispers` to `src/daemon/health.rs`**

Add at the top of the file:

```rust
use crate::radar::{RadarChannel, Whisper};
use crate::security::SecurityReport;
```

Add this function BEFORE the `start_health_worker` function:

```rust
pub(crate) fn emit_security_whispers(
    project_name: &str,
    report: &SecurityReport,
    radar: &RadarChannel,
) {
    use crate::security::Severity as SecSev;

    let whispers: Vec<Whisper> = report
        .issues
        .iter()
        .filter(|i| matches!(i.severity, SecSev::Critical | SecSev::High))
        .map(|issue| {
            let msg = format!("[{}] {} (OWASP {})", issue.severity.label(), issue.title, issue.owasp);
            Whisper::security_vuln(project_name, issue.file.clone(), &msg, None)
        })
        .collect();

    radar.emit_many(whispers);
}
```

- [ ] **Step 4: Call `emit_security_whispers` inside the health worker loop**

In `start_health_worker`, find the section that runs `check_project` and collects reports:

```rust
let mut handles = vec![];
for proj in projects.clone() {
    let tx_log = tx.clone();
    handles.push(tokio::task::spawn_blocking(move || {
        let report = check_project(&proj);
        ...
        report
    }));
}
```

Update the spawn_blocking closure to also run security scan and emit whispers:

```rust
let mut handles = vec![];
let radar = RadarChannel::new(tx.clone());
for proj in projects.clone() {
    let tx_log = tx.clone();
    let radar_clone = radar.clone();
    handles.push(tokio::task::spawn_blocking(move || {
        let report = check_project(&proj);

        // Emit Radar whispers for CRITICAL/HIGH security issues
        let sec_report = crate::security::scan_project(&proj.local_path);
        emit_security_whispers(&proj.name, &sec_report, &radar_clone);

        let log_msg = serde_json::json!({
            "event": "NewLog",
            "log": {
                "timestamp": chrono::Local::now().format("%H:%M:%S").to_string(),
                "sender": "HealthWorker",
                "content": format!("Checked: {}", proj.name)
            }
        });
        let _ = tx_log.send(log_msg.to_string());
        report
    }));
}
```

- [ ] **Step 5: Run test**

```powershell
cargo test daemon::health::tests::security_issues_produce_whispers
```

Expected: PASS

- [ ] **Step 6: Build check**

```powershell
cargo check 2>&1 | Select-Object -Last 5
```

Expected: no errors

- [ ] **Step 7: Commit**

```powershell
git add src/daemon/health.rs
git commit -m "feat: emit Radar whispers for CRITICAL/HIGH security issues in health worker"
```

---

### Task 2: Emit compliance whispers from the validation worker

**Files:**
- Modify: `src/daemon/validation.rs`

Key facts:
- `validate_file(path, proj)` returns `Vec<ValidationError>` (from `src/health.rs:168`)
- `ValidationError` has fields — check with `grep` first (step 1 below)
- `Whisper::arch_violation(project, file, message, hint)` builds an architectural violation whisper

- [ ] **Step 1: Write failing test**

`ValidationError` is defined in `src/daemon/state.rs` and has fields: `file: String`, `message: String`, `line: Option<usize>`, `source: String`.

Add at the bottom of `src/daemon/validation.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar::RadarChannel;
    use crate::daemon::state::ValidationError;

    #[test]
    fn compliance_violations_produce_arch_whispers() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(32);
        let radar = RadarChannel::new(tx);

        // ValidationError is from crate::daemon::state — fields: file: String, message: String, line: Option<usize>, source: String
        let errors = vec![
            ValidationError {
                file: "src/app.ts".to_string(),
                message: "console.log found in production code".to_string(),
                line: Some(42),
                source: "compliance".to_string(),
            }
        ];

        emit_compliance_whispers("my-proj", &errors, &radar);

        let msg = rx.try_recv().unwrap();
        let val: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(val["event"], "RadarWhisper");
        assert_eq!(val["kind"], "arch_violation");
    }
}
```

- [ ] **Step 2: Run test — confirm compile error**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test daemon::validation::tests::compliance_violations_produce_arch_whispers 2>&1 | Select-Object -Last 8
```

Expected: compile error (`emit_compliance_whispers` not defined)

- [ ] **Step 3: Add imports and `emit_compliance_whispers` to `src/daemon/validation.rs`**

Add imports:
```rust
use crate::daemon::state::ValidationError;
use crate::radar::{RadarChannel, Whisper};
```

Add function before `start_validation_worker`:
```rust
pub(crate) fn emit_compliance_whispers(
    project_name: &str,
    errors: &[ValidationError],
    radar: &RadarChannel,
) {
    let whispers: Vec<Whisper> = errors
        .iter()
        .map(|e| {
            Whisper::arch_violation(
                project_name,
                std::path::PathBuf::from(&e.file),
                &e.message,
                Some("fix compliance violation before merging"),
            )
        })
        .collect();
    radar.emit_many(whispers);
}

- [ ] **Step 4: Call `emit_compliance_whispers` inside the validation worker**

In `start_validation_worker`, find where `validate_file` results are used:

```rust
let errors = tokio::task::spawn_blocking(move || {
    validate_file(&path_clone, &proj_clone)
})
.await
.unwrap_or_default();
```

After this block, add:
```rust
let radar = RadarChannel::new(tx_broadcast.clone());
emit_compliance_whispers(&proj.name, &errors, &radar);
```

- [ ] **Step 5: Run the test**

```powershell
cargo test daemon::validation::tests::compliance_violations_produce_arch_whispers
```

Expected: PASS

- [ ] **Step 6: Run full suite**

```powershell
cargo test 2>&1 | Select-Object -Last 8
```

Expected: no new failures

- [ ] **Step 7: Commit**

```powershell
git add src/daemon/validation.rs
git commit -m "feat: emit Radar whispers for compliance violations in validation worker"
```
