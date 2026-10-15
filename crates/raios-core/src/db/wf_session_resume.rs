//! Durable launch metadata. Conversation bodies remain in their native agent stores.
#[cfg(test)]
use super::*;
use rusqlite::{params, Connection, Result, Transaction, TransactionBehavior};

#[derive(Debug, Clone, serde::Serialize)]
pub struct WrapperResumeRecord {
    pub run_id: String,
    pub native_session_id: Option<String>,
    pub workspace_root: String,
}

pub fn cp_wrapper_resume_record(conn: &Connection, run_id: &str) -> Result<WrapperResumeRecord> {
    conn.query_row("SELECT agent_run_id,native_session_id,workspace_root FROM cp_wrapper_resume WHERE agent_run_id=?1",[run_id],|r|Ok(WrapperResumeRecord{run_id:r.get(0)?,native_session_id:r.get(1)?,workspace_root:r.get(2)?}))
}

/// Bind a native identity before spawn; no raw launch flags or credentials are stored.
pub fn cp_bind_wrapper_resume(conn: &Connection, run_id: &str, native_id: &str) -> Result<()> {
    if native_id.is_empty()
        || native_id.len() > 128
        || !native_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "invalid native session ID".into(),
        ));
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let conflicting: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM cp_wrapper_resume j JOIN cp_agent_runs r ON r.id=j.agent_run_id JOIN cp_agent_runs current ON current.id=?2 WHERE j.native_session_id=?1 AND r.agent_name=current.agent_name AND r.status='running' AND r.id!=?2)",params![native_id,run_id],|r|r.get(0))?;
    if conflicting {
        return Err(rusqlite::Error::InvalidParameterName(
            "native conversation already has a running or unreconciled wrapper".into(),
        ));
    }
    let changed=tx.execute("UPDATE cp_wrapper_resume SET native_session_id=?1 WHERE agent_run_id=?2 AND native_session_id IS NULL AND EXISTS(SELECT 1 FROM cp_agent_runs WHERE id=?2 AND provider='wrapper' AND status='running')",params![native_id,run_id])?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidParameterName(
            "session identity is already bound or run is not active".into(),
        ));
    }
    crate::security::record_audit_event(
        &tx,
        "wrapper_resume_bound",
        "raios_wrapper",
        &serde_json::json!({"run_id":run_id,"native_session_id":native_id}).to_string(),
    )
    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::abs;
    #[test]
    fn launch_record_survives_reopen_without_session_end_and_binding_is_immutable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let run = {
            let conn = Connection::open(&path).unwrap();
            crate::db::migrate_existing(&conn).unwrap();
            let (_, run) = cp_session_start_named(
                &conn,
                "codex_kaira",
                None,
                abs("/workspace").as_str(),
                None,
                Some("Recovery"),
                Some("Check writes"),
            )
            .unwrap();
            cp_bind_wrapper_resume(&conn, &run, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
            run
        };
        let conn = Connection::open(path).unwrap();
        let record = cp_wrapper_resume_record(&conn, &run).unwrap();
        assert_eq!(record.workspace_root, abs("/workspace").as_str());
        assert!(record.native_session_id.is_some());
        assert!(cp_bind_wrapper_resume(&conn, &run, "other").is_err());
        assert_eq!(cp_sessions_list(&conn, 1).unwrap()[0].status, "running");
        assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
    }
    #[test]
    fn bind_audit_failure_rolls_back_identity() {
        let conn = crate::db::tests::in_memory();
        let (_, run) = cp_session_start(&conn, "claude_kaira", None).unwrap();
        conn.execute_batch("DROP TABLE audit_log").unwrap();
        assert!(cp_bind_wrapper_resume(&conn, &run, "test-id").is_err());
        assert!(cp_wrapper_resume_record(&conn, &run)
            .unwrap()
            .native_session_id
            .is_none());
    }
    #[test]
    fn only_one_running_wrapper_can_bind_the_same_native_conversation() {
        let conn = crate::db::tests::in_memory();
        let (task, first) = cp_session_start(&conn, "codex_kaira", None).unwrap();
        let (_, second) = cp_session_start(&conn, "codex_kaira", None).unwrap();
        cp_bind_wrapper_resume(&conn, &first, "native-id").unwrap();
        assert!(cp_bind_wrapper_resume(&conn, &second, "native-id").is_err());
        assert!(cp_wrapper_resume_record(&conn, &second)
            .unwrap()
            .native_session_id
            .is_none());
        cp_session_end(&conn, &task, &first, false).unwrap();
        cp_bind_wrapper_resume(&conn, &second, "native-id").unwrap();
    }
}
