# Factory + Proxy-Store Daemon Wire Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the existing `Factory` and `CapabilityProxy` modules as TCP commands in the daemon, and fix the `AgentInfo` command so it actually updates the session's `agent` field in SQLite.

**Architecture:** `Server` gains two new fields: `factory: Arc<Factory>` and `proxy: Arc<CapabilityProxy>`. Five new TCP command handlers are added to the dispatch loop in `run_inner()`. Since agents can't send arbitrary Rust futures over TCP, `SubmitJob` accepts a `shell_cmd` string that is executed as a blocking subprocess — output becomes the job result. `ExecuteCapability` runs synchronously within the request/response cycle.

**Tech Stack:** `rusqlite`, `tokio::process::Command`, existing `crate::factory::Factory`, `crate::proxy_store::{CapabilityProxy, CapabilityStore}`

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Modify | `src/daemon/server.rs` | Add `factory` + `proxy` fields, 5 new TCP command handlers, AgentInfo DB fix |
| No change | `src/factory.rs` | Already complete |
| No change | `src/proxy_store.rs` | Already complete |

---

### Task 1: Add `Factory` and `CapabilityProxy` to `Server`

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Read `src/daemon/server.rs`** to understand current struct layout before touching anything.

- [ ] **Step 2: Add imports at the top of `src/daemon/server.rs`**

```rust
use crate::factory::{Factory, Job};
use crate::proxy_store::{CapabilityProxy, CapabilityStore};
```

- [ ] **Step 3: Add fields to `Server` struct**

Find:
```rust
pub struct Server {
    state: Arc<RwLock<DaemonState>>,
    execution_proxy: super::proxy::ExecutionProxy,
    sessions: Arc<SessionStore>,
}
```

Replace with:
```rust
pub struct Server {
    state: Arc<RwLock<DaemonState>>,
    execution_proxy: super::proxy::ExecutionProxy,
    sessions: Arc<SessionStore>,
    factory: Arc<Factory>,
    proxy: Arc<CapabilityProxy>,
}
```

- [ ] **Step 4: Update `new()` to construct Factory and CapabilityProxy**

Find the `new()` method. Replace its body with:

```rust
pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
    let execution_proxy = super::proxy::ExecutionProxy::new(state.clone());
    let sessions = Arc::new(SessionStore::new(SessionStore::default_path()));

    // Factory shares the same broadcast channel — will be wired in run_inner()
    // Use a placeholder sender here; run_inner() calls Factory::new(tx)
    let (tmp_tx, _) = tokio::sync::broadcast::channel::<String>(1);
    let factory = Arc::new(Factory::new(tmp_tx));

    let proxy = Arc::new(CapabilityProxy::new(CapabilityStore::new()));

    Self { state, execution_proxy, sessions, factory, proxy }
}
```

- [ ] **Step 5: Fix — wire Factory with the real broadcast channel in `run_inner()`**

In `run_inner()`, after `let (tx, _) = broadcast::channel...` (or after the existing tx setup), add:

```rust
// Re-create factory with the real tx so job completions reach all clients
let factory = Arc::new(Factory::new(tx.clone()));
```

Then replace `self.factory.clone()` usage in spawned tasks with this new `factory` local variable. Pass it into the spawn closure as `factory_for_client`.

- [ ] **Step 6: Build check**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo check 2>&1 | Select-Object -Last 8
```

Expected: no errors (may have unused field warnings — fine)

- [ ] **Step 7: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "feat: add Factory and CapabilityProxy fields to daemon Server"
```

---

### Task 2: Fix `AgentInfo` — update sessions table in SQLite

**Files:**
- Modify: `src/daemon/server.rs` — inside the `AgentInfo` command handler

- [ ] **Step 1: Find the `AgentInfo` handler**

Search for `v["command"] == "AgentInfo"` in `src/daemon/server.rs`. Currently it calls `record_event(...)` but does NOT update the `sessions` table.

- [ ] **Step 2: Add the DB update inside the handler**

