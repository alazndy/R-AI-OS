# Phase 7: Evolutionary Intelligence — Learning from Task Outcomes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `InstinctEngine` learn automatically from Factory job outcomes — successful jobs teach "what works", failed jobs teach "what to avoid" — by subscribing to the daemon's broadcast channel and extracting instinct suggestions from job completion events.

**Architecture:** A new `EvolutionWorker` runs as a daemon background task. It subscribes to the broadcast channel and listens for `JobComplete` and `JobFailed` events. For each event it calls `suggest_from_outcome()` (new function) to generate 0-3 instinct candidates, then appends novel candidates to `InstinctEngine`. A 7-day expiry window (stored in SQLite) prevents stale instincts from accumulating. No LLM is involved — suggestions are heuristic pattern matches on description + result text.

**Tech Stack:** `crate::instinct::{InstinctEngine, InstinctData}`, `crate::session::SessionStore`, `rusqlite`, existing broadcast channel.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Modify | `src/instinct.rs` | Add `suggest_from_outcome()`, `suggest_from_failure()`, SQLite-backed instinct storage |
| Create | `src/evolution.rs` | `EvolutionWorker` — subscribes to broadcast, calls suggest, persists instincts |
| Modify | `src/db.rs` | Add `instinct_candidates` table |
| Modify | `src/daemon/server.rs` | Spawn `EvolutionWorker` alongside other workers |
| Modify | `src/lib.rs` | `pub mod evolution;` |

---

### Task 1: Add `instinct_candidates` table to SQLite

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Add table inside `migrate()`**

After the `session_events` block, add:

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS instinct_candidates (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        rule        TEXT NOT NULL UNIQUE,
        source      TEXT NOT NULL,
        confidence  REAL NOT NULL DEFAULT 0.5,
        created_at  TEXT NOT NULL DEFAULT (datetime('now')),
        expires_at  TEXT NOT NULL,
        promoted    INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_instinct_promoted ON instinct_candidates(promoted);
    ",
)?;
```

- [ ] **Step 2: Write test**

```rust
#[test]
fn instinct_candidates_table_exists() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instinct_candidates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 3: Run test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test db::tests::instinct_candidates_table_exists
```

Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add src/db.rs
git commit -m "feat(evolution): add instinct_candidates table to SQLite schema"
```

---

### Task 2: Add `suggest_from_outcome()` and `suggest_from_failure()` to `src/instinct.rs`

**Files:**
- Modify: `src/instinct.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/instinct.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_cargo_test_job_suggests_tdd_rule() {
        let suggestions = suggest_from_outcome(
            "run tests for auth module",
            "cargo test",
            "117 passed; 0 failed",
        );
        // Should suggest something about tests passing or TDD being effective
        assert!(!suggestions.is_empty());
        let combined = suggestions.join(" ").to_lowercase();
        assert!(combined.contains("test") || combined.contains("pass"));
    }

    #[test]
    fn failed_job_suggests_investigation_rule() {
        let suggestions = suggest_from_failure(
            "refactor auth module",
            "cargo check",
            "error[E0308]: mismatched types",
        );
        assert!(!suggestions.is_empty());
        let combined = suggestions.join(" ").to_lowercase();
        assert!(combined.contains("type") || combined.contains("error") || combined.contains("check"));
    }

    #[test]
    fn empty_result_produces_no_suggestions() {
        let suggestions = suggest_from_outcome("task", "cmd", "");
        assert!(suggestions.is_empty());
    }
}
```

