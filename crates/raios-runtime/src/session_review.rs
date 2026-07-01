use std::path::Path;
use std::time::SystemTime;

use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PostRunReview {
    pub changed: Option<String>,
    pub tests_run_during_session: bool,
    pub risks: Vec<String>,
    pub learned: Vec<String>,
}

impl PostRunReview {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

pub fn build_review(
    conn: &Connection,
    agent: &str,
    project_dir: &Path,
    session_started: SystemTime,
    session_start_utc: &str,
    session_end_utc: &str,
) -> PostRunReview {
    let changed = crate::git_utils::diff_stat(project_dir);
    let tests_run_during_session = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM audit_log
                WHERE event_type = 'tool_allow'
                  AND timestamp >= ?1
                  AND timestamp <= ?2
                  AND data LIKE '%\"tool\":\"run_tests\"%'
            )",
            params![session_start_utc, session_end_utc],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v == 1)
        .unwrap_or(false);

    let mut risks = Vec::new();
    if let Some(stat) = &changed {
        let impact = raios_core::db::parse_diff_stat(stat);
        let large_change = impact.lines_changed.unwrap_or(0) >= 25 || impact.files_changed.unwrap_or(0) >= 3;
        if large_change && !tests_run_during_session {
            risks.push("large change with no test run detected".to_string());
        }
        if stat.contains("raios-policy.toml") || stat.contains(".env") || stat.contains("security/") {
            risks.push("security-sensitive files touched".to_string());
        }
    }

    let learned = project_dir
        .to_str()
        .map(|project| crate::session_memory::collect_transcript(agent, project, session_started))
        .map(|transcript| crate::session_memory::decision_lines_from_transcript(&transcript))
        .unwrap_or_default();

    PostRunReview {
        changed,
        tests_run_during_session,
        risks,
        learned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_audit(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                actor TEXT NOT NULL,
                data TEXT NOT NULL,
                prev_hash TEXT NOT NULL,
                hash TEXT NOT NULL
            );",
        )
        .unwrap();
    }

    fn init_git_repo(tmp: &tempfile::TempDir) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("file.txt"), "one\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
    }

    #[test]
    fn detects_test_run_in_audit_window() {
        let conn = Connection::open_in_memory().unwrap();
        setup_audit(&conn);
        raios_core::security::record_tool_decision(
            &conn,
            "run_tests",
            "deadbeef",
            "default",
            "tool_allow",
            "claude_kaira",
        )
        .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        init_git_repo(&tmp);
        std::fs::write(tmp.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();

        let review = build_review(
            &conn,
            "claude",
            tmp.path(),
            SystemTime::now(),
            "2000-01-01T00:00:00Z",
            "2999-01-01T00:00:00Z",
        );
        assert!(review.tests_run_during_session);
    }

    #[test]
    fn flags_large_untested_change() {
        let conn = Connection::open_in_memory().unwrap();
        setup_audit(&conn);
        let tmp = tempfile::tempdir().unwrap();
        init_git_repo(&tmp);
        std::fs::write(
            tmp.path().join("file.txt"),
            (0..40).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n"),
        )
        .unwrap();

        let review = build_review(
            &conn,
            "claude",
            tmp.path(),
            SystemTime::now(),
            "2000-01-01T00:00:00Z",
            "2999-01-01T00:00:00Z",
        );
        assert!(review.risks.iter().any(|r| r.contains("large change")));
    }
}
