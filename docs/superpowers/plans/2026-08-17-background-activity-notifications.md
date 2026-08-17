# Background Activity Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface `aiosd`'s background work (health/git/lifecycle/scheduler workers, agent-run completions) and outstanding recommendations as desktop notifications, without the user needing to ask.

**Architecture:** A new `activity_events` table is the single append-only event log. Existing daemon workers write to it at their existing per-tick decision points (no new instrumentation layer). Two read-only HTTP endpoints (`/api/notifications/important`, `/api/notifications/digest`) expose it with per-client cursor tracking. Both `raios-tray.py` and kaira-launcher's GNOME extension poll these on their existing refresh timers and render through their native notification APIs.

**Tech Stack:** Rust (rusqlite, Axum), Python (PySide6/Qt), GJS (GNOME Shell extension JS)

**Spec:** `docs/superpowers/specs/2026-08-17-background-activity-notifications-design.md`

## Global Constraints

- Timestamps: `TEXT` columns formatted `chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")` in Rust at insert time — matches `record_audit_event`'s existing pattern exactly (`crates/raios-core/src/security/verify_chain.rs:26`). Never rely on SQLite's `datetime('now','utc')` DEFAULT for the actual value; it's schema-declared for consistency with `audit_log` but every insert supplies its own value explicitly.
- No new transport: both HTTP endpoints go on the existing Axum router (`crates/raios-runtime/src/server/http/mod.rs`), behind the existing `auth_middleware` layer — no new auth code per-route.
- `digest_interval_secs` lives in the existing `DaemonConfig` struct (`crates/raios-core/src/config.rs`), not a new top-level config section — matches the existing `*_interval_secs` fields.
- Every new DB function goes through `raios_core::db::*` (the existing `pub use module::*;` re-export pattern in `crates/raios-core/src/db/mod.rs`) — never open a second, ad-hoc connection path.
- TDD throughout — a failing test before every implementation step, per project convention.
- `cargo fmt` + `cargo clippy --all-targets --all-features -- -D warnings` clean before every commit (project standard, verified repeatedly this session).
- Regenerate `SIGMAP.md` (`sigmap`) before every commit — mandatory per this repo's own workflow rule (AGENT_CONSTITUTION.md Sec 7).

---

## Task 1: `activity_events` and `notification_cursors` schema

**Files:**
- Modify: `crates/raios-core/src/db/schema.rs` (append a new `conn.execute_batch(...)` block, following the existing `audit_log` block's exact style at line 419)
- Test: `crates/raios-core/src/db/schema.rs` (inline `#[cfg(test)] mod tests`, or the existing test module if schema.rs has one — check first with `grep -n "mod tests" crates/raios-core/src/db/schema.rs`; if absent, add one)

**Interfaces:**
- Produces: two tables other tasks write to and query: `activity_events(id, ts, source, project, tier, summary, detail_json)` and `notification_cursors(client_id, last_important_ts, last_digest_ts)`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn migrate_creates_activity_events_and_notification_cursors_tables() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO activity_events (ts, source, project, tier, summary, detail_json)
         VALUES ('2026-08-17T12:00:00Z', 'lifecycle', 'demo-project', 'important', 'demo-project archived', NULL)",
        [],
    )
    .expect("activity_events insert should succeed");

    conn.execute(
        "INSERT INTO notification_cursors (client_id, last_important_ts, last_digest_ts)
         VALUES ('raios-tray', '1970-01-01T00:00:00Z', '1970-01-01T00:00:00Z')",
        [],
    )
    .expect("notification_cursors insert should succeed");

    let summary: String = conn
        .query_row(
            "SELECT summary FROM activity_events WHERE source = 'lifecycle'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(summary, "demo-project archived");
}
```

Add this inside `schema.rs`'s test module (create `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the file if one doesn't already exist — check first).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-core schema::tests::migrate_creates_activity_events_and_notification_cursors_tables`
Expected: FAIL — `no such table: activity_events`

- [ ] **Step 3: Add the schema**

Append this block to `schema.rs`, in the same function as the existing `audit_log`/`mem_items` blocks (find the function via `grep -n "pub(super) fn migrate" crates/raios-core/src/db/schema.rs`):

```rust
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS activity_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          TEXT NOT NULL DEFAULT (datetime('now','utc')),
            source      TEXT NOT NULL,
            project     TEXT,
            tier        TEXT NOT NULL CHECK(tier IN ('important','routine')),
            summary     TEXT NOT NULL,
            detail_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_activity_events_tier_ts ON activity_events(tier, ts);

        CREATE TABLE IF NOT EXISTS notification_cursors (
            client_id         TEXT PRIMARY KEY,
            last_important_ts TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
            last_digest_ts    TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'
        );
        ",
    )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-core schema::tests::migrate_creates_activity_events_and_notification_cursors_tables`
Expected: PASS

- [ ] **Step 5: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add crates/raios-core/src/db/schema.rs SIGMAP.md
git commit -m "feat(core): add activity_events and notification_cursors schema"
```

---

## Task 2: `log_activity_event` and cursor read/poll functions

**Files:**
- Create: `crates/raios-core/src/db/activity_events.rs`
- Modify: `crates/raios-core/src/db/mod.rs` (add `pub mod activity_events;` and `pub use activity_events::*;` alphabetically among the existing `pub mod`/`pub use` lists)

**Interfaces:**
- Consumes: `rusqlite::Connection` (from `raios_core::db::open_db()`, already used throughout the codebase)
- Produces (used by Tasks 5-9 as producers, Task 11 as the API layer):
  - `pub fn log_activity_event(conn: &Connection, source: &str, project: Option<&str>, tier: &str, summary: &str, detail_json: Option<&str>) -> rusqlite::Result<()>`
  - `pub struct ActivityEvent { pub ts: String, pub source: String, pub project: Option<String>, pub summary: String }`
  - `pub fn poll_important_events(conn: &Connection, client_id: &str) -> rusqlite::Result<Vec<ActivityEvent>>` — reads events after the client's cursor, advances the cursor to the latest returned `ts`, returns them.
  - `pub struct DigestWindow { pub since_ts: String, pub until_ts: String, pub events: Vec<ActivityEvent> }`
  - `pub fn poll_digest_window(conn: &Connection, client_id: &str, digest_interval_secs: i64) -> rusqlite::Result<Option<DigestWindow>>` — returns `None` if the interval hasn't elapsed since `last_digest_ts`; otherwise returns the routine-tier window and advances the cursor to "now".
  - `pub fn prune_activity_events_older_than(conn: &Connection, days: i64) -> rusqlite::Result<usize>` — returns rows deleted (used by Task 10).

- [ ] **Step 1: Write the failing tests**

```rust
use rusqlite::Connection;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::db::schema_migrate_for_tests(&conn).unwrap();
    conn
}

