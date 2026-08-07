# tmux Steer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give raios a live "steer" capability — inject a message into a currently-running, daemon-spawned agent session — by launching agents inside tmux instead of a bare subprocess, and adding `send-keys`-based delivery.

**Architecture:** `ExecutionProxy::spawn_agent` (`crates/raios-runtime/src/daemon/proxy.rs`) switches from `tokio::process::Command` to `tmux new-session -d`. Exit status is recovered via tmux's `remain-on-exit` + `pane_dead_status` (not a held `Child` handle). Output capture moves from piped stdout/stderr to `tmux pipe-pane` writing a logfile that a tokio task tails. A new `steer_agent` function validates the target session and shells `tmux send-keys`. Both a CLI command (`raios steer`) and an MCP tool (`steer_agent`) call it — the CLI path is ungated (matches `raios handoff`'s precedent that a human typing a command is the authorization), the MCP path is gated automatically by `McpServer`'s existing central policy dispatch.

**Tech Stack:** Rust, tokio (async process/fs), `which` crate (already a dependency of `raios-runtime` and `raios-core`), tmux (shelled out, already installed at `/usr/bin/tmux` in this environment), rusqlite (audit_log), clap (CLI).

## Global Constraints

- One new Cargo dependency: `ureq` (blocking HTTP client), added to `raios-runtime/Cargo.toml` only in Task 5 — required because neither CLI process (`raios steer`, synchronous `fn main`, no tokio runtime) nor the MCP server process (confirmed separate from the daemon — `McpServer` holds no `DaemonState`/`ExecutionProxy`) can reach `DaemonState.active_agents` in-process; both must call the daemon's existing Axum HTTP server (port from `raios-policy.toml`'s `server.hub.http_port`, default 42071) the same way `raios-factory-ui` already does per `crates/raios-runtime/src/server/http/`. `which = "6"` (used by Task 1) is already present in `raios-runtime/Cargo.toml` and `raios-core/Cargo.toml` — no new dependency there.
- Never build a shell string from untrusted input — every `Command::new("tmux")` call passes arguments as a `Vec<String>`/`.arg(...)` list, never `.arg(format!("... {user_input} ..."))` concatenated into one string. This mirrors the existing "no ambient authority" rule enforced by `agent_command()` in `proxy.rs`.
- Session naming is always `format!("raios-agent-{}", agent_process_id)` — one naming scheme, defined once, reused everywhere (doctor check excluded, which has no session to name).
- `raios handoff` and its DB tables (`cp_tasks`, `cp_approvals`) are never touched by this plan.
- Every new Rust function gets a test in the same commit that introduces it — no task ends with untested new code.

---

### Task 1: tmux presence check in `raios doctor`

**Files:**
- Modify: `crates/raios-runtime/src/system_scan/doctor.rs`
- Test: same file, `#[cfg(test)] mod tests` block at the bottom (existing convention — see `doctor_check_missing_binary_returns_offline_tier`)

**Interfaces:**
- Produces: `run_doctor_check` (existing function, `crates/raios-runtime/src/system_scan/doctor.rs:78`) now always appends a `tmux: ...` line to `DoctorResult.notes`, regardless of `agent`/`tier` — later tasks' integration test relies on `raios doctor <any-agent>` surfacing a missing-tmux problem instead of `spawn_agent` failing silently later.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/raios-runtime/src/system_scan/doctor.rs`:

```rust
#[test]
fn doctor_check_notes_tmux_presence() {
    let res = run_doctor_check("claude", None);
    assert!(
        res.notes.iter().any(|n| n.starts_with("tmux:")),
        "expected a tmux presence note, got: {:?}",
        res.notes
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime doctor_check_notes_tmux_presence`
Expected: FAIL — no note starts with `"tmux:"` yet.

- [ ] **Step 3: Write minimal implementation**