- [ ] **Step 2: Confirm compile error**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test instinct::tests 2>&1 | Select-Object -Last 5
```

Expected: compile error (`suggest_from_outcome` not defined)

- [ ] **Step 3: Implement both functions**

Add to `src/instinct.rs` after the existing `suggest_from_health()` function:

```rust
/// Generate instinct suggestions from a SUCCESSFUL Factory job.
/// Returns 0-2 short rule strings based on heuristic pattern matching.
pub fn suggest_from_outcome(description: &str, command: &str, result: &str) -> Vec<String> {
    if result.trim().is_empty() {
        return vec![];
    }

    let desc_lower = description.to_lowercase();
    let result_lower = result.to_lowercase();
    let mut suggestions = Vec::new();

    // Test pass pattern
    if command.contains("test") || desc_lower.contains("test") {
        if result_lower.contains("passed") && !result_lower.contains("failed") {
            suggestions.push(format!(
                "Test suite passes for '{}' — keep TDD discipline before adding features",
                truncate(&desc_lower, 40)
            ));
        }
    }

    // Build success pattern
    if command.contains("build") || command.contains("cargo build") || command.contains("cargo check") {
        if !result_lower.contains("error") {
            suggestions.push(format!(
                "'{}' builds cleanly — run `cargo check` before submitting PRs",
                truncate(&desc_lower, 40)
            ));
        }
    }

    // Security scan pattern
    if desc_lower.contains("security") || command.contains("security") {
        suggestions.push(
            "Security scan succeeded — run `raios security` before every commit".to_string()
        );
    }

    suggestions.truncate(2);
    suggestions
}

/// Generate instinct suggestions from a FAILED Factory job.
pub fn suggest_from_failure(description: &str, command: &str, error: &str) -> Vec<String> {
    if error.trim().is_empty() {
        return vec![];
    }

    let error_lower = error.to_lowercase();
    let desc_lower = description.to_lowercase();
    let mut suggestions = Vec::new();

    if error_lower.contains("mismatched types") || error_lower.contains("e0308") {
        suggestions.push(
            "Type mismatch errors — run `cargo check` after every refactor, not just at the end"
                .to_string(),
        );
    }

    if error_lower.contains("borrow") || error_lower.contains("lifetime") {
        suggestions.push(
            "Borrow checker failure — prefer cloning over fighting the borrow checker in hot paths"
                .to_string(),
        );
    }

    if error_lower.contains("permission denied") || error_lower.contains("access is denied") {
        suggestions.push(format!(
            "'{}' failed with permission error — check file locks before running shell commands",
            truncate(&desc_lower, 40)
        ));
    }

    if error_lower.contains("connection refused") || error_lower.contains("failed to connect") {
        suggestions.push(
            "Connection failure — ensure aiosd daemon is running before agent-to-daemon tasks"
                .to_string(),
        );
    }

    suggestions.truncate(2);
    suggestions
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
```

- [ ] **Step 4: Run tests**

```powershell
cargo test instinct::tests
```

Expected: 3 tests PASS

- [ ] **Step 5: Commit**

```powershell
git add src/instinct.rs
git commit -m "feat(evolution): add suggest_from_outcome and suggest_from_failure to InstinctEngine"
```

---

### Task 3: Implement `EvolutionWorker` in `src/evolution.rs`

**Files:**
- Create: `src/evolution.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing test**

Create `src/evolution.rs` with only the test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn job_complete_event_generates_candidate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = CandidateStore::new(db_path.clone());

        let job_event = serde_json::json!({
            "event": "JobComplete",
            "job_id": "abc-123",
            "status": "completed",
            "result": "117 passed; 0 failed",
            "completed_at": "2026-05-20 12:00:00"
        });

        // Pretend the job was for "run tests"
        process_job_event(
            &job_event,
            "run tests for auth module",
            "cargo test",
            &store,
        );

        let candidates = store.list_pending(10);
        assert!(!candidates.is_empty());
    }

    #[tokio::test]
    async fn job_failed_event_generates_candidate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = CandidateStore::new(db_path);

        let job_event = serde_json::json!({
            "event": "JobFailed",
            "job_id": "xyz-456",
            "status": "failed",
            "error": "error[E0308]: mismatched types"
        });

        process_job_event(&job_event, "refactor auth", "cargo check", &store);

        let candidates = store.list_pending(10);
        assert!(!candidates.is_empty());
    }
}
```

- [ ] **Step 2: Implement `src/evolution.rs`**

```rust
//! Evolution Worker — learns from Factory job outcomes.
//!
//! Subscribes to the daemon broadcast channel, extracts learning signals from
//! JobComplete and JobFailed events, and persists instinct candidates to SQLite.
//! Candidates expire after 7 days unless promoted to the active InstinctEngine.
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;

