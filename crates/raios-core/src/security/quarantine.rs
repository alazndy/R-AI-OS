use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ─── Config (raios-policy.toml [quarantine]) ─────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuarantineConfig {
    pub enabled: bool,
    /// Tool names that are always quarantined regardless of args.
    #[serde(default)]
    pub always_quarantine: Vec<String>,
    /// Substrings matched against args JSON; any match triggers quarantine.
    #[serde(default)]
    pub suspicious_patterns: Vec<String>,
}

// ─── Item ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct QuarantineItem {
    pub id: String,
    pub tool: String,
    pub args_json: String,
    pub created_at: String,
    pub status: String,
}

// ─── Store ────────────────────────────────────────────────────────────────────

pub struct QuarantineStore {
    config: QuarantineConfig,
}

impl QuarantineStore {
    pub fn new(config: QuarantineConfig) -> Self {
        Self { config }
    }

    pub fn disabled() -> Self {
        Self::new(QuarantineConfig::default())
    }

    pub fn from_policy(config: Option<QuarantineConfig>) -> Self {
        match config {
            Some(c) => Self::new(c),
            None => Self::disabled(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    fn is_suspicious(&self, tool: &str, args_json: &str) -> bool {
        if self.config.always_quarantine.iter().any(|t| t == tool) {
            return true;
        }
        self.config
            .suspicious_patterns
            .iter()
            .any(|p| args_json.contains(p.as_str()) || tool.contains(p.as_str()))
    }

    /// Check whether a tool call should be quarantined.
    ///
    /// - Returns `Ok(())` if allowed (not suspicious, or already approved).
    /// - Returns `Err(QuarantineError::Queued(id))` if newly queued.
    /// - Returns `Err(QuarantineError::Denied(id))` if a denied entry exists.
    pub fn check(
        &self,
        conn: &Connection,
        tool: &str,
        args_json: &str,
    ) -> Result<(), QuarantineError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check for an existing approved token for this tool → consume and allow.
        if let Some(_id) = consume_approved(conn, tool) {
            return Ok(());
        }

        // Check for an existing denied entry for this tool.
        if let Some(id) = find_denied(conn, tool) {
            return Err(QuarantineError::Denied(id));
        }

        if self.is_suspicious(tool, args_json) {
            let id = new_id();
            insert_item(conn, &id, tool, args_json)?;
            return Err(QuarantineError::Queued(id));
        }

        Ok(())
    }
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum QuarantineError {
    Queued(String),
    Denied(String),
    Db(rusqlite::Error),
}

impl std::fmt::Display for QuarantineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued(id) => write!(
                f,
                "quarantine: tool call queued for human approval (id: {id}). \
                 Run `raios quarantine approve {id}` then retry the tool call."
            ),
            Self::Denied(id) => write!(
                f,
                "quarantine: tool call denied by human (id: {id}). \
                 Run `raios quarantine deny {id} --clear` to remove the block."
            ),
            Self::Db(e) => write!(f, "quarantine: db error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for QuarantineError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

// ─── DB helpers ──────────────────────────────────────────────────────────────

pub fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS quarantine_queue (
            id         TEXT PRIMARY KEY,
            tool       TEXT NOT NULL,
            args_json  TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            status     TEXT NOT NULL DEFAULT 'pending'
        );
        CREATE INDEX IF NOT EXISTS idx_quarantine_tool ON quarantine_queue(tool, status);",
    )
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs:x}{nanos:08x}")
}

fn insert_item(
    conn: &Connection,
    id: &str,
    tool: &str,
    args_json: &str,
) -> Result<(), QuarantineError> {
    conn.execute(
        "INSERT OR IGNORE INTO quarantine_queue (id, tool, args_json) VALUES (?1,?2,?3)",
        params![id, tool, args_json],
    )?;
    Ok(())
}

fn consume_approved(conn: &Connection, tool: &str) -> Option<String> {
    let id: String = conn
        .query_row(
            "SELECT id FROM quarantine_queue WHERE tool=?1 AND status='approved' LIMIT 1",
            params![tool],
            |r| r.get(0),
        )
        .ok()?;
    let _ = conn.execute(
        "UPDATE quarantine_queue SET status='consumed' WHERE id=?1",
        params![&id],
    );
    Some(id)
}

fn find_denied(conn: &Connection, tool: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM quarantine_queue WHERE tool=?1 AND status='denied' LIMIT 1",
        params![tool],
        |r| r.get(0),
    )
    .ok()
}