In `run_doctor_check` (`crates/raios-runtime/src/system_scan/doctor.rs:78`), right after the existing offline-tier binary check block (after the `if !binary_found && !config_dir_found { ... return ... }` early return, so this only runs once the requested agent's own binary check has passed — the tmux note is layered onto whatever tier that agent already reached, not a separate tier), add:

```rust
match which::which("tmux") {
    Ok(path) => notes.push(format!("tmux: found at {}", path.display())),
    Err(_) => notes.push(
        "tmux: NOT FOUND on PATH — required for `raios steer` and daemon-spawned \
         agent sessions; install it (e.g. `apt install tmux` / `brew install tmux`)"
            .to_string(),
    ),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime doctor_check_notes_tmux_presence`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/src/system_scan/doctor.rs
git commit -m "feat(doctor): surface tmux presence in raios doctor output"
```

---

### Task 2: Launch agents via tmux, recover exit status via `remain-on-exit`

**Files:**
- Modify: `crates/raios-runtime/src/daemon/proxy.rs`
- Test: same file, `#[cfg(test)] mod tests` block at the bottom (existing convention)

**Interfaces:**
- Consumes: `AgentProcess` struct (existing, `proxy.rs:36-42`), `DaemonState.active_agents` (existing, `crates/raios-runtime/src/daemon/state.rs`).
- Produces: a private helper `fn tmux_session_name(id: Uuid) -> String` returning `format!("raios-agent-{id}")` — later tasks (3, 4, 5, 6) import and reuse this instead of re-deriving the format string.
- Produces: `spawn_agent` (existing public signature unchanged: `pub async fn spawn_agent(&self, agent_name: &str, project_path: &str, timeout_secs: u64) -> Result<String>`) now launches via tmux internally. Its background task still writes `agent.status` into `DaemonState.active_agents` on completion, same field, same three string values (`"Completed Successfully"`, `"Exited with Error"`, `"Killed by Death Timer (Timeout)"`) as before — no consumer of `AgentProcess.status` needs to change.

This task does **not** yet touch log capture (`AgentProcess.logs` stays empty for tmux-launched agents until Task 3) — keeping this task reviewable on its own: process lifecycle correctness, independent of output plumbing.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/raios-runtime/src/daemon/proxy.rs` (below the existing `spawn_agent_rejects_unknown_identity_before_touching_state` test):

```rust
#[tokio::test]
async fn spawn_agent_via_tmux_reaches_completed_status() {
    use super::{DaemonState, ExecutionProxy};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // `agent_command("claude")` resolves to a real "claude" binary this test
    // environment doesn't control, so this test exercises the tmux/exit-status
    // plumbing directly by calling the new `spawn_via_tmux` helper (Step 3)
    // with an arbitrary harmless command instead of going through the
    // `agent_command()` allowlist — that allowlist itself is already covered
    // by `agent_command_resolves_all_canonical_identities` above.
    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    proxy
        .spawn_via_tmux(id, "true", &[], ".", 5)
        .await
        .expect("tmux spawn should succeed");

    // Poll up to 3s for the background task to observe pane exit and update state —
    // `true` exits immediately, well inside the 5s Death Timer.
    let mut status = String::new();
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            status = agent.status.clone();
            if status != "Running" {
                break;
            }
        }
    }
    assert_eq!(status, "Completed Successfully");
}

#[tokio::test]
async fn spawn_agent_via_tmux_death_timer_kills_long_running_session() {
    use super::{DaemonState, ExecutionProxy};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    proxy
        .spawn_via_tmux(id, "sleep", &["30".to_string()], ".", 1)
        .await
        .expect("tmux spawn should succeed");

    let mut status = String::new();
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            status = agent.status.clone();
            if status != "Running" {
                break;
            }
        }
    }
    assert_eq!(status, "Killed by Death Timer (Timeout)");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime spawn_agent_via_tmux --no-fail-fast`
Expected: FAIL to compile — `spawn_via_tmux` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

In `crates/raios-runtime/src/daemon/proxy.rs`, add the session-name helper near the top (after `agent_command`):

```rust
/// One naming scheme for tmux sessions, defined once. Every steer/spawn/kill
/// call derives the session name this way — never re-formatted ad hoc.
pub(crate) fn tmux_session_name(id: Uuid) -> String {
    format!("raios-agent-{id}")
}
```

Replace the body of the `tokio::spawn(async move { ... })` block inside `spawn_agent` (the block starting `use std::process::Stdio;` at `proxy.rs:130`) so that it calls a new method instead of building `Command` directly. First, extract the new method onto `impl ExecutionProxy` (add it right after `spawn_agent`, before the closing `}` of `impl ExecutionProxy`):

```rust
    /// Launches `program` (with `args`) inside a detached tmux session named
    /// after `id`, polls for the pane's exit via tmux's own `remain-on-exit`
    /// + `pane_dead_status` (tmux keeps the dead pane around instead of
    /// auto-closing the session, so we can read the real exit code — a bare
    /// `tmux has-session` poll can't distinguish success from failure), and
    /// writes the resulting status into `DaemonState.active_agents` exactly
    /// like the pre-tmux `Command::spawn` path did. `timeout_secs` is the
    /// Death Timer: exceeding it kills the session and records
    /// `"Killed by Death Timer (Timeout)"`, unchanged from before.
    ///
    /// Does not touch `AgentProcess.logs` — see Task 3 for output capture.
    pub(crate) async fn spawn_via_tmux(
        &self,
        id: Uuid,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_secs: u64,
    ) -> Result<()> {
        let session = tmux_session_name(id);

        let mut new_session = Command::new("tmux");
        new_session
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(&session)
            .arg("-c")
            .arg(cwd)
            .arg(program);
        for a in args {
            new_session.arg(a);
        }
        let status = new_session.status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "tmux new-session failed for session '{}' (exit: {:?})",
                session,
                status.code()
            ));
        }

        // Keep the pane around after the command exits so we can read its
        // exit status instead of the session silently vanishing.
        Command::new("tmux")
            .args(["set-option", "-t", &session, "remain-on-exit", "on"])
            .status()
            .await?;

        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        let session_for_task = session.clone();

        tokio::spawn(async move {
            let poll_interval = Duration::from_millis(500);
            let max_polls = ((timeout_secs * 1000) / poll_interval.as_millis() as u64).max(1);
            let mut final_status = "Killed by Death Timer (Timeout)";

            for _ in 0..max_polls {
                tokio::time::sleep(poll_interval).await;

                let dead = Command::new("tmux")
                    .args([
                        "display-message",
                        "-p",
                        "-t",
                        &session_for_task,
                        "#{pane_dead}",
                    ])
                    .output()
                    .await
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
                    .unwrap_or(false);

                if dead {
                    let exit_ok = Command::new("tmux")
                        .args([
                            "display-message",
                            "-p",
                            "-t",
                            &session_for_task,
                            "#{pane_dead_status}",
                        ])
                        .output()
                        .await
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
                        .unwrap_or(false);
                    final_status = if exit_ok {
                        "Completed Successfully"
                    } else {
                        "Exited with Error"
                    };
                    break;
                }
            }

            // Either the pane died naturally (final_status set above) or the
            // Death Timer ran out (final_status still its default) — either
            // way, tear the session down. Killing an already-dead session's
            // remnants is a no-op tmux tolerates.
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &session_for_task])
                .status()
                .await;

            let mut state_lock = state.write().await;
            if let Some(agent) = state_lock.active_agents.iter_mut().find(|a| a.id == id) {
                agent.status = final_status.to_string();
            }
            drop(state_lock);

            if let Some(tx) = &event_tx {
                let _ = tx.send(
                    serde_json::json!({
                        "event": "AgentStopped",
                        "agent_id": id.to_string(),
                        "final_status": final_status,
                    })
                    .to_string(),
                );
            }
        });

        Ok(())
    }
