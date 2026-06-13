//! Evolution Worker — learns from Factory job outcomes.
//!
//! Subscribes to the daemon broadcast channel, extracts learning signals from
//! JobComplete and JobFailed events, and persists instinct candidates to SQLite.
//! Candidates expire after 7 days unless promoted to the active InstinctEngine.
use rusqlite::{params, Connection};
use std::path::PathBuf;
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
        let Ok(conn) = self.connect() else {
            return vec![];
        };
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
        let Ok(conn) = self.connect() else {
            return 0;
        };
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