use crate::instinct::{suggest_from_failure, suggest_from_outcome};

const EXPIRY_DAYS: i64 = 7;

// ─── Candidate store ──────────────────────────────────────────────────────────

pub struct CandidateStore {
    db_path: PathBuf,
}

impl CandidateStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        let db_path = db_path.into();
        let store = Self { db_path };
        store.ensure_table();
        store
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("workspace.db")
    }

    fn connect(&self) -> rusqlite::Result<Connection> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        Connection::open(&self.db_path)
    }

    fn ensure_table(&self) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS instinct_candidates (
                    id         INTEGER PRIMARY KEY AUTOINCREMENT,
                    rule       TEXT NOT NULL UNIQUE,
                    source     TEXT NOT NULL,
                    confidence REAL NOT NULL DEFAULT 0.5,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    expires_at TEXT NOT NULL,
                    promoted   INTEGER NOT NULL DEFAULT 0
                );",
            );
        }
    }

    pub fn insert(&self, rule: &str, source: &str, confidence: f32) {
        if let Ok(conn) = self.connect() {
            let expires = chrono::Local::now()
                .checked_add_signed(chrono::Duration::days(EXPIRY_DAYS))
                .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "2099-01-01 00:00:00".to_string());
            let _ = conn.execute(
                "INSERT OR IGNORE INTO instinct_candidates (rule, source, confidence, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![rule, source, confidence as f64, expires],
            );
        }
    }

    pub fn list_pending(&self, limit: usize) -> Vec<String> {
        let Ok(conn) = self.connect() else { return vec![]; };
        let mut stmt = conn
            .prepare(
                "SELECT rule FROM instinct_candidates
                 WHERE promoted = 0 AND expires_at > datetime('now')
                 ORDER BY confidence DESC LIMIT ?1",
            )
            .ok();
        match &mut stmt {
            Some(s) => s
                .query_map(params![limit as i64], |row| row.get(0))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn promote(&self, rule: &str) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE instinct_candidates SET promoted = 1 WHERE rule = ?1",
                params![rule],
            );
        }
    }

    pub fn sweep_expired(&self) -> usize {
        let Ok(conn) = self.connect() else { return 0; };
        conn.execute(
            "DELETE FROM instinct_candidates WHERE expires_at <= datetime('now')",
            [],
        )
        .unwrap_or(0)
    }
}

// ─── Event processing ─────────────────────────────────────────────────────────

/// Process a single broadcast event. Exported for testing.
pub fn process_job_event(
    event: &serde_json::Value,
    description: &str,
    command: &str,
    store: &CandidateStore,
) {
    match event["event"].as_str() {
        Some("JobComplete") => {
            let result = event["result"].as_str().unwrap_or("");
            let suggestions = suggest_from_outcome(description, command, result);
            for s in suggestions {
                store.insert(&s, description, 0.7);
            }
        }
        Some("JobFailed") => {
            let error = event["error"].as_str().unwrap_or("");
            let suggestions = suggest_from_failure(description, command, error);
            for s in suggestions {
                store.insert(&s, description, 0.6);
            }
        }
        _ => {}
    }
}

// ─── Background worker ────────────────────────────────────────────────────────