```

Now rewrite `spawn_agent`'s own inner `tokio::spawn` block (`proxy.rs:130-215` in the current file) to call this instead of building `Command` itself. Replace that entire inner `tokio::spawn(async move { ... });` block with:

```rust
        let session_name = tmux_session_name(process_id);
        if let Err(e) = self
            .spawn_via_tmux(process_id, program, &program_args, project_path, timeout_secs)
            .await
        {
            let mut state_lock = self.state.write().await;
            if let Some(agent) = state_lock
                .active_agents
                .iter_mut()
                .find(|a| a.id == process_id)
            {
                agent.status = "Failed to spawn process".to_string();
            }
            drop(state_lock);
            return Err(e);
        }
        println!(
            "[Proxy] Spawning agent '{}' (ID: {}) with {}s death timer via tmux session '{}'",
            agent_name, process_id, timeout_secs, session_name
        );
```

(This replaces the old `println!("[Proxy] Spawning agent...")` line too — keep only the one shown above, remove the old duplicate earlier in the function if the diff would otherwise produce two prints.)

Add `use std::time::Duration;` stays (already imported at the top of the file) — no new imports beyond what's already there (`tokio::process::Command`, `tokio::time::timeout` becomes unused by this task and can stay for now; Task 3 doesn't need it either — if `cargo clippy` flags it unused after this task, remove the `use tokio::time::timeout;` line at the top of the file in this same commit).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime spawn_agent_via_tmux --no-fail-fast`
Expected: PASS (both new tests). Also run `cargo test -p raios-runtime --lib daemon::proxy` to confirm the pre-existing `agent_command_*` and `spawn_agent_rejects_unknown_identity_before_touching_state` tests still pass unchanged.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/src/daemon/proxy.rs
git commit -m "feat(daemon): launch spawned agents in tmux, recover exit status via remain-on-exit"
```

---

### Task 3: Output capture via `tmux pipe-pane`

**Files:**
- Modify: `crates/raios-runtime/src/daemon/proxy.rs`
- Test: same file, `#[cfg(test)] mod tests` block

**Interfaces:**
- Consumes: `spawn_via_tmux` (Task 2), `tmux_session_name` (Task 2).
- Produces: `AgentProcess.logs` populated for tmux-launched agents again (parity restored with pre-Task-2 behavior, this time backed by a logfile instead of piped stdout).

- [ ] **Step 1: Write the failing test**

Add to `crates/raios-runtime/src/daemon/proxy.rs`'s test module:

```rust
#[tokio::test]
async fn spawn_agent_via_tmux_captures_output_into_logs() {
    use super::{DaemonState, ExecutionProxy};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    proxy
        .spawn_via_tmux(
            id,
            "sh",
            &["-c".to_string(), "echo hello-from-tmux".to_string()],
            ".",
            5,
        )
        .await
        .expect("tmux spawn should succeed");

    let mut found = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            if agent.logs.iter().any(|l| l.contains("hello-from-tmux")) {
                found = true;
                break;
            }
        }
    }
    assert!(found, "expected captured logs to contain the echoed line");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime spawn_agent_via_tmux_captures_output_into_logs`