#[test]
fn log_activity_event_writes_a_retrievable_row() {
    let conn = test_conn();
    log_activity_event(&conn, "lifecycle", Some("demo"), "important", "demo archived", None).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn poll_important_events_returns_only_new_rows_and_advances_cursor() {
    let conn = test_conn();
    log_activity_event(&conn, "lifecycle", Some("a"), "important", "a archived", None).unwrap();

    let first_poll = poll_important_events(&conn, "raios-tray").unwrap();
    assert_eq!(first_poll.len(), 1);
    assert_eq!(first_poll[0].summary, "a archived");

    let second_poll = poll_important_events(&conn, "raios-tray").unwrap();
    assert!(second_poll.is_empty(), "cursor must have advanced past the first event");
}

#[test]
fn poll_important_events_cursors_are_isolated_per_client() {
    let conn = test_conn();
    log_activity_event(&conn, "lifecycle", Some("a"), "important", "a archived", None).unwrap();

    let tray_poll = poll_important_events(&conn, "raios-tray").unwrap();
    assert_eq!(tray_poll.len(), 1);

    let gnome_poll = poll_important_events(&conn, "kaira-gnome-ext").unwrap();
    assert_eq!(
        gnome_poll.len(),
        1,
        "a second, distinct client_id must see the event independently"
    );
}

#[test]
fn poll_important_events_ignores_routine_tier_rows() {
    let conn = test_conn();
    log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None).unwrap();

    let events = poll_important_events(&conn, "raios-tray").unwrap();
    assert!(events.is_empty());
}

#[test]
fn poll_digest_window_returns_none_before_interval_elapses() {
    let conn = test_conn();
    log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None).unwrap();

    // First call with a very long interval — cursor starts at epoch, so "now - epoch"
    // is always >= interval on the *first* call regardless of interval size; to test
    // the "not yet" path we must first advance the cursor via one real call, then
    // immediately call again with a large interval.
    let first = poll_digest_window(&conn, "raios-tray", 0).unwrap();
    assert!(first.is_some(), "first-ever call with interval=0 must fire immediately");

    let second = poll_digest_window(&conn, "raios-tray", 3600).unwrap();
    assert!(
        second.is_none(),
        "an immediate second call with a 1h interval must not fire again"
    );
}

#[test]
fn poll_digest_window_groups_only_routine_tier_rows_in_the_window() {
    let conn = test_conn();
    log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None).unwrap();
    log_activity_event(&conn, "lifecycle", Some("b"), "important", "b archived", None).unwrap();

    let window = poll_digest_window(&conn, "raios-tray", 0).unwrap().unwrap();
    assert_eq!(window.events.len(), 1);
    assert_eq!(window.events[0].summary, "a: branch changed");
}