/// Subscribe to the broadcast channel and process job events indefinitely.
/// Spawns its own sweep task for expired candidates.
pub async fn start_evolution_worker(mut rx: broadcast::Receiver<String>) {
    println!("[Evolution] Worker started.");
    let store = CandidateStore::new(CandidateStore::default_path());

    // Weekly sweep for expired candidates
    {
        let store2 = CandidateStore::new(CandidateStore::default_path());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;
                let removed = store2.sweep_expired();
                if removed > 0 {
                    eprintln!("[Evolution] Swept {} expired instinct candidates", removed);
                }
            }
        });
    }

    while let Ok(msg) = rx.recv().await {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&msg) else {
            continue;
        };
        let kind = event["event"].as_str().unwrap_or("");
        if kind != "JobComplete" && kind != "JobFailed" {
            continue;
        }
        // We don't have the original description/command here — the job record in
        // SQLite does. Look it up by job_id.
        if let Some(job_id_str) = event["job_id"].as_str() {
            if let Some((desc, cmd)) = lookup_job_meta(job_id_str) {
                process_job_event(&event, &desc, &cmd, &store);
            }
        }
    }
}

fn lookup_job_meta(job_id: &str) -> Option<(String, String)> {
    let conn = Connection::open(CandidateStore::default_path()).ok()?;
    conn.query_row(
        "SELECT description, COALESCE(agent, '') FROM factory_jobs WHERE id = ?1",
        params![job_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn job_complete_event_generates_candidate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = CandidateStore::new(tmp.path().join("test.db"));

        let job_event = serde_json::json!({
            "event": "JobComplete",
            "job_id": "abc-123",
            "result": "117 passed; 0 failed"
        });

        process_job_event(&job_event, "run tests for auth module", "cargo test", &store);

        let candidates = store.list_pending(10);
        assert!(!candidates.is_empty());
    }

    #[tokio::test]
    async fn job_failed_event_generates_candidate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = CandidateStore::new(tmp.path().join("test.db"));

        let job_event = serde_json::json!({
            "event": "JobFailed",
            "job_id": "xyz-456",
            "error": "error[E0308]: mismatched types"
        });

        process_job_event(&job_event, "refactor auth", "cargo check", &store);

        let candidates = store.list_pending(10);
        assert!(!candidates.is_empty());
    }
}
```

- [ ] **Step 3: Register in `src/lib.rs`**

Add `pub mod evolution;` after `pub mod edge;`.

- [ ] **Step 4: Run tests**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test evolution::tests
```

Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```powershell
git add src/evolution.rs src/lib.rs
git commit -m "feat(evolution): add EvolutionWorker — learns instinct candidates from job outcomes"
```

---

### Task 4: Spawn `EvolutionWorker` in daemon + expose via TCP

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Spawn worker in `run_inner()`**

In `run_inner()`, after the existing worker spawns (health, git, validation, etc.), add:

```rust
let evolution_rx = tx.subscribe();
tokio::spawn(async move {
    crate::evolution::start_evolution_worker(evolution_rx).await;
});
```

- [ ] **Step 2: Add `ListInstinctCandidates` and `PromoteInstinct` TCP commands**

In the dispatch loop, add:

```rust
} else if v["command"] == "ListInstinctCandidates" {
    let limit = v["limit"].as_u64().unwrap_or(20) as usize;
    let store = crate::evolution::CandidateStore::new(
        crate::evolution::CandidateStore::default_path()
    );
    let candidates = store.list_pending(limit);
    let response = serde_json::json!({
        "event": "InstinctCandidates",
        "candidates": candidates
    });
    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
} else if v["command"] == "PromoteInstinct" {
    if let Some(rule) = v["rule"].as_str() {
        let store = crate::evolution::CandidateStore::new(
            crate::evolution::CandidateStore::default_path()
        );
        store.promote(rule);
        let mut engine = crate::instinct::InstinctEngine::init();
        engine.add_rule(rule.to_string());
        let _ = engine.save();
        let response = serde_json::json!({
            "event": "InstinctPromoted",
            "rule": rule
        });
        let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
    }
```

- [ ] **Step 3: Build check + full test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo check
cargo test 2>&1 | Select-Object -Last 5
```

Expected: no new failures

- [ ] **Step 4: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "feat(evolution): spawn EvolutionWorker in daemon, add ListInstinctCandidates and PromoteInstinct commands"
```