Expected: FAIL — `agent.logs` stays empty (Task 2 didn't wire capture).

- [ ] **Step 3: Write minimal implementation**

In `spawn_via_tmux` (`proxy.rs`, added in Task 2), right after the `remain-on-exit` call and before spawning the polling `tokio::spawn`, add:

```rust
        let log_dir = std::env::temp_dir().join("raios-agent-logs");
        tokio::fs::create_dir_all(&log_dir).await.ok();
        let logfile = log_dir.join(format!("{session}.log"));

        Command::new("tmux")
            .args([
                "pipe-pane",
                "-o",
                "-t",
                &session,
                &format!("cat >> {}", logfile.display()),
            ])
            .status()
            .await?;
```

Then add a second background task, spawned alongside the existing polling task (add this right before the polling `tokio::spawn(async move { ... })` block, using its own cloned handles):

```rust
        let state_for_logs = self.state.clone();
        let logfile_for_task = logfile.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};

            // `cat >>` creates the file on open even before the first write,
            // but there's a short window right after the pipe-pane command
            // returns before that open happens — wait for it rather than
            // failing to open.
            for _ in 0..25 {
                if tokio::fs::metadata(&logfile_for_task).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let Ok(file) = tokio::fs::File::open(&logfile_for_task).await else {
                return;
            };
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF on a plain file doesn't mean "no more data
                        // ever" the way it would on a closed pipe — `cat`
                        // may still be appending. Poll instead of stopping.
                        // Give up once the session itself is gone.
                        let still_alive = Command::new("tmux")
                            .args(["has-session", "-t", &session])
                            .status()
                            .await
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if !still_alive {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }
                    Ok(_) => {
                        let mut s = state_for_logs.write().await;
                        if let Some(agent) =
                            s.active_agents.iter_mut().find(|a| a.id == id)
                        {
                            agent.logs.push(line.trim_end().to_string());
                        }
                    }
                    Err(_) => break,
                }
            }
        });
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime spawn_agent_via_tmux_captures_output_into_logs`
Expected: PASS. Also re-run the two Task 2 tests to confirm no regression:
`cargo test -p raios-runtime spawn_agent_via_tmux --no-fail-fast`

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/src/daemon/proxy.rs
git commit -m "feat(daemon): capture spawned-agent output via tmux pipe-pane"
```

---

### Task 4: `ExecutionProxy::steer_agent` + audit logging + policy rule

**Files:**
- Modify: `crates/raios-runtime/src/daemon/proxy.rs`
- Modify: `raios-policy.toml` (repo root)
- Test: `proxy.rs`'s test module

**Interfaces:**
- Consumes: `tmux_session_name` (Task 2), `DaemonState.active_agents` (existing), `raios_core::security::record_audit_event` (existing, `crates/raios-core/src/security/verify_chain.rs:10`, signature `pub fn record_audit_event(conn: &Connection, event_type: &str, actor: &str, data: &str) -> Result<()>`).
- Produces: `pub async fn steer_agent(&self, agent_id: Uuid, message: &str, sender: &str) -> Result<()>` on `ExecutionProxy` — Tasks 5 and 6 (CLI and MCP wiring) both call this directly, no other new public surface.

- [ ] **Step 1: Write the failing test**

Add to `crates/raios-runtime/src/daemon/proxy.rs`'s test module:

```rust
#[tokio::test]
async fn steer_agent_rejects_unknown_target() {
    use super::{DaemonState, ExecutionProxy};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state);
    let err = proxy
        .steer_agent(uuid::Uuid::new_v4(), "hello", "claude_kaira")
        .await
        .expect_err("steering an unknown agent id must fail");
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn steer_agent_sends_keys_into_live_session() {
    use super::{DaemonState, ExecutionProxy};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    // A session that reads one line and echoes it back with a marker prefix —
    // lets the test assert the steered message actually reached the pane.
    proxy
        .spawn_via_tmux(
            id,
            "sh",
            &[
                "-c".to_string(),
                "read line; echo \"STEERED:$line\"; sleep 5".to_string(),
            ],
            ".",
            10,
        )
        .await
        .expect("tmux spawn should succeed");

    // Give the session a moment to reach the `read` before steering it.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    proxy
        .steer_agent(id, "ping-from-test", "claude_kaira")
        .await
        .expect("steer should succeed against a live session");

    let mut found = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            if agent.logs.iter().any(|l| l.contains("STEERED:ping-from-test")) {
                found = true;
                break;
            }
        }
    }
    assert!(found, "expected the steered message to be echoed back through captured logs");
}
```

Note: `steer_agent_sends_keys_into_live_session` depends on Task 3's log capture being present — run this after Task 3 is committed, not before.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime steer_agent --no-fail-fast`
Expected: FAIL to compile — `steer_agent` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add to `impl ExecutionProxy` in `crates/raios-runtime/src/daemon/proxy.rs` (after `spawn_via_tmux`):