// ─── Public queue management ─────────────────────────────────────────────────

pub fn list_pending(conn: &Connection) -> rusqlite::Result<Vec<QuarantineItem>> {
    let mut stmt = conn.prepare(
        "SELECT id,tool,args_json,created_at,status FROM quarantine_queue
         WHERE status='pending' ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(QuarantineItem {
            id: r.get(0)?,
            tool: r.get(1)?,
            args_json: r.get(2)?,
            created_at: r.get(3)?,
            status: r.get(4)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

pub fn approve(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE quarantine_queue SET status='approved' WHERE id=?1 AND status='pending'",
        params![id],
    )?;
    Ok(n > 0)
}

pub fn deny(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE quarantine_queue SET status='denied' WHERE id=?1 AND status IN ('pending','approved')",
        params![id],
    )?;
    Ok(n > 0)
}

pub fn clear(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("DELETE FROM quarantine_queue WHERE id=?1", params![id])?;
    Ok(n > 0)
}

pub fn list_all(conn: &Connection) -> rusqlite::Result<Vec<QuarantineItem>> {
    let mut stmt = conn.prepare(
        "SELECT id,tool,args_json,created_at,status FROM quarantine_queue ORDER BY created_at DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(QuarantineItem {
            id: r.get(0)?,
            tool: r.get(1)?,
            args_json: r.get(2)?,
            created_at: r.get(3)?,
            status: r.get(4)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    fn store(always: &[&str], patterns: &[&str]) -> QuarantineStore {
        QuarantineStore::new(QuarantineConfig {
            enabled: true,
            always_quarantine: always.iter().map(|s| s.to_string()).collect(),
            suspicious_patterns: patterns.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn disabled_store_allows_everything() {
        let conn = mem_conn();
        let s = QuarantineStore::disabled();
        assert!(s.check(&conn, "git_commit", "{}").is_ok());
        assert!(s.check(&conn, "evil_tool", r#"{"args":"--force"}"#).is_ok());
    }

    #[test]
    fn always_quarantine_blocks_and_queues() {
        let conn = mem_conn();
        let s = store(&["git_commit"], &[]);
        let err = s.check(&conn, "git_commit", "{}").unwrap_err();
        assert!(matches!(err, QuarantineError::Queued(_)));
        assert!(err.to_string().starts_with("quarantine:"));
    }

    #[test]
    fn suspicious_pattern_in_args_blocks() {
        let conn = mem_conn();
        let s = store(&[], &["--force"]);
        let err = s
            .check(&conn, "run_build", r#"{"flags":"--force"}"#)
            .unwrap_err();
        assert!(matches!(err, QuarantineError::Queued(_)));
    }

    #[test]
    fn non_suspicious_call_passes() {
        let conn = mem_conn();
        let s = store(&["git_commit"], &["--force"]);
        assert!(s.check(&conn, "list_projects", "{}").is_ok());
    }

    #[test]
    fn approve_then_retry_succeeds() {
        let conn = mem_conn();
        let s = store(&["git_commit"], &[]);
        let QuarantineError::Queued(id) = s.check(&conn, "git_commit", "{}").unwrap_err() else {
            panic!("expected Queued")
        };
        approve(&conn, &id).unwrap();
        assert!(s.check(&conn, "git_commit", "{}").is_ok());
    }

    #[test]
    fn deny_blocks_future_calls() {
        let conn = mem_conn();
        let s = store(&["run_build"], &[]);
        let QuarantineError::Queued(id) = s.check(&conn, "run_build", "{}").unwrap_err() else {
            panic!("expected Queued")
        };
        deny(&conn, &id).unwrap();
        let err = s.check(&conn, "run_build", "{}").unwrap_err();
        assert!(matches!(err, QuarantineError::Denied(_)));
    }

    #[test]
    fn list_pending_returns_queued_items() {
        let conn = mem_conn();
        let s = store(&["git_commit"], &[]);
        s.check(&conn, "git_commit", "{}").unwrap_err();
        let items = list_pending(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tool, "git_commit");
        assert_eq!(items[0].status, "pending");
    }
}
