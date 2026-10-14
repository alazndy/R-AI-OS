//! Owner-reviewed repair of missing task references, preserving contracts.
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct OrphanContract {
    pub contract_id: String,
    pub missing_task_id: String,
    pub referencing_runs: usize,
}

#[derive(Debug, Serialize)]
pub struct ContractRecoveryReport {
    pub applied: bool,
    pub candidates: Vec<OrphanContract>,
    pub detached: usize,
}

/// Clear only dangling task links on unreferenced contracts. This implements
/// the schema's ON DELETE SET NULL intent without deleting historic policy.
pub fn repair_orphan_contracts(path: &Path, apply: bool) -> Result<ContractRecoveryReport> {
    let mut conn = Connection::open_with_flags(
        path,
        if apply {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if apply {
        conn.pragma_update(None, "foreign_keys", true)?;
    }
    let tx = conn.transaction_with_behavior(if apply {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    })?;
    let candidates = {
        let mut stmt = tx.prepare("SELECT c.id,c.task_id,(SELECT count(*) FROM cp_agent_runs r WHERE r.run_contract_id=c.id) FROM cp_run_contracts c WHERE c.task_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM cp_tasks t WHERE t.id=c.task_id) ORDER BY c.id")?;
        let rows = stmt.query_map([], |r| {
            Ok(OrphanContract {
                contract_id: r.get(0)?,
                missing_task_id: r.get(1)?,
                referencing_runs: r.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut report = ContractRecoveryReport {
        applied: apply,
        candidates,
        detached: 0,
    };
    if apply {
        if report.candidates.iter().any(|c| c.referencing_runs != 0) {
            bail!("refusing to detach a contract referenced by an agent run; investigate canonical ownership first");
        }
        for candidate in &report.candidates {
            let changed = tx.execute("UPDATE cp_run_contracts SET task_id=NULL WHERE id=?1 AND task_id=?2 AND NOT EXISTS(SELECT 1 FROM cp_tasks WHERE id=?2) AND NOT EXISTS(SELECT 1 FROM cp_agent_runs WHERE run_contract_id=?1)", params![candidate.contract_id,candidate.missing_task_id])?;
            if changed != 1 {
                bail!("orphan contract changed during repair");
            }
            crate::security::record_audit_event(&tx, "orphan_contract_detached", "raios_db_maintenance", &serde_json::json!({"contract_id":candidate.contract_id,"missing_task_id":candidate.missing_task_id}).to_string())?;
            report.detached += 1;
        }
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let conn = Connection::open(&path).unwrap();
        crate::db::migrate_existing(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let contract = crate::db::cp_insert_run_contract(
            &conn,
            None,
            "/project",
            "[\"source\"]",
            "[]",
            "[]",
            None,
            None,
        )
        .unwrap();
        conn.execute(
            "UPDATE cp_run_contracts SET task_id='missing' WHERE id=?1",
            [&contract],
        )
        .unwrap();
        (temp, path, contract)
    }

    #[test]
    fn preview_is_read_only_and_apply_preserves_policy_with_a_verified_audit() {
        let (_temp, path, id) = fixture();
        let report = repair_orphan_contracts(&path, false).unwrap();
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.detached, 0);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            crate::db::cp_get_run_contract(&conn, &id)
                .unwrap()
                .unwrap()
                .task_id
                .as_deref(),
            Some("missing")
        );
        let report = repair_orphan_contracts(&path, true).unwrap();
        assert_eq!(report.detached, 1);
        let contract = crate::db::cp_get_run_contract(&conn, &id).unwrap().unwrap();
        assert_eq!(contract.task_id, None);
        assert_eq!(contract.allowed_paths_json, "[\"source\"]");
        assert_eq!(crate::security::verify_chain(&conn).unwrap(), 1);
        assert_eq!(repair_orphan_contracts(&path, true).unwrap().detached, 0);
    }

    #[test]
    fn audit_failure_rolls_back_detachment() {
        let (_temp, path, id) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE audit_log").unwrap();
        assert!(repair_orphan_contracts(&path, true).is_err());
        assert_eq!(
            crate::db::cp_get_run_contract(&conn, &id)
                .unwrap()
                .unwrap()
                .task_id
                .as_deref(),
            Some("missing")
        );
    }

    #[test]
    fn referenced_contract_blocks_the_entire_repair() {
        let (_temp, path, id) = fixture();
        let conn = Connection::open(&path).unwrap();
        let (_, run) = crate::db::cp_session_start(&conn, "codex_kaira", None).unwrap();
        conn.execute(
            "UPDATE cp_agent_runs SET run_contract_id=?1 WHERE id=?2",
            params![id, run],
        )
        .unwrap();
        assert_eq!(
            repair_orphan_contracts(&path, false).unwrap().candidates[0].referencing_runs,
            1
        );
        assert!(repair_orphan_contracts(&path, true).is_err());
        assert_eq!(
            crate::db::cp_get_run_contract(&conn, &id)
                .unwrap()
                .unwrap()
                .task_id
                .as_deref(),
            Some("missing")
        );
    }
}