```rust
    /// Injects `message` into a currently-running, tmux-backed agent session
    /// as if the sender had typed it — `tmux send-keys`, the same mechanism
    /// omnigent-ai/omnigent uses for its live-verified `claude-native` steer
    /// path (see docs/superpowers/specs/2026-08-07-tmux-steer-design.md).
    /// Best-effort delivery: this does not know whether the target is mid
    /// turn or idle, and does not claim to. `sender` is recorded verbatim
    /// into the audit ledger's `actor` column for traceability — callers
    /// resolve it the same way existing call sites do (CLI:
    /// `RAIOS_AGENT_IDENTITY` env var; MCP: the tool call's caller context).
    pub async fn steer_agent(&self, agent_id: Uuid, message: &str, sender: &str) -> Result<()> {
        let target_name = {
            let state_lock = self.state.read().await;
            let agent = state_lock
                .active_agents
                .iter()
                .find(|a| a.id == agent_id && a.status == "Running")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "steer target not found: no running agent with id '{}'",
                        agent_id
                    )
                })?;
            agent.name.clone()
        };

        let session = tmux_session_name(agent_id);

        let alive = Command::new("tmux")
            .args(["has-session", "-t", &session])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if !alive {
            let mut state_lock = self.state.write().await;
            if let Some(agent) = state_lock
                .active_agents
                .iter_mut()
                .find(|a| a.id == agent_id)
            {
                agent.status = "Session Not Found (Steer Failed)".to_string();
            }
            return Err(anyhow::anyhow!(
                "steer target session '{}' not found — agent '{}' may have finished or crashed",
                session,
                target_name
            ));
        }

        Command::new("tmux")
            .args(["send-keys", "-t", &session, message, "Enter"])
            .status()
            .await?;

        self.push_event(serde_json::json!({
            "event": "AgentSteered",
            "agent_id": agent_id.to_string(),
            "sender": sender,
        }));

        if let Ok(conn) = raios_core::db::open_db() {
            let data = serde_json::json!({
                "target_agent_id": agent_id.to_string(),
                "target_agent_name": target_name,
                "message": message,
            })
            .to_string();
            let _ = raios_core::security::record_audit_event(&conn, "agent.steer", sender, &data);
        }

        Ok(())
    }
```

Add to `raios-policy.toml` (repo root), inside the existing `[tools]` block, alongside the other `[[tools.rules]]` entries:

```toml
[[tools.rules]]
name = "steer_agent"
action = "confirm"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime steer_agent --no-fail-fast`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/src/daemon/proxy.rs raios-policy.toml
git commit -m "feat(daemon): add steer_agent with tmux send-keys delivery and audit logging"
```

---

### Task 5: `POST /api/agents/steer` HTTP route + shared blocking client helper

**Why this task exists:** verified by grep that neither the CLI binary (`raios-surface-cli`, synchronous `fn main` in `crates/raios-surface-cli/src/bin/raios.rs:17`, no tokio runtime) nor the MCP server (`McpServer` in `crates/raios-surface-mcp/src/mcp/mod.rs` holds no `DaemonState`/`ExecutionProxy` field — it's a separate process) can reach `DaemonState.active_agents` in-process. `crates/raios-surface-tui/src/app/ipc.rs`'s `connect_daemon`/`connect_daemon_addr` are channel-based and built for the TUI's async event loop — not a fit for a one-shot blocking call. The daemon's existing Axum HTTP server (`crates/raios-runtime/src/server/http/`, routes registered in `mod.rs:73-89`) already exposes `active_agents` over `/api/health` and already has a POST-handler precedent to mirror: `handle_approve` (`routes.rs`, registered as `.route("/api/approve", post(handle_approve))` at `mod.rs:80`).

**Files:**
- Modify: `crates/raios-runtime/src/server/http/routes.rs` (new `handle_steer`)
- Modify: `crates/raios-runtime/src/server/http/mod.rs` (register the route)
- Create: `crates/raios-runtime/src/daemon_client.rs` (new shared blocking HTTP client module, used by Tasks 6 and 7)
- Modify: `crates/raios-runtime/src/lib.rs` (declare the new module — check this file for its exact `pub mod` list style before adding `pub mod daemon_client;`)
- Modify: `crates/raios-runtime/Cargo.toml` (add `ureq`)
- Test: `crates/raios-runtime/src/daemon_client.rs`'s own `#[cfg(test)]` block

**Interfaces:**
- Consumes: `ExecutionProxy::steer_agent` (Task 4), `AppState { daemon_state: Arc<RwLock<DaemonState>>, tx: broadcast::Sender<String> }` (existing, `crates/raios-runtime/src/server/http/mod.rs:27-30`), `PolicyConfig.server.hub.http_port` (existing, `crates/raios-core/src/security/policy.rs:38`, default `42071` per `raios-policy.toml:8`).
- Produces: `pub fn steer_agent_via_http(agent_id: &str, message: &str, sender: &str) -> anyhow::Result<()>` in `raios_runtime::daemon_client` — Tasks 6 and 7 both call exactly this function, no other new public surface for reaching the daemon.

- [ ] **Step 1: Write the failing test**

Add to `crates/raios-runtime/src/daemon_client.rs` (new file):