Find this section:
```rust
} else if v["command"] == "AgentInfo" {
    if let Some(agent) = v["agent"].as_str() {
        let project = v["project"].as_str();
        sessions_for_client.record_event(
            &session_id,
            "agent_info",
            &format!("agent={} project={}", agent, project.unwrap_or("-")),
        );
        let _ = writer.write_all(b"{\"event\":\"AgentInfoAck\"}\n").await;
    }
```

Replace with:
```rust
} else if v["command"] == "AgentInfo" {
    if let Some(agent) = v["agent"].as_str() {
        let project = v["project"].as_str();
        sessions_for_client.record_event(
            &session_id,
            "agent_info",
            &format!("agent={} project={}", agent, project.unwrap_or("-")),
        );
        // Update the session row so queries see the real agent name
        if let Ok(conn) = rusqlite::Connection::open(
            crate::session::SessionStore::default_path()
        ) {
            let _ = conn.execute(
                "UPDATE sessions SET agent=?1, project=COALESCE(?2, project) WHERE id=?3",
                rusqlite::params![agent, project, session_id],
            );
        }
        let _ = writer.write_all(b"{\"event\":\"AgentInfoAck\"}\n").await;
    }
```

- [ ] **Step 3: Build check**

```powershell
cargo check 2>&1 | Select-Object -Last 5
```

Expected: no errors

- [ ] **Step 4: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "fix: AgentInfo command now updates agent name in sessions table"
```

---

### Task 3: Add `SubmitJob` and `GetJob` TCP commands

**Files:**
- Modify: `src/daemon/server.rs` — command dispatch loop

- [ ] **Step 1: Write a failing integration test in `src/factory.rs`**

In the `#[cfg(test)] mod tests` block of `src/factory.rs`, add:

```rust
#[tokio::test]
async fn shell_job_captures_output() {
    let tmp = TempDir::new().unwrap();
    let (tx, mut rx) = broadcast::channel::<String>(16);
    let factory = Factory {
        db_path: Arc::new(tmp.path().join("t.db")),
        tx,
        write_lock: Arc::new(Mutex::new(())),
    };
    factory.ensure_table();

    let job = Job::new("echo test", "claude", None, None);
    let id = factory.submit(
        job,
        Box::pin(async {
            let out = tokio::process::Command::new("cmd")
                .args(["/C", "echo hello"])
                .output()
                .await?;
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }),
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let j = factory.get(&id).unwrap();
    assert_eq!(j.status, JobStatus::Completed);
    assert!(j.result.as_deref().unwrap_or("").contains("hello"));
}
```

- [ ] **Step 2: Run test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test factory::tests::shell_job_captures_output
```

Expected: PASS

- [ ] **Step 3: Add `SubmitJob` handler to the dispatch loop in `src/daemon/server.rs`**

Inside the `tokio::select!` command loop, after the last `} else if` branch and before the `line.clear();`, add:

```rust
} else if v["command"] == "SubmitJob" {
    let description = v["description"].as_str().unwrap_or("unnamed task").to_string();
    let agent_name = v["agent"].as_str().unwrap_or("unknown").to_string();
    let project = v["project"].as_str().map(|s| s.to_string());
    let webhook_url = v["webhook_url"].as_str().map(|s| s.to_string());
    let shell_cmd = v["shell_cmd"].as_str().unwrap_or("").to_string();

    if shell_cmd.is_empty() {
        let err = serde_json::json!({
            "event": "JobError",
            "error": "shell_cmd is required"
        });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
    } else {
        let job = Job::new(
            &description,
            &agent_name,
            project.as_deref(),
            webhook_url.as_deref(),
        );
        let task = Box::pin(async move {
            let output = tokio::process::Command::new("cmd")
                .args(["/C", &shell_cmd])
                .output()
                .await?;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.success() {
                Ok(stdout)
            } else {
                Err(anyhow::anyhow!("exit {}: {}", output.status, stderr))
            }
        });
        let job_id = factory_for_client.submit(job, task);
        let response = serde_json::json!({
            "event": "JobSubmitted",
            "job_id": job_id.to_string()
        });
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    }
} else if v["command"] == "GetJob" {
    if let Some(id_str) = v["job_id"].as_str() {
        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
            match factory_for_client.get(&id) {
                Some(job) => {
                    let response = serde_json::json!({
                        "event": "JobInfo",
                        "job": job
                    });
                    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
                }
                None => {
                    let err = serde_json::json!({
                        "event": "JobError",
                        "error": format!("job {} not found", id_str)
                    });
                    let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
                }
            }
        }
    }
