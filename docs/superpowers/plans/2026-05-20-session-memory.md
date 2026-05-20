# Session Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Track what each agent connection does during its lifetime — files touched, commands sent, whispers received, manual notes — and make this retrievable via MCP and optionally auto-appended to `memory.md` on session end.

**Architecture:** A `SessionStore` manages a SQLite `sessions` + `session_events` table. When an agent connects to the daemon TCP port, a session is auto-started and its ID sent back. When the connection closes, the session ends. Agents can write structured notes via a new `session/note` MCP tool. Sessions are exposed as an MCP resource (`raios://session/current`, `raios://session/{id}`). At session end, R-AI-OS optionally writes a summary line to the project's `memory.md`.

**Tech Stack:** `rusqlite` (already a dependency), `chrono` (already a dependency), no new crates.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Create | `src/session.rs` | `SessionStore`, `Session`, `SessionEvent` types + SQLite I/O |
| Modify | `src/db.rs` | Add `sessions` + `session_events` tables to `migrate()` |
| Modify | `src/daemon/server.rs` | Auto-start session on connect, auto-end on disconnect, record IPC events |
| Modify | `src/kernel.rs` | Pass `SessionStore` to MCP-over-TCP handler |
| Modify | `src/mcp_server.rs` | Add `session/note` tool + `raios://session/current` resource |
| Modify | `src/lib.rs` | `pub mod session;` |

---

### Task 1: Add `sessions` and `session_events` tables

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Add tables to `migrate()`**

After the existing `CREATE TABLE IF NOT EXISTS tasks` block, add:

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS sessions (
        id          TEXT PRIMARY KEY,
        agent       TEXT NOT NULL,
        project     TEXT,
        started_at  TEXT NOT NULL,
        ended_at    TEXT,
        summary     TEXT
    );

    CREATE TABLE IF NOT EXISTS session_events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        event_type  TEXT NOT NULL,
        data        TEXT NOT NULL DEFAULT '',
        timestamp   TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE INDEX IF NOT EXISTS idx_se_session ON session_events(session_id);
    ",
)?;
```

- [ ] **Step 2: Write failing test**

```rust
#[test]
fn sessions_table_exists_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 3: Run test**

```
cargo test db::tests::sessions_table_exists_after_migrate
```

Expected: PASS

- [ ] **Step 4: Commit**

```
git add src/db.rs
git commit -m "feat: add sessions and session_events tables to SQLite schema"
```

---

### Task 2: Implement `src/session.rs`

**Files:**
- Create: `src/session.rs`

- [ ] **Step 1: Write failing tests first**

Create `src/session.rs` with only the test module (no implementation yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store(tmp: &TempDir) -> SessionStore {
        SessionStore::new(tmp.path().join("test.db"))
    }

    #[test]
    fn start_and_end_session() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let id = s.start("claude", Some("r-ai-os"));
        assert!(!id.is_empty());
        s.end(&id, Some("fixed the build"));
        let sess = s.get(&id).unwrap();
        assert!(sess.ended_at.is_some());
        assert_eq!(sess.summary.as_deref(), Some("fixed the build"));
    }

    #[test]
    fn record_event_and_list() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let id = s.start("gemini", None);
        s.record_event(&id, "file_read", "src/main.rs");
        s.record_event(&id, "whisper_received", "compile_error in src/lib.rs");
        let events = s.events(&id);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "file_read");
    }

    #[test]
    fn current_session_is_most_recent_open() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let id1 = s.start("claude", None);
        let id2 = s.start("gemini", None);
        s.end(&id1, None);
        let current = s.current_open();
        assert_eq!(current.map(|s| s.id), Some(id2));
    }

    #[test]
    fn append_to_memory_md_writes_summary() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let id = s.start("claude", Some("my-proj"));
        s.record_event(&id, "note", "implemented auth flow");
        s.end(&id, Some("auth flow done"));

        let mem_path = tmp.path().join("memory.md");
        std::fs::write(&mem_path, "# Memory\n").unwrap();
        s.append_to_memory(&id, &mem_path);

        let content = std::fs::read_to_string(&mem_path).unwrap();
        assert!(content.contains("auth flow done"));
        assert!(content.contains("claude"));
    }
}
```

- [ ] **Step 2: Run tests — confirm compile error**

```
cargo test session::tests
```

Expected: compile error (module not found)

- [ ] **Step 3: Implement `src/session.rs`**

```rust
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent: String,
    pub project: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub data: String,
    pub timestamp: String,
}