```rust
#[cfg(test)]
mod tests {
    use super::resolve_base_url;

    #[test]
    fn resolve_base_url_defaults_to_42071() {
        // No raios-policy.toml in the test's cwd → falls back to the
        // documented default port, same default the policy file itself
        // ships (raios-policy.toml:8, `http_port = 42071`).
        assert_eq!(resolve_base_url(), "http://127.0.0.1:42071");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime resolve_base_url_defaults_to_42071`
Expected: FAIL to compile — `crates/raios-runtime/src/daemon_client.rs` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add `ureq` to `crates/raios-runtime/Cargo.toml`'s `[dependencies]` section:

```toml
ureq = "2"
```

Create `crates/raios-runtime/src/daemon_client.rs`:

```rust
//! Blocking HTTP client for one-shot processes (the CLI binary, the MCP
//! server) that need to reach the running daemon's live `DaemonState` —
//! neither process holds it in-process. Used only for `raios steer` /
//! the `steer_agent` MCP tool today; not a general daemon-RPC framework.

use anyhow::{anyhow, Result};

/// Resolves the daemon's HTTP base URL from the same policy file the daemon
/// itself reads its bind port from (`raios-policy.toml`'s
/// `[server.hub] http_port`), falling back to the documented default.
pub(crate) fn resolve_base_url() -> String {
    let port = raios_core::security::PolicyConfig::try_load_default()
        .and_then(|p| p.server)
        .and_then(|s| s.hub)
        .and_then(|h| h.http_port)
        .unwrap_or(42071);
    format!("http://127.0.0.1:{port}")
}

/// Calls the daemon's `POST /api/agents/steer` route. Returns `Err` with the
/// daemon's own error message on any non-2xx response or transport failure —
/// callers (CLI, MCP) surface this string directly rather than wrapping it.
pub fn steer_agent_via_http(agent_id: &str, message: &str, sender: &str) -> Result<()> {
    let url = format!("{}/api/agents/steer", resolve_base_url());
    let body = serde_json::json!({
        "agent_id": agent_id,
        "message": message,
        "sender": sender,
    });

    let resp = ureq::post(&url)
        .send_json(body)
        .map_err(|e| anyhow!("could not reach raios daemon at {url}: {e}"))?;

    let parsed: serde_json::Value = resp
        .into_json()
        .map_err(|e| anyhow!("daemon returned an unparseable response: {e}"))?;

    match parsed.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(()),
        _ => Err(anyhow!(
            "steer failed: {}",
            parsed
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown daemon error")
        )),
    }
}
```