```

- [ ] **Step 4: Clone `factory_for_client` before the spawn closure**

In `run_inner()`, where `sessions_for_client = self.sessions.clone()` is set, add:
```rust
let factory_for_client = factory.clone();
```

- [ ] **Step 5: Build check**

```powershell
cargo check 2>&1 | Select-Object -Last 5
```

Expected: no errors

- [ ] **Step 6: Commit**

```powershell
git add src/daemon/server.rs src/factory.rs
git commit -m "feat: add SubmitJob and GetJob TCP commands to daemon"
```

---

### Task 4: Add `ListInbox`, `ListRunning`, and `ExecuteCapability` TCP commands

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Add `ListInbox` handler**

After the `GetJob` handler, add:

```rust
} else if v["command"] == "ListInbox" {
    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
    let jobs = factory_for_client.list_inbox(limit);
    let response = serde_json::json!({
        "event": "InboxList",
        "jobs": jobs
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
} else if v["command"] == "ListRunning" {
    let jobs = factory_for_client.list_running();
    let response = serde_json::json!({
        "event": "RunningList",
        "jobs": jobs
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
```

- [ ] **Step 2: Add `ExecuteCapability` handler**

Add `proxy_for_client` clone before the spawn (alongside `factory_for_client`):
```rust
let proxy_for_client = self.proxy.clone();
```

Then add the handler:

```rust
} else if v["command"] == "ExecuteCapability" {
    let capability = v["capability"].as_str().unwrap_or("").to_string();
    let input = v["input"].as_str().unwrap_or("").to_string();

    if capability.is_empty() {
        let err = serde_json::json!({
            "event": "CapabilityError",
            "error": "capability name is required"
        });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
    } else {
        match proxy_for_client.execute(&capability, &input) {
            Ok(result) => {
                let response = serde_json::json!({
                    "event": "CapabilityResult",
                    "capability": capability,
                    "result": result
                });
                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
            }
            Err(e) => {
                let err = serde_json::json!({
                    "event": "CapabilityError",
                    "capability": capability,
                    "error": e.to_string()
                });
                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
            }
        }
    }
} else if v["command"] == "ListCapabilities" {
    let caps: Vec<serde_json::Value> = proxy_for_client
        .store()
        .list()
        .iter()
        .map(|c| serde_json::json!({
            "name": c.name,
            "description": c.description,
            "platforms": c.platforms
        }))
        .collect();
    let response = serde_json::json!({
        "event": "CapabilityList",
        "capabilities": caps
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
```

- [ ] **Step 3: Build check + full test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo check
cargo test 2>&1 | Select-Object -Last 8
```

Expected: no new failures (114+ pass, 3 pre-existing git failures)

- [ ] **Step 4: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "feat: add ListInbox, ListRunning, ExecuteCapability, ListCapabilities TCP commands"
```

---

## TCP Command Reference (after this plan)

| Command | Required fields | Response event |
|---------|----------------|----------------|
| `SubmitJob` | `description`, `shell_cmd` | `JobSubmitted` {job_id} |
| `GetJob` | `job_id` | `JobInfo` {job} |
| `ListInbox` | `limit` (opt, default 20) | `InboxList` {jobs} |
| `ListRunning` | — | `RunningList` {jobs} |
| `ExecuteCapability` | `capability`, `input` | `CapabilityResult` or `CapabilityError` |
| `ListCapabilities` | — | `CapabilityList` {capabilities} |
| `AgentInfo` (fixed) | `agent`, `project` | `AgentInfoAck` + DB UPDATE |