// ─── Store ────────────────────────────────────────────────────────────────────

pub struct SessionStore {
    db_path: PathBuf,
}

impl SessionStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        let db_path = db_path.into();
        let store = Self { db_path };
        store.ensure_tables();
        store
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("workspace.db")
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn ensure_tables(&self) {
        if let Ok(conn) = self.connect() {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id         TEXT PRIMARY KEY,
                    agent      TEXT NOT NULL,
                    project    TEXT,
                    started_at TEXT NOT NULL,
                    ended_at   TEXT,
                    summary    TEXT
                );
                CREATE TABLE IF NOT EXISTS session_events (
                    id         INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    event_type TEXT NOT NULL,
                    data       TEXT NOT NULL DEFAULT '',
                    timestamp  TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_se_session ON session_events(session_id);",
            );
        }
    }

    /// Start a new session and return its ID.
    pub fn start(&self, agent: &str, project: Option<&str>) -> String {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "INSERT INTO sessions (id, agent, project, started_at) VALUES (?1,?2,?3,?4)",
                params![id, agent, project, now],
            );
        }
        id
    }

    /// End a session and optionally record a summary.
    pub fn end(&self, id: &str, summary: Option<&str>) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE sessions SET ended_at=?2, summary=?3 WHERE id=?1",
                params![id, now, summary],
            );
        }
    }

    /// Record a structured event for a session.
    pub fn record_event(&self, session_id: &str, event_type: &str, data: &str) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "INSERT INTO session_events (session_id, event_type, data, timestamp)
                 VALUES (?1,?2,?3,?4)",
                params![session_id, event_type, data, now],
            );
        }
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id, agent, project, started_at, ended_at, summary FROM sessions WHERE id=?1",
            params![id],
            |row| Ok(Session {
                id: row.get(0)?,
                agent: row.get(1)?,
                project: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                summary: row.get(5)?,
            }),
        ).ok()
    }

    pub fn events(&self, session_id: &str) -> Vec<SessionEvent> {
        let Ok(conn) = self.connect() else { return vec![]; };
        let mut stmt = conn.prepare(
            "SELECT id, session_id, event_type, data, timestamp
             FROM session_events WHERE session_id=?1 ORDER BY id"
        ).ok();
        match &mut stmt {
            Some(s) => s.query_map(params![session_id], |row| Ok(SessionEvent {
                id: row.get(0)?,
                session_id: row.get(1)?,
                event_type: row.get(2)?,
                data: row.get(3)?,
                timestamp: row.get(4)?,
            })).ok().map(|r| r.flatten().collect()).unwrap_or_default(),
            None => vec![],
        }
    }

    /// Return the most-recently-started session that hasn't ended yet.
    pub fn current_open(&self) -> Option<Session> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id, agent, project, started_at, ended_at, summary
             FROM sessions WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
            [],
            |row| Ok(Session {
                id: row.get(0)?,
                agent: row.get(1)?,
                project: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                summary: row.get(5)?,
            }),
        ).ok()
    }

    /// Recent completed sessions (newest first).
    pub fn recent(&self, limit: usize) -> Vec<Session> {
        let Ok(conn) = self.connect() else { return vec![]; };
        let mut stmt = conn.prepare(
            "SELECT id, agent, project, started_at, ended_at, summary
             FROM sessions WHERE ended_at IS NOT NULL
             ORDER BY ended_at DESC LIMIT ?1"
        ).ok();
        match &mut stmt {
            Some(s) => s.query_map(params![limit as i64], |row| Ok(Session {
                id: row.get(0)?,
                agent: row.get(1)?,
                project: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                summary: row.get(5)?,
            })).ok().map(|r| r.flatten().collect()).unwrap_or_default(),
            None => vec![],
        }
    }

    /// Append a one-line session summary entry to a project's memory.md.
    pub fn append_to_memory(&self, session_id: &str, memory_path: &Path) {
        let Some(sess) = self.get(session_id) else { return; };
        let summary = sess.summary.as_deref().unwrap_or("(no summary)");
        let ended = sess.ended_at.as_deref().unwrap_or("?");
        let line = format!(
            "\n- **{}** `{}` {} — {}\n",
            ended, sess.agent,
            sess.project.as_deref().map(|p| format!("[{}]", p)).unwrap_or_default(),
            summary
        );
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(memory_path)
            .map(|mut f| { use std::io::Write; let _ = f.write_all(line.as_bytes()); });
    }
}
```

- [ ] **Step 4: Register module in `src/lib.rs`**

```rust
pub mod session;
```

- [ ] **Step 5: Run tests**

```
cargo test session::tests
```

Expected: 4 tests PASS

- [ ] **Step 6: Commit**

```
git add src/session.rs src/lib.rs
git commit -m "feat: add SessionStore with SQLite-backed session memory"
```

---

### Task 3: Auto-start/end sessions in daemon TCP handler

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Add `SessionStore` to `Server` struct**

At the top of `src/daemon/server.rs`, add:

```rust
use crate::session::SessionStore;
```

Change `Server` struct:

```rust
pub struct Server {
    state: Arc<RwLock<DaemonState>>,
    execution_proxy: super::proxy::ExecutionProxy,
    sessions: Arc<SessionStore>,
}
```

Update `new()`:

```rust
pub fn new(state: Arc<RwLock<DaemonState>>) -> Self {
    let execution_proxy = super::proxy::ExecutionProxy::new(state.clone());
    let sessions = Arc::new(SessionStore::new(SessionStore::default_path()));
    Self { state, execution_proxy, sessions }
}
```

- [ ] **Step 2: Auto-start session and send ID to agent on connect**

In `run_inner()`, after the auth challenge succeeds (after `println!("[Daemon] Client {} authenticated.", addr);`), add:

```rust
// Start a session for this connection
let agent_name = "daemon-client"; // will be updated if agent sends AgentInfo command
let session_id = sessions_ref.start(agent_name, None);
let session_msg = serde_json::json!({
    "event": "SessionStarted",
    "session_id": session_id
});
let _ = writer.write_all(format!("{}\n", session_msg).as_bytes()).await;
```

Pass `sessions` into the spawned task:

```rust
let sessions_ref = self.sessions.clone();
tokio::spawn(async move {
    // ... existing client loop ...
    // At the end of the loop (client disconnected):
    sessions_ref.end(&session_id, None);
});
```

- [ ] **Step 3: Handle `AgentInfo` command to tag the session**

Inside the command dispatch, add a new branch before the closing `}`:

```rust
} else if v["command"] == "AgentInfo" {
    if let Some(agent) = v["agent"].as_str() {
        let project = v["project"].as_str();
        let _ = sessions_ref.record_event(&session_id, "agent_info",
            &format!("agent={} project={}", agent, project.unwrap_or("-")));
        // Tag the session with the real agent name (overwrite placeholder)
        if let Ok(conn) = rusqlite::Connection::open(SessionStore::default_path()) {
            let _ = conn.execute(
                "UPDATE sessions SET agent=?1, project=?2 WHERE id=?3",
                rusqlite::params![agent, project, session_id],
            );
        }
        let _ = writer.write_all(b"{\"event\":\"AgentInfoAck\"}\n").await;
    }
```

- [ ] **Step 4: Record key IPC events**

In the existing `Handover` command handler, after approval, add:

```rust
sessions_ref.record_event(&session_id, "handover", &format!("target={target} instruction={instruction}"));
```

In the existing `RequestFileChange` handler, add:

```rust
sessions_ref.record_event(&session_id, "file_change_request", &format!("path={}", approval.path));
```

- [ ] **Step 5: Build check**

```
cargo check
```

Expected: no errors

- [ ] **Step 6: Commit**

```
git add src/daemon/server.rs
git commit -m "feat: auto-start/end sessions on agent TCP connect/disconnect"
```

---

### Task 4: Expose sessions via MCP

**Files:**
- Modify: `src/mcp_server.rs`

- [ ] **Step 1: Add `raios://session/current` to resource list**

In `handle_resources_list()`, add to the resources array:

```rust
{
    "uri": "raios://session/current",
    "name": "Current Session",
    "description": "Most recent open agent session — events, notes, context",
    "mimeType": "application/json"
},
{
    "uri": "raios://session/recent",
    "name": "Recent Sessions",
    "description": "Last 10 completed sessions",
    "mimeType": "application/json"
}
```

- [ ] **Step 2: Handle reading session resources in `handle_resources_read()`**

Add these match arms:

```rust
"raios://session/current" => {
    let store = crate::session::SessionStore::new(
        crate::session::SessionStore::default_path()
    );
    match store.current_open() {
        Some(sess) => {
            let events = store.events(&sess.id);
            let payload = serde_json::json!({ "session": sess, "events": events });
            Ok(json!({
                "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }]
            }))
        }
        None => Ok(json!({
            "contents": [{ "uri": uri, "mimeType": "application/json", "text": "{\"session\":null}" }]
        }))
    }
},
"raios://session/recent" => {
    let store = crate::session::SessionStore::new(
        crate::session::SessionStore::default_path()
    );
    let sessions = store.recent(10);
    let payload = serde_json::json!({ "sessions": sessions });
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "application/json", "text": payload.to_string() }]
    }))
},
```

- [ ] **Step 3: Add `session_note` tool**

In `handle_tools_list()`, add to the tools array:

```rust
{
    "name": "session_note",
    "description": "Write a structured note to the current session memory. Call this when you make a decision, complete a task, or want to remember context for the next session.",
    "inputSchema": {
        "type": "object",
        "required": ["note"],
        "properties": {
            "note": { "type": "string", "description": "The note to record (max 500 chars)" },
            "session_id": { "type": "string", "description": "Session ID (omit to use current open session)" }
        }
    }
},
```

In `handle_tools_call()`, add:

```rust
"session_note" => {
    let note = args["note"].as_str().ok_or("missing note")?;
    let note = &note[..note.len().min(500)];
    let store = crate::session::SessionStore::new(
        crate::session::SessionStore::default_path()
    );
    let session_id = args["session_id"].as_str()
        .map(|s| s.to_string())
        .or_else(|| store.current_open().map(|s| s.id));
    match session_id {
        Some(id) => {
            store.record_event(&id, "note", note);
            Ok(json!({ "recorded": true, "session_id": id }))
        }
        None => Err("no active session — connect via aiosd first".to_string()),
    }
},
```

- [ ] **Step 4: Build check**

```
cargo check
```

Expected: no errors

- [ ] **Step 5: Run all tests**

```
cargo test
```

Expected: no new failures

- [ ] **Step 6: Commit**

```
git add src/mcp_server.rs src/session.rs src/lib.rs
git commit -m "feat: expose session memory via MCP resources and session_note tool"
```

---

### Task 5: Auto-append to memory.md on session end

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Append to memory.md when session ends**

Find the `sessions_ref.end(&session_id, None);` line added in Task 3.

Replace with:

```rust
sessions_ref.end(&session_id, None);

// Auto-append to the project's memory.md if we know the project
if let Some(sess) = sessions_ref.get(&session_id) {
    if let Some(proj) = &sess.project {
        let config = crate::config::Config::load();
        if let Some(config) = config {
            let mem_path = config.dev_ops_path
                .join(proj)
                .join("memory.md");
            if mem_path.exists() {
                sessions_ref.append_to_memory(&session_id, &mem_path);
            }
        }
    }
}
```

- [ ] **Step 2: Build check**

```
cargo check
```

Expected: no errors

- [ ] **Step 3: Run all tests**

```
cargo test
```

Expected: no new failures

- [ ] **Step 4: Commit**

```
git add src/daemon/server.rs
git commit -m "feat: auto-append session summary to memory.md on disconnect"
```