In `crates/raios-runtime/src/lib.rs`, add (matching whatever style the existing `pub mod` list there already uses — e.g. alphabetical order if that's the convention):

```rust
pub mod daemon_client;
```

Add to `crates/raios-runtime/src/server/http/routes.rs`, next to `ApprovePayload`/`handle_approve`:

```rust
#[derive(Deserialize)]
pub(super) struct SteerPayload {
    agent_id: String,
    message: String,
    sender: String,
}

pub(super) async fn handle_steer(
    State(state): State<AppState>,
    Json(payload): Json<SteerPayload>,
) -> impl IntoResponse {
    let id = match uuid::Uuid::parse_str(&payload.agent_id) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "status": "error",
                "message": format!("'{}' is not a valid agent id", payload.agent_id),
            }));
        }
    };

    let proxy = crate::daemon::proxy::ExecutionProxy::new(state.daemon_state.clone());
    match proxy.steer_agent(id, &payload.message, &payload.sender).await {
        Ok(_) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}
```

Add to `crates/raios-runtime/src/server/http/mod.rs`, right after the existing `.route("/api/approve", post(handle_approve))` line (`mod.rs:80`):

```rust
        .route("/api/agents/steer", post(handle_steer))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime resolve_base_url_defaults_to_42071`
Expected: PASS. Also run `cargo build -p raios-runtime` to confirm `handle_steer`/route registration compile (this exercises the `handle_steer` import path in `mod.rs` — fix any import-name mismatch now).

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/Cargo.toml crates/raios-runtime/src/daemon_client.rs crates/raios-runtime/src/lib.rs crates/raios-runtime/src/server/http/routes.rs crates/raios-runtime/src/server/http/mod.rs
git commit -m "feat(daemon): add POST /api/agents/steer route and shared HTTP client helper"
```

---

### Task 6: `raios steer` CLI command

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/args.rs` (new `Steer` variant on the `Commands` enum)
- Create: `crates/raios-surface-cli/src/cli/steer.rs`
- Modify: `crates/raios-surface-cli/src/cli/mod.rs` (register module + dispatch arm)

**Interfaces:**
- Consumes: `raios_runtime::daemon_client::steer_agent_via_http` (Task 5).
- Produces: `raios steer <agent-id> "<message>"` on the command line.

- [ ] **Step 1: Write the failing test**

Add to a new file `crates/raios-surface-cli/src/cli/steer.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn steer_requires_nonempty_message() {
        // cmd_steer exits the process on empty input via std::process::exit,
        // matching cmd_handoff's existing "fail loud, fail early" pattern —
        // so this is a plain data-shape test on the validation helper, not
        // a full process test.
        assert!(super::validate_message("").is_err());
        assert!(super::validate_message("hello").is_ok());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-surface-cli steer_requires_nonempty_message`
Expected: FAIL to compile — `crates/raios-surface-cli/src/cli/steer.rs` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Create `crates/raios-surface-cli/src/cli/steer.rs`:

```rust
/// Rejects an empty steer message before it ever reaches the daemon —
/// mirrors `cli/handoff.rs`'s pattern of failing loud and early on missing
/// required input.
pub(super) fn validate_message(msg: &str) -> Result<(), &'static str> {
    if msg.trim().is_empty() {
        Err("steer message must not be empty")
    } else {
        Ok(())
    }
}

pub(super) fn cmd_steer(agent_id: String, message: String, json: bool) {
    if let Err(e) = validate_message(&message) {
        eprintln!("Steer failed: {e}");
        std::process::exit(1);
    }

    let sender =
        std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());

    match raios_runtime::daemon_client::steer_agent_via_http(&agent_id, &message, &sender) {
        Ok(_) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"steer": "sent", "agent_id": agent_id}).to_string()
                );
            } else {
                println!("Steer sent to agent {agent_id}");
            }
        }
        Err(e) => {
            eprintln!("Steer failed: {e}");
            std::process::exit(1);
        }
    }
}
```

In `crates/raios-surface-cli/src/cli/args.rs`, add a new variant to the `Commands` enum, right after the existing `Handoff { ... }` variant (`args.rs:214-230`):

```rust
    /// Inject a message into a currently-running, daemon-spawned agent
    /// session (best-effort — does not know if the target is mid-turn).
    Steer {
        /// The agent process id (UUID) reported by `raios agents`/`raios sessions`.
        agent_id: String,
        /// The message to inject, as if typed into the agent's session.
        message: String,
    },
```

In `crates/raios-surface-cli/src/cli/mod.rs`:
1. Add `mod steer;` to the alphabetically-sorted `mod` list at the top (between `mod session;` and `mod swarm;`).
2. Add a dispatch arm, right after the existing `Commands::Handoff { ... } => { ... }` arm:

```rust
        Commands::Steer { agent_id, message } => steer::cmd_steer(agent_id, message, cli.json),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-surface-cli steer_requires_nonempty_message`
Expected: PASS. Also run `cargo build -p raios-surface-cli` to confirm the new CLI variant, dispatch arm, and `daemon_client` call compile.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-surface-cli/src/cli/steer.rs crates/raios-surface-cli/src/cli/args.rs crates/raios-surface-cli/src/cli/mod.rs
git commit -m "feat(cli): add raios steer command"
```

---

### Task 7: `steer_agent` MCP tool

**Files:**
- Modify: `crates/raios-surface-mcp/src/mcp/tools.rs` (dispatch arm, mirroring `"get_stats" => self.tool_get_stats()` at `tools.rs:160`; schema entry mirroring the `get_stats` one at `tools.rs:72`)
- Test: same file's existing `#[cfg(test)]` block if present, else a new one at the bottom of `tools.rs`

**Interfaces:**
- Consumes: `raios_runtime::daemon_client::steer_agent_via_http` (Task 5) — same call the CLI (Task 6) makes; the MCP server is confirmed a separate process from the daemon (no `DaemonState` field on `McpServer`), so it reaches the daemon the same way.
- Produces: MCP tool `steer_agent(agent_id: string, message: string)`, gated automatically by the existing `raios-policy.toml` `steer_agent` rule (Task 4) via `McpServer`'s existing central dispatch (`enforce_capability`/`record_tool_audit`, `tools.rs:1-45`) — no new policy-handling code in this task.

- [ ] **Step 1: Write the failing test**

Add to `crates/raios-surface-mcp/src/mcp/tools.rs` (create a `#[cfg(test)] mod tests` block at the bottom if one doesn't already exist there):

```rust
#[cfg(test)]
mod steer_tool_tests {
    #[test]
    fn steer_agent_requires_both_fields() {
        let missing_message = serde_json::json!({ "agent_id": "abc" });
        assert!(super::McpServer::extract_steer_args(&missing_message).is_err());

        let missing_agent = serde_json::json!({ "message": "hi" });
        assert!(super::McpServer::extract_steer_args(&missing_agent).is_err());

        let both = serde_json::json!({ "agent_id": "abc", "message": "hi" });
        assert!(super::McpServer::extract_steer_args(&both).is_ok());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-surface-mcp steer_agent_requires_both_fields`
Expected: FAIL to compile — `extract_steer_args` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add the schema entry to the tool list in `crates/raios-surface-mcp/src/mcp/tools.rs`, next to the `get_stats` one (`tools.rs:72`):

```rust
            { "name": "steer_agent", "description": "Inject a message into a currently-running, daemon-spawned agent session — best-effort delivery, does not know if the target is mid-turn.", "inputSchema": { "type": "object", "properties": { "agent_id": { "type": "string" }, "message": { "type": "string" } }, "required": ["agent_id", "message"] } },
```

Add the dispatch arm next to `"get_stats" => self.tool_get_stats()` (`tools.rs:160`):

```rust
            "steer_agent" => self.tool_steer_agent(args),
```

Add the handler and its arg-extraction helper (near wherever `tool_get_stats` is implemented in the same `impl McpServer` block):

```rust
    fn extract_steer_args(args: &Value) -> Result<(String, String), String> {
        let agent_id = args
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or("steer_agent requires a string 'agent_id'")?
            .to_string();
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or("steer_agent requires a string 'message'")?
            .to_string();
        Ok((agent_id, message))
    }

    fn tool_steer_agent(&self, args: &Value) -> Result<Value, String> {
        let (agent_id, message) = Self::extract_steer_args(args)?;

        // Sender identity for an MCP-triggered call comes from the same
        // place record_tool_audit (tools.rs:24) already reads it from —
        // reuse that, don't add a second identity source.
        let sender =
            std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());

        raios_runtime::daemon_client::steer_agent_via_http(&agent_id, &message, &sender)
            .map(|_| serde_json::json!({ "steer": "sent", "agent_id": agent_id }))
            .map_err(|e| e.to_string())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-surface-mcp steer_agent_requires_both_fields`
Expected: PASS. Also run `cargo build -p raios-surface-mcp` to confirm the dispatch arm and `daemon_client` call compile.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-surface-mcp/src/mcp/tools.rs
git commit -m "feat(mcp): add steer_agent tool, gated by existing policy dispatch"
```

---

### Task 8: End-to-end integration test + `cargo clippy`/`cargo fmt` pass

**Files:**
- Test: `crates/raios-runtime/tests/` (new file, e.g. `crates/raios-runtime/tests/steer_integration.rs` — check whether `raios-runtime` has an existing `tests/` directory with a similar daemon-level integration test to match its harness setup before creating a new one)

**Interfaces:**
- Consumes: `ExecutionProxy::spawn_agent`, `ExecutionProxy::steer_agent` (both from earlier tasks) — exercised together, end to end, once more, at the crate's public boundary rather than via `pub(crate)` internals, to catch anything the unit tests' direct internal access could have papered over.

- [ ] **Step 1: Check for an existing integration test harness to match**

Run: `ls crates/raios-runtime/tests/ 2>/dev/null && grep -rln "ExecutionProxy" crates/raios-runtime/tests/ 2>/dev/null`

If a directory and pattern already exist, follow its exact setup/teardown style. If not, proceed with Step 2 as a new file.

- [ ] **Step 2: Write the test**

Create `crates/raios-runtime/tests/steer_integration.rs` (adjust module path if Step 1 found an existing convention to match instead):

```rust
use raios_runtime::daemon::proxy::ExecutionProxy;
use raios_runtime::daemon::state::DaemonState;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn spawn_then_steer_full_roundtrip() {
    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    proxy
        .spawn_via_tmux(
            id,
            "sh",
            &[
                "-c".to_string(),
                "read line; echo \"GOT:$line\"; sleep 3".to_string(),
            ],
            ".",
            10,
        )
        .await
        .expect("spawn should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    proxy
        .steer_agent(id, "integration-test-message", "claude_kaira")
        .await
        .expect("steer should succeed");

    let mut ok = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            if agent
                .logs
                .iter()
                .any(|l| l.contains("GOT:integration-test-message"))
            {
                ok = true;
                break;
            }
        }
    }
    assert!(ok, "full spawn -> steer -> captured-output roundtrip failed");
}
```

Note: `spawn_via_tmux` is currently `pub(crate)` (Task 2). If this integration test lives in `crates/raios-runtime/tests/` (a separate compilation unit, outside the crate), change its visibility to `pub` in `proxy.rs` before this test can compile — do that as part of this step, and re-check whether that widening needs a doc comment update (it does: note in `spawn_via_tmux`'s doc comment that it's `pub` specifically so integration tests can exercise it directly, not for external callers to use instead of `spawn_agent`).

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p raios-runtime --test steer_integration`
Expected: PASS.

- [ ] **Step 4: Full workspace verification**

Run, in order, stopping to fix anything that fails before continuing:
1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --lib`
4. `raios security` (per this project's own mandatory pre-commit gate — Section "Git Standards" of AGENT_CONSTITUTION.md)
5. `sigmap` (regenerate `SIGMAP.md` per the same section)

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS-tmux-steer-spec
git add crates/raios-runtime/tests/steer_integration.rs crates/raios-runtime/src/daemon/proxy.rs SIGMAP.md
git commit -m "test(daemon): add end-to-end spawn+steer integration test"
```

---

## Explicitly Out of Scope (this plan)

Same as the spec: the interactive `raios run <agent>` path, busy/idle detection, `delivered`/`queued` distinction, policy 3-tier scoping, and bwrap/Landlock sandboxing — each is a separate future spec/plan cycle.