#[test]
fn prune_activity_events_older_than_deletes_only_stale_rows() {
    let conn = test_conn();
    conn.execute(
        "INSERT INTO activity_events (ts, source, project, tier, summary)
         VALUES ('2020-01-01T00:00:00Z', 'git', 'old', 'routine', 'stale row')",
        [],
    )
    .unwrap();
    log_activity_event(&conn, "git", Some("new"), "routine", "fresh row", None).unwrap();

    let deleted = prune_activity_events_older_than(&conn, 30).unwrap();
    assert_eq!(deleted, 1);

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}
```

The tests reference `crate::db::schema_migrate_for_tests` — check whether `schema::migrate` is already reachable from other `db/*.rs` test modules (grep `schema::migrate\|schema_migrate_for_tests` across `crates/raios-core/src/db/*.rs` for the existing convention other modules use to get a fully-migrated in-memory DB in their own tests) and use that exact existing helper instead of inventing a new one. If no such helper exists yet, call `super::schema::migrate(&conn)` directly (schema.rs's `migrate` is `pub(super)`, reachable from sibling `db/` modules).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-core db::activity_events::tests`
Expected: FAIL — compile error, module/functions don't exist yet

- [ ] **Step 3: Implement**

```rust
use rusqlite::{params, Connection, OptionalExtension};

pub fn log_activity_event(
    conn: &Connection,
    source: &str,
    project: Option<&str>,
    tier: &str,
    summary: &str,
    detail_json: Option<&str>,
) -> rusqlite::Result<()> {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO activity_events (ts, source, project, tier, summary, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts, source, project, tier, summary, detail_json],
    )?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActivityEvent {
    pub ts: String,
    pub source: String,
    pub project: Option<String>,
    pub summary: String,
}

fn ensure_cursor(conn: &Connection, client_id: &str) -> rusqlite::Result<(String, String)> {
    let existing = conn
        .query_row(
            "SELECT last_important_ts, last_digest_ts FROM notification_cursors WHERE client_id = ?1",
            params![client_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;

    if let Some(cursor) = existing {
        return Ok(cursor);
    }

    conn.execute(
        "INSERT INTO notification_cursors (client_id) VALUES (?1)",
        params![client_id],
    )?;
    Ok(("1970-01-01T00:00:00Z".to_string(), "1970-01-01T00:00:00Z".to_string()))
}

pub fn poll_important_events(conn: &Connection, client_id: &str) -> rusqlite::Result<Vec<ActivityEvent>> {
    let (last_important_ts, _) = ensure_cursor(conn, client_id)?;

    let mut stmt = conn.prepare(
        "SELECT ts, source, project, summary FROM activity_events
         WHERE tier = 'important' AND ts > ?1 ORDER BY ts ASC",
    )?;
    let events: Vec<ActivityEvent> = stmt
        .query_map(params![last_important_ts], |row| {
            Ok(ActivityEvent {
                ts: row.get(0)?,
                source: row.get(1)?,
                project: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    if let Some(last) = events.last() {
        conn.execute(
            "UPDATE notification_cursors SET last_important_ts = ?1 WHERE client_id = ?2",
            params![last.ts.clone(), client_id],
        )?;
    }

    Ok(events)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DigestWindow {
    pub since_ts: String,
    pub until_ts: String,
    pub events: Vec<ActivityEvent>,
}

pub fn poll_digest_window(
    conn: &Connection,
    client_id: &str,
    digest_interval_secs: i64,
) -> rusqlite::Result<Option<DigestWindow>> {
    let (_, last_digest_ts) = ensure_cursor(conn, client_id)?;

    let last_dt = chrono::DateTime::parse_from_str(&format!("{last_digest_ts} +0000"), "%Y-%m-%dT%H:%M:%SZ %z")
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::DateTime::from_timestamp(0, 0).unwrap());
    let now = chrono::Utc::now();

    if (now - last_dt).num_seconds() < digest_interval_secs {
        return Ok(None);
    }

    let until_ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut stmt = conn.prepare(
        "SELECT ts, source, project, summary FROM activity_events
         WHERE tier = 'routine' AND ts > ?1 AND ts <= ?2 ORDER BY ts ASC",
    )?;
    let events: Vec<ActivityEvent> = stmt
        .query_map(params![last_digest_ts, until_ts], |row| {
            Ok(ActivityEvent {
                ts: row.get(0)?,
                source: row.get(1)?,
                project: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    conn.execute(
        "UPDATE notification_cursors SET last_digest_ts = ?1 WHERE client_id = ?2",
        params![until_ts.clone(), client_id],
    )?;

    Ok(Some(DigestWindow {
        since_ts: last_digest_ts,
        until_ts,
        events,
    }))
}

pub fn prune_activity_events_older_than(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    conn.execute("DELETE FROM activity_events WHERE ts < ?1", params![cutoff])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::migrate(&conn).unwrap();
        conn
    }

    // ... (paste the 7 tests from Step 1 here, replacing the placeholder
    // `crate::db::schema_migrate_for_tests` call with `test_conn()` above)
}
```

Add `chrono` to `crates/raios-core/Cargo.toml` if it isn't already a dependency (check with `grep chrono crates/raios-core/Cargo.toml` first — `raios-core/src/security/verify_chain.rs` already uses `chrono::Utc::now()`, so it almost certainly already is).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-core db::activity_events::tests`
Expected: PASS (all 7 tests)

- [ ] **Step 5: Wire the module in and verify workspace-wide**

In `crates/raios-core/src/db/mod.rs`, add alphabetically:
```rust
pub mod activity_events;
```
and
```rust
pub use activity_events::*;
```

Run: `cargo build --workspace --all-targets && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: clean

- [ ] **Step 6: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add crates/raios-core/src/db/activity_events.rs crates/raios-core/src/db/mod.rs SIGMAP.md
git commit -m "feat(core): add log_activity_event, poll_important_events, poll_digest_window, prune"
```

---

## Task 3: Relocate `ProjectSnapshot`/`build_recommendations` to `raios-runtime`

**Why:** the digest endpoint (Task 11) needs `build_recommendations` for its `top_recommendation` field, but the HTTP server lives in `raios-runtime`, which `raios-surface-cli` (where `build_recommendations` currently lives, in `reflect.rs`) depends *on* — not the reverse. Moving it down the dependency graph is required, not optional; there is no way to call it from `raios-runtime` otherwise.

**Files:**
- Create: `crates/raios-runtime/src/reflect_scoring.rs`
- Modify: `crates/raios-runtime/src/lib.rs` (add `pub mod reflect_scoring;`)
- Modify: `crates/raios-surface-cli/src/cli/reflect.rs` (delete the relocated items, import from `raios_runtime::reflect_scoring` instead)

**Interfaces:**
- Produces: `pub struct ProjectSnapshot { pub name: String, pub dirty_files: usize, pub last_commit_days: Option<u64>, pub has_readme: bool, pub has_memory: bool, pub has_sigmap: bool, pub memory_stale_days: Option<u64> }`, `pub fn snapshot(p: &raios_core::entities::EntityProject) -> ProjectSnapshot`, `pub fn build_recommendations(snaps: &[ProjectSnapshot]) -> Vec<String>`.
- Consumed by: Task 11 (digest endpoint).

- [ ] **Step 1: Confirm the current test suite still describes the behavior being moved**

Run: `cargo test -p raios-surface-cli reflect:: -- --list`
Expected: lists `build_recommendations_of_healthy_projects_is_empty`, `build_recommendations_lists_dirty_projects_by_name`, `build_recommendations_counts_missing_memory_and_sigmap_files`, `build_recommendations_lists_stale_memory_projects_by_name`, `build_recommendations_orders_dirty_memory_sigmap_then_stale_memory` (5 tests, per `crates/raios-surface-cli/src/cli/reflect.rs` lines ~404-457). These move with the code in Step 3 — this step is just confirming the baseline before moving anything.

- [ ] **Step 2: Create the new module with the relocated code**

Read the full current content of `crates/raios-surface-cli/src/cli/reflect.rs` first (`cat crates/raios-surface-cli/src/cli/reflect.rs`) to copy every field/line exactly — do not retype from memory. Move these items verbatim into `crates/raios-runtime/src/reflect_scoring.rs`, changing only visibility (`struct`/`fn` → `pub struct`/`pub fn`, and each field to `pub`):

- `struct ProjectSnapshot { ... }` (7 fields)
- `fn snapshot(p: &EntityProject) -> ProjectSnapshot { ... }`
- `fn count_dirty_files(dir: &Path) -> usize { ... }`
- `fn git_days_since_last_commit(dir: &Path) -> Option<u64> { ... }`
- `fn file_age_days(path: &Path) -> Option<u64> { ... }`
- `fn build_recommendations(snaps: &[ProjectSnapshot]) -> Vec<String> { ... }`
- The 5 `build_recommendations_*` tests (in a `#[cfg(test)] mod tests` block), unchanged — they test pure functions and need no fixture changes.

`snapshot`/`count_dirty_files`/`git_days_since_last_commit`/`file_age_days` stay non-`pub` (`fn`, not `pub fn`) except `snapshot` itself, which Task 11 needs — make exactly `snapshot` and `build_recommendations` (plus the `ProjectSnapshot` type and its fields) `pub`; leave the three git/filesystem helpers private to the new module.

Imports needed at the top of the new file: `use raios_core::entities::EntityProject;`, `use std::path::Path;`, `use std::process::Command;` (same as the original file's imports).

- [ ] **Step 3: Delete the relocated code from `reflect.rs`, import instead**

In `crates/raios-surface-cli/src/cli/reflect.rs`:
- Delete the `struct ProjectSnapshot`, `fn snapshot`, `fn count_dirty_files`, `fn git_days_since_last_commit`, `fn file_age_days`, `fn build_recommendations`, and the 5 relocated tests.
- Add `use raios_runtime::reflect_scoring::{snapshot, build_recommendations, ProjectSnapshot};` at the top.
- Everything else in the file (`cmd_reflect`, `print_json`, `print_report`, the remaining flag-building logic around `has_sigmap`/`has_readme`/`memory_stale_days`) stays exactly as-is — it already calls `snapshot(p)` and `build_recommendations(&snaps)` by name, so once the import resolves, no call-site changes are needed.

Register the new module in `crates/raios-runtime/src/lib.rs` — add `pub mod reflect_scoring;` alphabetically among the existing `pub mod` declarations (check the file first with `grep -n "^pub mod" crates/raios-runtime/src/lib.rs` to place it correctly).

- [ ] **Step 4: Run tests to verify nothing broke**

Run: `cargo test --workspace --lib`
Expected: same pass count as before this task, just with the 5 `build_recommendations_*` tests now reporting under `raios_runtime::reflect_scoring::tests` instead of `raios_surface_cli::cli::reflect::tests`.

- [ ] **Step 5: Clippy and build check**

Run: `cargo build --workspace --all-targets && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: clean

- [ ] **Step 6: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add crates/raios-runtime/src/reflect_scoring.rs crates/raios-runtime/src/lib.rs crates/raios-surface-cli/src/cli/reflect.rs SIGMAP.md
git commit -m "refactor(runtime): relocate ProjectSnapshot/build_recommendations from surface-cli

Needed so the HTTP server (raios-runtime) can build the notification
digest's top_recommendation without a reverse dependency on
raios-surface-cli."
```

---

## Task 4: `digest_interval_secs` config field

**Files:**
- Modify: `crates/raios-core/src/config.rs`
- Test: inline in `config.rs`'s existing test module (check `grep -n "mod tests" crates/raios-core/src/config.rs` first)

**Interfaces:**
- Produces: `DaemonConfig.digest_interval_secs: u64`, default `1800`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn daemon_config_default_digest_interval_is_thirty_minutes() {
    let config = DaemonConfig::default();
    assert_eq!(config.digest_interval_secs, 1800);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-core config::tests::daemon_config_default_digest_interval_is_thirty_minutes`
Expected: FAIL — compile error, no field `digest_interval_secs`

- [ ] **Step 3: Add the field**

In `DaemonConfig` (the `pub struct DaemonConfig { ... }` block), add after `scheduler_interval_secs`:
```rust
    pub enable_scheduler_worker: bool,
    pub scheduler_interval_secs: u64,
    /// How often the routine-activity digest fires (seconds). 0 = every poll.
    pub digest_interval_secs: u64,
```

In `impl Default for DaemonConfig`, add after the existing `scheduler_interval_secs` line:
```rust
            digest_interval_secs: 1800,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-core config::tests::daemon_config_default_digest_interval_is_thirty_minutes`
Expected: PASS

- [ ] **Step 5: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add crates/raios-core/src/config.rs SIGMAP.md
git commit -m "feat(core): add DaemonConfig.digest_interval_secs (default 1800)"
```

---

## Task 5: Security-whisper → important activity_event (health worker)

**Files:**
- Modify: `crates/raios-runtime/src/daemon/health.rs`
- Test: inline `#[cfg(test)] mod tests` in the same file (add one if absent — check first)

**Interfaces:**
- Consumes: `raios_core::db::log_activity_event` (Task 2), `raios_core::db::open_db` (existing).

- [ ] **Step 1: Write the failing test**

`emit_security_whispers` (the existing function at `health.rs:14`) takes `project_name: &str, report: &SecurityReport, radar: &RadarChannel`. Add a sibling function this task introduces, tested directly (unit-testable without a real DB by using an in-memory connection):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use raios_core::security::{Severity, SecurityIssue, SecurityReport};
    use rusqlite::Connection;

    fn report_with(severity: Severity) -> SecurityReport {
        SecurityReport {
            issues: vec![SecurityIssue {
                severity,
                title: "hardcoded secret".to_string(),
                file: "src/main.rs".to_string(),
                owasp: "A02".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn log_security_whispers_as_activity_writes_important_row_for_critical() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap(); // see note below on this helper

        log_security_whispers_as_activity(&conn, "demo-project", &report_with(Severity::Critical));

        let (tier, project): (String, Option<String>) = conn
            .query_row(
                "SELECT tier, project FROM activity_events WHERE source = 'audit'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(tier, "important");
        assert_eq!(project.as_deref(), Some("demo-project"));
    }

    #[test]
    fn log_security_whispers_as_activity_ignores_low_and_info_severity() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        log_security_whispers_as_activity(&conn, "demo-project", &report_with(Severity::Low));

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

Before writing this, check the real field names of `SecurityIssue`/`SecurityReport` and the real `Severity` variants (`grep -n "struct SecurityIssue\|struct SecurityReport\|enum Severity" -A 10 crates/raios-core/src/security/mod.rs`) — the test above is illustrative of intent; match it exactly to the real struct definitions (field names, whether `Default` is derived, exact `Severity` variant names) before running it. The in-memory-DB-for-tests helper is the already-public `raios_core::db::migrate_existing(&conn)` (confirmed via its existing use in `crates/raios-runtime/src/control_plane/service.rs`'s own tests — `let conn = Connection::open_in_memory().unwrap(); raios_core::db::migrate_existing(&conn).unwrap();`), already used consistently throughout this plan.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime health::tests::log_security_whispers`
Expected: FAIL — `log_security_whispers_as_activity` doesn't exist yet

- [ ] **Step 3: Implement**

Add to `crates/raios-runtime/src/daemon/health.rs`, near `emit_security_whispers`:

```rust
/// Persists Critical/High security findings as `activity_events` rows
/// (tier=important) so notification clients can surface them, independent
/// of the ephemeral Radar-whisper broadcast `emit_security_whispers`
/// already does for connected agents.
pub(crate) fn log_security_whispers_as_activity(
    conn: &rusqlite::Connection,
    project_name: &str,
    report: &SecurityReport,
) {
    for issue in report
        .issues
        .iter()
        .filter(|i| matches!(i.severity, SecSev::Critical | SecSev::High))
    {
        let summary = format!("{} [{}] {}", project_name, issue.severity.label(), issue.title);
        if let Err(e) = raios_core::db::log_activity_event(
            conn,
            "audit",
            Some(project_name),
            "important",
            &summary,
            None,
        ) {
            eprintln!("[Daemon] Failed to log security activity event for {project_name}: {e}");
        }
    }
}
```

Call it right after the existing `emit_security_whispers(&proj_name_clone, &sec_report, &radar_clone);` line inside `start_health_worker`'s per-project `spawn_blocking` closure (around line 89). That closure doesn't currently hold a DB connection — open one at the top of the closure the same way `daemon/lifecycle.rs` does (`raios_core::db::open_db()`, matching its existing error-handling style: log and skip on failure, don't panic the worker):

```rust
                tokio::task::spawn_blocking(move || {
                    let report = check_project_fast(&proj);
                    let sec_report = scan_project_fast(&proj_path_clone);
                    emit_security_whispers(&proj_name_clone, &sec_report, &radar_clone);
                    if let Ok(conn) = raios_core::db::open_db() {
                        log_security_whispers_as_activity(&conn, &proj_name_clone, &sec_report);
                    }
                    let log_msg = serde_json::json!({
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-runtime health::tests`
Expected: PASS

- [ ] **Step 5: Full workspace check**

Run: `cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: clean, no regressions

- [ ] **Step 6: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add crates/raios-runtime/src/daemon/health.rs SIGMAP.md
git commit -m "feat(runtime): log Critical/High security findings as important activity_events"
```

---

## Task 6: Git-worker status change → routine activity_event

**Files:**
- Modify: `crates/raios-runtime/src/daemon/git.rs`
- Test: inline `#[cfg(test)]` (add module if absent)

**Interfaces:**
- Consumes: `raios_core::db::log_activity_event` (Task 2).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_status_change_as_activity_writes_routine_row_with_old_and_new_status() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap(); // see Task 5 note on locating the real helper name

        log_status_change_as_activity(&conn, "demo-project", "main", "main (dirty)");

        let (tier, summary): (String, String) = conn
            .query_row(
                "SELECT tier, summary FROM activity_events WHERE source = 'git'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(tier, "routine");
        assert!(summary.contains("demo-project"));
        assert!(summary.contains("main (dirty)"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime daemon::git::tests`
Expected: FAIL — function doesn't exist

- [ ] **Step 3: Implement**

Add to `crates/raios-runtime/src/daemon/git.rs`:

```rust
fn log_status_change_as_activity(
    conn: &rusqlite::Connection,
    project_name: &str,
    old_status: &str,
    new_status: &str,
) {
    let summary = format!("{project_name}: {old_status} → {new_status}");
    if let Err(e) =
        raios_core::db::log_activity_event(conn, "git", Some(project_name), "routine", &summary, None)
    {
        eprintln!("[Daemon] Failed to log git activity event for {project_name}: {e}");
    }
}
```

Wire it into the existing status-change detection at the block:
```rust
                    if proj.status != new_status {
                        proj.status = new_status;
                        updated = true;
                    }
```
This runs inside `let mut s = state.write().await;` where a DB connection isn't currently open. Open one once per loop iteration (not per-project, to avoid a connection-per-project churn) — add it right before the `let mut updated = false;` line at the top of the loop body, matching `lifecycle.rs`'s existing pattern of opening one connection per tick:

```rust
        let conn = raios_core::db::open_db().ok();
        let mut updated = false;
        {
            let mut s = state.write().await;
            for proj in s.projects.iter_mut() {
                if proj.local_path.join(".git").exists() {
                    // ... existing branch/dirty detection ...

                    if proj.status != new_status {
                        if let Some(ref conn) = conn {
                            log_status_change_as_activity(conn, &proj.name, &proj.status, &new_status);
                        }
                        proj.status = new_status;
                        updated = true;
                    }
```

(`conn: Option<Connection>` rather than propagating an `Err` up through the loop — matches the worker's existing "best-effort, never panic the loop" style seen in `lifecycle.rs`'s `match raios_core::db::open_db() { Ok(c) => c, Err(e) => { eprintln!(...); sleep(interval).await; continue; } }`, simplified here since a missing activity-log connection shouldn't skip the actual git-status update.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime daemon::git::tests`
Expected: PASS

- [ ] **Step 5: Full workspace check and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-runtime/src/daemon/git.rs SIGMAP.md
git commit -m "feat(runtime): log git status changes as routine activity_events"
```

---

## Task 7: Lifecycle transition → important activity_event

**Files:**
- Modify: `crates/raios-runtime/src/daemon/lifecycle.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `raios_core::db::log_activity_event` (Task 2).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_transition_as_activity_writes_important_row() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        log_transition_as_activity(&conn, "demo-project", "active", "beklemede");

        let (tier, project, summary): (String, Option<String>, String) = conn
            .query_row(
                "SELECT tier, project, summary FROM activity_events WHERE source = 'lifecycle'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(tier, "important");
        assert_eq!(project.as_deref(), Some("demo-project"));
        assert!(summary.contains("active"));
        assert!(summary.contains("beklemede"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime daemon::lifecycle::tests`
Expected: FAIL — function doesn't exist

- [ ] **Step 3: Implement**

Add to `crates/raios-runtime/src/daemon/lifecycle.rs`:

```rust
fn log_transition_as_activity(
    conn: &rusqlite::Connection,
    project_name: &str,
    old_status: &str,
    new_status: &str,
) {
    let summary = format!("{project_name}: {old_status} → {new_status}");
    if let Err(e) = raios_core::db::log_activity_event(
        conn,
        "lifecycle",
        Some(project_name),
        "important",
        &summary,
        None,
    ) {
        eprintln!("[Lifecycle] Failed to log activity event for {project_name}: {e}");
    }
}
```

Call it in the existing transition block (the `conn` variable is already in scope here — this loop already opens one at the top):
```rust
            if let Some(status) = new_status {
                if let Err(e) = raios_core::db::update_project_status(&conn, &path_str, status) {
                    eprintln!("[Lifecycle] Failed to update {}: {e}", proj.name);
                } else {
                    log_transition_as_activity(&conn, &proj.name, current, status);
                    println!(
                        "[Lifecycle] {} → {} (age: {}d)",
                        proj.name,
                        status,
                        age_secs / 86_400
                    );
                    updated = true;
                }
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime daemon::lifecycle::tests`
Expected: PASS

- [ ] **Step 5: Full workspace check and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-runtime/src/daemon/lifecycle.rs SIGMAP.md
git commit -m "feat(runtime): log lifecycle status transitions as important activity_events"
```

---

## Task 8: Scheduler fire-success (routine) and overdue (important)

**Files:**
- Modify: `crates/raios-runtime/src/daemon/scheduler.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `raios_core::db::log_activity_event` (Task 2), `raios_core::db::ScheduledJob` (existing, `crates/raios-core/src/db/scheduler.rs`).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use raios_core::db::ScheduledJob;
    use rusqlite::Connection;

    fn job_due_at(next_run_at: &str, interval_secs: i64) -> ScheduledJob {
        ScheduledJob {
            id: "job-1".into(),
            title: "JSON Backup".into(),
            agent: "claude".into(),
            task_description: "backup".into(),
            project_id: None,
            interval_secs,
            status: "active".into(),
            last_run_at: None,
            next_run_at: next_run_at.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            run_count: 0,
        }
    }

    #[test]
    fn log_fire_success_as_activity_writes_routine_row() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        log_fire_success_as_activity(&conn, &job_due_at("2026-08-17T12:00:00Z", 3600));

        let tier: String = conn
            .query_row("SELECT tier FROM activity_events WHERE source = 'scheduler'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tier, "routine");
    }

    #[test]
    fn overdue_important_event_fires_when_job_is_more_than_2x_interval_late() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s (1h), next_run_at was due 3 hours ago -> overdue by 3x interval
        let three_hours_ago = (chrono::Utc::now() - chrono::Duration::hours(3))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&three_hours_ago, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let tier: String = conn
            .query_row("SELECT tier FROM activity_events WHERE source = 'scheduler'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tier, "important");
    }

    #[test]
    fn overdue_important_event_does_not_fire_when_job_is_only_slightly_late() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        // interval=3600s, due 5 minutes ago -> not overdue by the 2x threshold
        let five_min_ago = (chrono::Utc::now() - chrono::Duration::minutes(5))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let job = job_due_at(&five_min_ago, 3600);

        log_overdue_activity_if_needed(&conn, &job);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime daemon::scheduler::tests`
Expected: FAIL — functions don't exist

- [ ] **Step 3: Implement**

Add to `crates/raios-runtime/src/daemon/scheduler.rs`:

```rust
fn log_fire_success_as_activity(conn: &rusqlite::Connection, job: &raios_core::db::ScheduledJob) {
    let summary = format!("'{}' fired ({})", job.title, job.agent);
    if let Err(e) = raios_core::db::log_activity_event(conn, "scheduler", None, "routine", &summary, None) {
        eprintln!("[Scheduler] Failed to log fire-success activity event: {e}");
    }
}

fn log_overdue_activity_if_needed(conn: &rusqlite::Connection, job: &raios_core::db::ScheduledJob) {
    let Ok(due) = chrono::DateTime::parse_from_str(
        &format!("{} +0000", job.next_run_at),
        "%Y-%m-%dT%H:%M:%SZ %z",
    ) else {
        return;
    };
    let overdue_secs = (chrono::Utc::now() - due.with_timezone(&chrono::Utc)).num_seconds();

    if overdue_secs > 2 * job.interval_secs {
        let summary = format!(
            "'{}' is overdue by {}m (interval {}m)",
            job.title,
            overdue_secs / 60,
            job.interval_secs / 60
        );
        if let Err(e) =
            raios_core::db::log_activity_event(conn, "scheduler", None, "important", &summary, None)
        {
            eprintln!("[Scheduler] Failed to log overdue activity event: {e}");
        }
    }
}
```

Wire both into the existing loop. `log_overdue_activity_if_needed` runs for every claimed job, right after `for job in jobs {` (before the existing prompt-building/spawn logic — it's a passive read-only check on data already available on `job`, doesn't affect firing):

```rust
        for job in jobs {
            log_overdue_activity_if_needed(&conn, &job);

            let prompt = format!(
```

`log_fire_success_as_activity` goes in the existing `Ok(Ok(pid)) => { ... }` success branch, after the existing `cp_scheduled_job_mark_fired` call:

```rust
                match spawn_result {
                    Ok(Ok(pid)) => {
                        let _ =
                            raios_core::db::cp_scheduled_job_mark_fired(&conn, &job_id, interval);
                        log_fire_success_as_activity(&conn, &job);
                        let evt = serde_json::json!({
```

(`job` needs to still be in scope / cloneable into the inner `tokio::spawn` closure for this — check the closure's captured variables via `grep -n "let job_id = job.id.clone();" -A 10 crates/raios-runtime/src/daemon/scheduler.rs`; if `job` itself isn't already moved into the closure, either clone it alongside `job_id`/`interval`/etc. the same way, or reconstruct the fields already captured (`job.title`, `job.agent` are already cloned as `job_id`/`agent` — add `let job_title = job.title.clone();` next to them and use that instead of the full struct if `job` itself isn't available inside the closure).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-runtime daemon::scheduler::tests`
Expected: PASS

- [ ] **Step 5: Full workspace check and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-runtime/src/daemon/scheduler.rs SIGMAP.md
git commit -m "feat(runtime): log scheduler fires (routine) and overdue jobs (important) as activity_events"
```

---

## Task 9: Agent-run completion → routine activity_event

**Files:**
- Modify: `crates/raios-core/src/db/wf_sessions.rs`
- Test: inline, next to any existing tests in that file (check first: `grep -n "mod tests" crates/raios-core/src/db/wf_sessions.rs`)

**Interfaces:**
- Consumes: `log_activity_event` (Task 2, same crate — no cross-crate import needed, just call the sibling module directly: `super::activity_events::log_activity_event` or via the `pub use` re-export at `crate::db::log_activity_event`).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn cp_session_end_with_summary_logs_a_routine_activity_event_on_success() {
    let conn = /* whatever this file's existing tests use to get a migrated in-memory Connection — check the existing test module first */;

    // Set up a minimal task/run row this function can update — follow the
    // exact fixture pattern already used by this file's other tests for
    // cp_session_end / cp_session_end_with_summary (check
    // `grep -n "fn.*test" crates/raios-core/src/db/wf_sessions.rs` for an
    // existing test to copy the task/run creation boilerplate from).

    cp_session_end_with_summary(&conn, "task-1", "run-1", true, Some("committed 3 files")).unwrap();

    let (tier, summary): (String, String) = conn
        .query_row(
            "SELECT tier, summary FROM activity_events WHERE source = 'agent_run'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tier, "routine");
    assert!(summary.contains("committed 3 files"));
}

