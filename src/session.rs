use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

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
                    id TEXT PRIMARY KEY, agent TEXT NOT NULL, project TEXT,
                    started_at TEXT NOT NULL, ended_at TEXT, summary TEXT
                );
                CREATE TABLE IF NOT EXISTS session_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    event_type TEXT NOT NULL,
                    data TEXT NOT NULL DEFAULT '',
                    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_se_session ON session_events(session_id);",
            );
        }
    }

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

    pub fn end(&self, id: &str, summary: Option<&str>) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "UPDATE sessions SET ended_at=?2, summary=?3 WHERE id=?1",
                params![id, now, summary],
            );
        }
    }

    pub fn record_event(&self, session_id: &str, event_type: &str, data: &str) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(conn) = self.connect() {
            let _ = conn.execute(
                "INSERT INTO session_events (session_id, event_type, data, timestamp) VALUES (?1,?2,?3,?4)",
                params![session_id, event_type, data, now],
            );
        }
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id, agent, project, started_at, ended_at, summary FROM sessions WHERE id=?1",
            params![id],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    agent: row.get(1)?,
                    project: row.get(2)?,
                    started_at: row.get(3)?,
                    ended_at: row.get(4)?,
                    summary: row.get(5)?,
                })
            },
        )
        .ok()
    }

    pub fn events(&self, session_id: &str) -> Vec<SessionEvent> {
        let Ok(conn) = self.connect() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, event_type, data, timestamp \
                 FROM session_events WHERE session_id=?1 ORDER BY id",
            )
            .ok();
        match &mut stmt {
            Some(s) => s
                .query_map(params![session_id], |row| {
                    Ok(SessionEvent {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        event_type: row.get(2)?,
                        data: row.get(3)?,
                        timestamp: row.get(4)?,
                    })
                })
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn current_open(&self) -> Option<Session> {
        let conn = self.connect().ok()?;
        conn.query_row(
            "SELECT id, agent, project, started_at, ended_at, summary \
             FROM sessions WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
            [],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    agent: row.get(1)?,
                    project: row.get(2)?,
                    started_at: row.get(3)?,
                    ended_at: row.get(4)?,
                    summary: row.get(5)?,
                })
            },
        )
        .ok()
    }

    pub fn recent(&self, limit: usize) -> Vec<Session> {
        let Ok(conn) = self.connect() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, agent, project, started_at, ended_at, summary \
                 FROM sessions WHERE ended_at IS NOT NULL \
                 ORDER BY ended_at DESC LIMIT ?1",
            )
            .ok();
        match &mut stmt {
            Some(s) => s
                .query_map(params![limit as i64], |row| {
                    Ok(Session {
                        id: row.get(0)?,
                        agent: row.get(1)?,
                        project: row.get(2)?,
                        started_at: row.get(3)?,
                        ended_at: row.get(4)?,
                        summary: row.get(5)?,
                    })
                })
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            None => vec![],
        }
    }

    pub fn append_to_memory(&self, session_id: &str, memory_path: &Path) {
        let Some(sess) = self.get(session_id) else {
            return;
        };
        let summary = sess.summary.as_deref().unwrap_or("(no summary)");
        let ended = sess.ended_at.as_deref().unwrap_or("?");
        let proj = sess
            .project
            .as_deref()
            .map(|p| format!("[{}] ", p))
            .unwrap_or_default();
        let line = format!(
            "\n- **{}** `{}` {}— {}\n",
            ended, sess.agent, proj, summary
        );
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(memory_path)
            .map(|mut f| {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            });
    }
}

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
