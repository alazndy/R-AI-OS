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
            if !has_rule_schema(&conn) {
                let _ = conn.execute(
                    "INSERT INTO instinct_candidates
                        (project_name, command, outcome, suggestion, status)
                     SELECT 'global', ?1, ?2, ?3, 'pending'
                     WHERE NOT EXISTS (
                        SELECT 1 FROM instinct_candidates WHERE suggestion = ?3
                     )",
                    params![source, format!("confidence={confidence:.2}"), rule],
                );
                return;
            }
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
        if !has_rule_schema(&conn) {
            let mut stmt = conn
                .prepare(
                    "SELECT suggestion FROM instinct_candidates
                     WHERE status = 'pending'
                     ORDER BY created_at DESC LIMIT ?1",
                )
                .ok();
            return match &mut stmt {
                Some(s) => s
                    .query_map(params![limit as i64], |row| row.get(0))
                    .ok()
                    .map(|r| r.flatten().collect())
                    .unwrap_or_default(),
                None => vec![],
            };
        }
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
            if !has_rule_schema(&conn) {
                let _ = conn.execute(
                    "UPDATE instinct_candidates SET status = 'promoted' WHERE suggestion = ?1",
                    params![rule],
                );
                return;
            }
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
        if !has_rule_schema(&conn) {
            return 0;
        }
        conn.execute(
            "DELETE FROM instinct_candidates WHERE expires_at <= datetime('now')",
            [],
        )
        .unwrap_or(0)
    }
}

fn has_rule_schema(conn: &Connection) -> bool {
    conn.prepare("SELECT rule, promoted, expires_at FROM instinct_candidates LIMIT 0")
        .is_ok()
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

pub fn suggest_from_trace(trace: &raios_core::db::ToolTraceRow) -> Option<String> {
    if trace.redacted || !trace.success {
        return None;
    }
    let fix = trace.fix_summary.trim();
    if fix.is_empty() {
        return None;
    }
    let trigger = trace
        .error_summary
        .trim()
        .lines()
        .next()
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| trace.command.trim());
    if trigger.is_empty() {
        return None;
    }
    Some(format!(
        "When `{}` appears in `{}`, try: {}",
        compact_rule_part(trigger, 96),
        compact_rule_part(&trace.project, 48),
        compact_rule_part(fix, 160)
    ))
}

pub fn import_trace_candidates(
    conn: &Connection,
    store: &CandidateStore,
    project: Option<&str>,
    limit: usize,
) -> rusqlite::Result<usize> {
    let rows = raios_core::db::tool_trace_search(
        conn,
        raios_core::db::ToolTraceQuery {
            text: "",
            project,
            preferred_project: project,
            success_only: true,
            tag: None,
            limit,
        },
    )?;
    let mut inserted = 0usize;
    for row in rows {
        let Some(rule) = suggest_from_trace(&row) else {
            continue;
        };
        let before = store.list_pending(usize::MAX).len();
        store.insert(&rule, &format!("trace:{}", row.id), 0.72);
        let after = store.list_pending(usize::MAX).len();
        if after > before {
            inserted += 1;
        }
    }
    Ok(inserted)
}

fn compact_rule_part(value: &str, max_chars: usize) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut out: String = flat.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ─── Approval streak policy promotion ─────────────────────────────────────────

static APPROVAL_STREAKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(String, String), usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Record an approval or denial decision for a tool call in a project.
/// Returns true if N consecutive approvals were reached and a policy promotion candidate was inserted.
pub fn record_approval_decision(
    store: &CandidateStore,
    tool_name: &str,
    project: &str,
    approved: bool,
    threshold: usize,
) -> bool {
    let key = (tool_name.to_string(), project.to_string());
    let mut streaks = APPROVAL_STREAKS.lock().unwrap_or_else(|e| e.into_inner());

    if !approved {
        streaks.insert(key, 0);
        return false;
    }

    let count = streaks.entry(key).or_insert(0);
    *count += 1;

    if *count >= threshold {
        let rule = format!("[policy] allow tool {} in {}", tool_name, project);
        store.insert(&rule, tool_name, 0.85);
        return true;
    }

    false
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

    #[tokio::test]
    async fn trace_import_generates_candidate_from_successful_fix() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = CandidateStore::new(tmp.path().join("test.db"));
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        raios_core::db::tool_trace_insert(
            &conn,
            raios_core::db::ToolTraceInsert {
                project: "R-AI-OS",
                agent: "codex_kaira",
                command: "cargo test -p raios-runtime trace_recall",
                context: "runtime trace recall",
                outcome: "tests passed",
                error_summary: "trace recall missed partial phrase",
                fix_summary: "fall back to significant query tokens before project fallback",
                tags_json: r#"["trace"]"#,
                success: true,
                confidence: 0.9,
                related_task_id: None,
            },
        )
        .unwrap();

        let inserted = import_trace_candidates(&conn, &store, Some("R-AI-OS"), 10).unwrap();
        let candidates = store.list_pending(10);

        assert_eq!(inserted, 1);
        assert!(candidates
            .iter()
            .any(|rule| rule.contains("significant query tokens")));
    }

    #[tokio::test]
    async fn candidate_store_supports_core_schema_candidates_table() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("workspace.db");
        let conn = Connection::open(&db_path).unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        drop(conn);

        let store = CandidateStore::new(&db_path);
        store.insert(
            "When trace recall fails, inspect token fallback",
            "trace:test",
            0.7,
        );
        let candidates = store.list_pending(10);

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].contains("token fallback"));
        store.promote(&candidates[0]);
        assert!(store.list_pending(10).is_empty());
    }

    #[tokio::test]
    async fn approval_streak_promotes_policy_rule_after_n_consecutive_approvals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("workspace.db");
        let store = CandidateStore::new(&db_path);

        let tool = "custom_exec";
        let project = "test_project";

        // 4 approvals (threshold 5) -> no candidate yet
        for _ in 0..4 {
            let res = record_approval_decision(&store, tool, project, true, 5);
            assert!(!res);
        }
        assert!(store.list_pending(10).is_empty());

        // 5th approval -> emits candidate
        let res = record_approval_decision(&store, tool, project, true, 5);
        assert!(res);

        let candidates = store.list_pending(10);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].contains("[policy] allow tool custom_exec in test_project"));

        // Interleaved denial resets streak
        let tool2 = "reset_tool";
        for _ in 0..3 {
            record_approval_decision(&store, tool2, project, true, 5);
        }
        // Denial
        record_approval_decision(&store, tool2, project, false, 5);

        // 2 more approvals (total 2 after reset) -> still 0 for tool2
        for _ in 0..2 {
            record_approval_decision(&store, tool2, project, true, 5);
        }
        let candidates_after = store.list_pending(10);
        assert_eq!(
            candidates_after.len(),
            1,
            "existing candidate preserved, tool2 not added"
        );
    }
}