#[test]
fn cp_session_end_with_summary_does_not_log_activity_when_summary_is_empty() {
    let conn = /* same fixture setup as above */;

    cp_session_end_with_summary(&conn, "task-1", "run-1", true, None).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

Before finalizing this test, read the full existing test module in `crates/raios-core/src/db/wf_sessions.rs` (`cat crates/raios-core/src/db/wf_sessions.rs`) to copy its exact fixture-setup helper (how it creates a valid `task_id`/`run_id` pair before calling `cp_session_end*`) rather than guessing — the placeholder comments above must be replaced with real code from that file.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-core db::wf_sessions::tests::cp_session_end_with_summary_logs`
Expected: FAIL — no `activity_events` row gets written yet

- [ ] **Step 3: Implement**

In `cp_session_end_with_summary` (`crates/raios-core/src/db/wf_sessions.rs:57`), after the existing status-update logic succeeds and only when `success` is true and `summary` is non-empty, call `log_activity_event`. Read the full function body first (it's already partially shown in this plan's research — re-read the live file, since the exact tail of the function wasn't captured verbatim) and add the call right before the function's final `Ok(())`:

```rust
    if success {
        if let Some(text) = summary.filter(|s| !s.is_empty()) {
            let event_summary = format!("agent run completed: {text}");
            let _ = crate::db::log_activity_event(conn, "agent_run", None, "routine", &event_summary, None);
        }
    }

    Ok(())
```

(Best-effort `let _ =` here matches this task's low-stakes nature — a missing notification-log row must never fail the actual session-completion write, which is the function's real job.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-core db::wf_sessions::tests`
Expected: PASS, including all pre-existing tests in this file (no regressions)

- [ ] **Step 5: Full workspace check and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-core/src/db/wf_sessions.rs SIGMAP.md
git commit -m "feat(core): log successful agent-run completions as routine activity_events"
```

---

## Task 10: Retention pruning

**Files:**
- Modify: `crates/raios-runtime/src/daemon/lifecycle.rs`

**Interfaces:**
- Consumes: `raios_core::db::prune_activity_events_older_than` (Task 2).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn lifecycle_tick_prunes_activity_events_older_than_30_days() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    conn.execute(
        "INSERT INTO activity_events (ts, source, project, tier, summary)
         VALUES ('2020-01-01T00:00:00Z', 'git', 'old', 'routine', 'stale')",
        [],
    )
    .unwrap();

    prune_stale_activity_events(&conn);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p raios-runtime daemon::lifecycle::tests::lifecycle_tick_prunes`
Expected: FAIL — function doesn't exist

- [ ] **Step 3: Implement**

Add to `crates/raios-runtime/src/daemon/lifecycle.rs`:

```rust
const ACTIVITY_EVENT_RETENTION_DAYS: i64 = 30;

fn prune_stale_activity_events(conn: &rusqlite::Connection) {
    match raios_core::db::prune_activity_events_older_than(conn, ACTIVITY_EVENT_RETENTION_DAYS) {
        Ok(0) => {}
        Ok(n) => println!("[Lifecycle] Pruned {n} stale activity_events row(s)."),
        Err(e) => eprintln!("[Lifecycle] activity_events prune failed: {e}"),
    }
}
```

Call it once per lifecycle tick, right after the existing `let conn = match raios_core::db::open_db() { ... };` block succeeds (this worker already runs on an hourly-scale `interval`, matching the 30-day retention window's granularity — no new timer needed):

```rust
        let conn = match raios_core::db::open_db() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Lifecycle] DB open failed: {e}");
                sleep(interval).await;
                continue;
            }
        };

        prune_stale_activity_events(&conn);

        let mut updated = false;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p raios-runtime daemon::lifecycle::tests`
Expected: PASS (all tests in this file, no regressions from Task 7's earlier addition)

- [ ] **Step 5: Full workspace check and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-runtime/src/daemon/lifecycle.rs SIGMAP.md
git commit -m "feat(runtime): prune activity_events older than 30 days on each lifecycle tick"
```

---

## Task 11: `/api/notifications/important` and `/api/notifications/digest` endpoints

**Files:**
- Modify: `crates/raios-runtime/src/server/http/routes.rs`
- Modify: `crates/raios-runtime/src/server/http/mod.rs` (route registration)
- Test: integration test in `crates/raios-runtime/tests/` (check the existing test directory structure first — `ls crates/raios-runtime/tests/` — and match its existing style for spinning up a test server/DB, e.g. the pattern used by whatever integration test already covers `/api/health` or similar, if one exists; otherwise a `#[cfg(test)]` unit test directly against the handler functions is acceptable, calling them with a hand-built `AppState`)

**Interfaces:**
- Consumes: `raios_core::db::{poll_important_events, poll_digest_window}` (Task 2), `raios_runtime::reflect_scoring::{snapshot, build_recommendations}` (Task 3), `raios_core::entities::discover_entities` (existing), `raios_core::config::Config` (existing).

- [ ] **Step 1: Write the failing tests**

```rust
#[derive(Deserialize)]
pub(super) struct ClientIdQuery {
    client_id: String,
}

pub(super) async fn handle_notifications_important(
    Query(params): Query<ClientIdQuery>,
) -> impl IntoResponse {
    match raios_core::db::open_db() {
        Ok(conn) => match raios_core::db::poll_important_events(&conn, &params.client_id) {
            Ok(events) => Json(json!({ "status": "ok", "events": events })),
            Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
        },
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

pub(super) async fn handle_notifications_digest(
    Query(params): Query<ClientIdQuery>,
) -> impl IntoResponse {
    let config =
        Config::load().unwrap_or_else(|| Config::from_detect_result(Config::auto_detect()));

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => return Json(json!({ "status": "error", "message": e.to_string() })),
    };

    let window = match raios_core::db::poll_digest_window(
        &conn,
        &params.client_id,
        config.daemon.digest_interval_secs as i64,
    ) {
        Ok(w) => w,
        Err(e) => return Json(json!({ "status": "error", "message": e.to_string() })),
    };

    let Some(window) = window else {
        return Json(json!({ "status": "ok", "digest": null }));
    };

    let projects = raios_core::entities::discover_entities(&config.dev_ops_path);
    let snapshots: Vec<_> = projects.iter().map(raios_runtime_reflect_scoring_snapshot).collect();
    let top_recommendation = raios_runtime::reflect_scoring::build_recommendations(&snapshots)
        .into_iter()
        .next();

    let summary = build_digest_summary(&window.events);

    Json(json!({
        "status": "ok",
        "digest": {
            "since_ts": window.since_ts,
            "until_ts": window.until_ts,
            "summary": summary,
            "top_recommendation": top_recommendation,
            "event_count": window.events.len(),
        }
    }))
}

fn build_digest_summary(events: &[raios_core::db::ActivityEvent]) -> String {
    if events.is_empty() {
        return "No background activity.".to_string();
    }

    let mut by_source: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for e in events {
        *by_source.entry(e.source.as_str()).or_insert(0) += 1;
    }

    let order = ["git", "health", "scheduler", "agent_run"];
    let clauses: Vec<String> = order
        .iter()
        .filter_map(|source| {
            by_source.get(source).map(|count| match *source {
                "git" => format!("{count} git status change(s)"),
                "health" => format!("{count} health scan update(s)"),
                "scheduler" => format!("{count} scheduled job(s) ran"),
                "agent_run" => format!("{count} agent run(s) completed"),
                _ => unreachable!(),
            })
        })
        .collect();

    clauses.join("; ")
}
```

The `raios_runtime_reflect_scoring_snapshot` placeholder name above is wrong on purpose — fix it before implementing: `reflect_scoring::snapshot` takes `&EntityProject` and this file already has `raios_core::entities::EntityProject` in scope elsewhere, so the real call is just `raios_runtime::reflect_scoring::snapshot(p)` inside the `.map(...)` closure — write `projects.iter().map(|p| raios_runtime::reflect_scoring::snapshot(p)).collect()`.

Now the actual tests:

```rust
#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn build_digest_summary_returns_quiet_message_when_no_events() {
        assert_eq!(build_digest_summary(&[]), "No background activity.");
    }

    #[test]
    fn build_digest_summary_groups_by_source_in_fixed_order() {
        let events = vec![
            raios_core::db::ActivityEvent {
                ts: "t1".into(),
                source: "scheduler".into(),
                project: None,
                summary: "x".into(),
            },
            raios_core::db::ActivityEvent {
                ts: "t2".into(),
                source: "git".into(),
                project: Some("a".into()),
                summary: "y".into(),
            },
        ];
        let summary = build_digest_summary(&events);
        // git must appear before scheduler, matching the fixed `order` array
        let git_pos = summary.find("git").unwrap();
        let sched_pos = summary.find("scheduled").unwrap();
        assert!(git_pos < sched_pos);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime server::http::routes::notification_tests`
Expected: FAIL — compile error, functions/module don't exist yet in `routes.rs`

- [ ] **Step 3: Add the handlers to `routes.rs`**

Append everything from Step 1 (minus the illustrative placeholder line) to `crates/raios-runtime/src/server/http/routes.rs`. Add `use raios_core::config::Config;` if not already imported (it already is, per the file's existing top-of-file imports seen during research — verify with `grep -n "^use raios_core::config::Config;" crates/raios-runtime/src/server/http/routes.rs` first, don't duplicate the import if present).

- [ ] **Step 4: Register the routes**

In `crates/raios-runtime/src/server/http/mod.rs`, add to the `Router::new()` chain, alongside the existing `.route("/api/usage", get(handle_usage))` line:

```rust
        .route("/api/notifications/important", get(handle_notifications_important))
        .route("/api/notifications/digest", get(handle_notifications_digest))
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p raios-runtime server::http::routes`
Expected: PASS

- [ ] **Step 6: Live-verify against the running daemon**

```bash
cargo build --release --workspace --bins
bash install.sh
TOKEN=$(cat ~/.config/raios/.session_token)
curl -s -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:42071/api/notifications/important?client_id=plan-verify" | head -c 500
curl -s -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:42071/api/notifications/digest?client_id=plan-verify" | head -c 500
```
Expected: both return `{"status":"ok", ...}` JSON, not a 401 or 500. (Check `auth.rs` for the exact header/token-file convention first if this 401s — `grep -n "Authorization\|Bearer" crates/raios-runtime/src/server/http/auth.rs`.)

- [ ] **Step 7: Full workspace check, regenerate SIGMAP.md, and commit**

```bash
cargo test --workspace --lib && cargo clippy --workspace --all-targets --all-features -- -D warnings
sigmap
git add crates/raios-runtime/src/server/http/routes.rs crates/raios-runtime/src/server/http/mod.rs SIGMAP.md
git commit -m "feat(runtime): add /api/notifications/important and /api/notifications/digest"
```

---

## Task 12: `raios-tray.py` client integration

**Files:**
- Modify: `tools/raios-tray/raios-tray.py`

**Interfaces:**
- Consumes: `/api/notifications/important?client_id=raios-tray`, `/api/notifications/digest?client_id=raios-tray` (Task 11), the existing `fetch_state`-style REST helper and `self._notify()` (both already present in this file).

- [ ] **Step 1: Locate the existing REST fetch helper**

Read the full existing HTTP-request helper this file already uses (the one `fetch_state()` calls, referenced near line 259 per this file's earlier `API_BASE + path` usage) — `grep -n "def fetch_state\|def _http_get\|API_BASE + path" tools/raios-tray/raios-tray.py` — and match its exact signature/error handling for the two new calls below, rather than inventing a second HTTP pattern.

- [ ] **Step 2: Add the notification-polling methods**

In the `RaiosTray` class, add (matching whatever the real helper from Step 1 is named — the sketch below assumes a synchronous `_http_get_json(path: str) -> dict | None` exists; adjust to the real helper's actual name/signature):

```python
CLIENT_ID = "raios-tray"

    def _check_important_notifications(self) -> None:
        data = self._http_get_json(f"/api/notifications/important?client_id={CLIENT_ID}")
        if not data or data.get("status") != "ok":
            return
        for event in data.get("events", []):
            self._notify(event.get("summary", ""))

    def _check_digest_notification(self) -> None:
        data = self._http_get_json(f"/api/notifications/digest?client_id={CLIENT_ID}")
        if not data or data.get("status") != "ok":
            return
        digest = data.get("digest")
        if not digest:
            return
        message = digest.get("summary", "")
        rec = digest.get("top_recommendation")
        if rec:
            message += f"\nTop recommendation: {rec}"
        self._notify(message)
```

- [ ] **Step 3: Wire into `refresh()` and a new digest timer**

In `refresh()` (the existing method, ~line 1609), add a call to `self._check_important_notifications()` — it already runs on the existing 15s `refresh_timer`, satisfying the "near-instant" requirement without a new timer.

In `__init__` (~line 1577, right after the existing `self.refresh_timer` setup), add a second timer for the digest:

```python
        self.digest_timer = QTimer(self)
        self.digest_timer.setInterval(60 * 1000)  # poll every 60s; server gates by digest_interval_secs
        self.digest_timer.timeout.connect(self._check_digest_notification)
        self.digest_timer.start()
```

(The digest endpoint itself returns `null` until the server-side interval elapses — polling it every 60s client-side is cheap and just picks up the digest promptly once it's ready, rather than trying to keep client and server intervals in exact lockstep.)

- [ ] **Step 4: Manual verification (no automated test harness exists for this file, per project convention)**

```bash
python3 -m py_compile tools/raios-tray/raios-tray.py
```
Expected: no syntax errors.

Then run the tray app directly against the live daemon (`python3 tools/raios-tray/raios-tray.py &`), trigger a real lifecycle transition or scheduler overdue condition if feasible, and confirm a system tray notification appears. If not feasible to trigger a real event during this task, at minimum confirm no exceptions are thrown by watching the process's stderr for one full `digest_timer` cycle (60s+).

- [ ] **Step 5: Regenerate SIGMAP.md and commit**

```bash
sigmap
git add tools/raios-tray/raios-tray.py SIGMAP.md
git commit -m "feat(raios-tray): poll and surface background-activity notifications"
```

---

## Task 13: kaira-launcher GNOME extension client integration

**Files:**
- Modify: `code/gnome-extension/extension.js` (in the `kaira-launcher` repo, `/home/alaz/dev/tools/kaira-launcher`)
- Create (optional, if `extension.js` is already large — check its line count first with `wc -l code/gnome-extension/extension.js`; if over ~500 lines, split into a new `notifications.js` module following this file's existing pattern of one module per concern, e.g. `weatherIndicator.js`, `tailscaleIndicator.js`): `code/gnome-extension/notifications.js`

**Interfaces:**
- Consumes: `fetchRaiosJson(session, path)` and `readRaiosToken()` (both already shared module-scope helpers in `extension.js`, per this repo's 2026-07-06 `memory.md` entry — reuse them, don't duplicate).

- [ ] **Step 1: Confirm the exact existing helper signatures**

```bash
cd /home/alaz/dev/tools/kaira-launcher
grep -n "^function fetchRaiosJson\|^function readRaiosToken\|RAIOS_API_BASE" code/gnome-extension/extension.js
wc -l code/gnome-extension/extension.js
```
Use the real signatures found here for Step 2 — do not guess.

- [ ] **Step 2: Add the polling + notification functions**

```javascript
const CLIENT_ID = "kaira-gnome-ext";

function checkImportantNotifications(session) {
  fetchRaiosJson(session, `/notifications/important?client_id=${CLIENT_ID}`)
    .then((data) => {
      if (!data || data.status !== "ok") return;
      for (const event of data.events || []) {
        Main.notify("R-AI-OS", event.summary);
      }
    })
    .catch((_error) => {});
}

function checkDigestNotification(session) {
  fetchRaiosJson(session, `/notifications/digest?client_id=${CLIENT_ID}`)
    .then((data) => {
      if (!data || data.status !== "ok" || !data.digest) return;
      let message = data.digest.summary;
      if (data.digest.top_recommendation) {
        message += `\n${data.digest.top_recommendation}`;
      }
      Main.notify("R-AI-OS", message);
    })
    .catch((_error) => {});
}
```

`Main` here is GNOME Shell's `resource:///org/gnome/shell/ui/main.js` — check the existing top-of-file imports (`grep -n "^import Main\|imports.ui.main" code/gnome-extension/extension.js`) for this codebase's exact import style (GNOME Shell 45+ uses ES module `import * as Main from 'resource:///org/gnome/shell/ui/main.js';`) and match it exactly rather than guessing the import syntax.

Verify the endpoint path prefix: `fetchRaiosJson`'s `path` argument — check whether `RAIOS_API_BASE` already includes `/api` (per `RAIOS_API_BASE + path` usage seen in this file) — if so, use `/notifications/important` as above; if `RAIOS_API_BASE` is just the bare host with no `/api` prefix, use `/api/notifications/important` instead. Confirm by reading the constant's value directly (`grep -n "const RAIOS_API_BASE" code/gnome-extension/extension.js`).

- [ ] **Step 3: Wire into the extension's enable/timer lifecycle**

Find the existing timer setup pattern this extension already uses (per Task 1's own research this session: `weatherIndicator.js:196` uses `GLib.timeout_add_seconds`). In `extension.js`'s `enable()` function (or wherever the existing periodic timers for the projects/weather indicators are registered — `grep -n "GLib.timeout_add_seconds" code/gnome-extension/extension.js`), add two more timers following the exact same registration/cleanup pattern (including removing them in `disable()` — check how existing timers get torn down, e.g. `GLib.source_remove(this._pollId)` in `weatherIndicator.js`, and replicate that for these two new ones so `disable()`/`enable()` cycles don't leak timers):

```javascript
this._importantNotifId = GLib.timeout_add_seconds(
  GLib.PRIORITY_DEFAULT,
  15,
  () => {
    checkImportantNotifications(this._httpSession);
    return GLib.SOURCE_CONTINUE;
  },
);

this._digestNotifId = GLib.timeout_add_seconds(
  GLib.PRIORITY_DEFAULT,
  60,
  () => {
    checkDigestNotification(this._httpSession);
    return GLib.SOURCE_CONTINUE;
  },
);
```

(`this._httpSession` — confirm this extension already holds a shared `Soup.Session` instance somewhere reusable across indicators, per the `fetchRaiosJson(session, path)` signature requiring one; grep `new Soup.Session` to find where it's currently constructed and reuse that instance rather than creating a new one.)

- [ ] **Step 4: Manual verification**

Per this repo's own documented reload caveat (`kaira-launcher/memory.md`, 2026-07-06 entry): `gnome-extensions disable/enable` does **not** reliably reload `extension.js` changes in this GNOME Shell version — a full logout/login is required to verify. After that, confirm via `journalctl --user -b 0 | grep -i kaira-launcher` that no JS ERROR lines reference the new functions, and (if a real lifecycle/scheduler-overdue event can be triggered) confirm a GNOME notification banner appears.

- [ ] **Step 5: Regenerate SIGMAP.md (kaira-launcher repo) and commit**

```bash
cd /home/alaz/dev/tools/kaira-launcher
sigmap
git add code/gnome-extension/extension.js SIGMAP.md
# if a separate notifications.js was created per Step (optional) above, add it too
git commit -m "feat(gnome-ext): poll and surface background-activity notifications"
```

---

## Final Verification

- [ ] Full workspace test suite green: `cd /home/alaz/dev/core/R-AI-OS && cargo test --workspace --lib`
- [ ] Clippy clean: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] Release build + reinstall: `cargo build --release --workspace --bins && bash install.sh`
- [ ] Live daemon healthy after reinstall: `systemctl --user status aiosd`, `ss -ltnp | grep -E "42069|42070|42071"`
- [ ] Both new endpoints respond `200` with real data against the live daemon (repeat Task 11 Step 6's `curl` calls)
- [ ] Update `memory.md` (R-AI-OS repo) with a Change Log entry summarizing what shipped, per this repo's own established convention (see every prior entry in this file for the expected level of detail)
