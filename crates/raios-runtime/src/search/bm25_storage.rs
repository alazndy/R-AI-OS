//! Explicit, audited conversion of legacy repeated snippets to per-line storage.
//! Preview opens the database read-only; production apply requires a fresh backup
//! and coordinated replacement of every reader (CLI and daemon).
use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const LINE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS bm25_lines (
    file_id INTEGER NOT NULL REFERENCES bm25_files(id) ON DELETE CASCADE,
    line_no INTEGER NOT NULL, snippet TEXT NOT NULL,
    PRIMARY KEY(file_id,line_no)) WITHOUT ROWID;";
pub(crate) const META_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS bm25_storage_meta (
    key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;";

pub(crate) fn normalized_storage(conn: &Connection) -> Result<bool> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='bm25_storage_meta' AND type='table')",[],|r|r.get(0))?;
    if !exists {
        return Ok(false);
    }
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM bm25_storage_meta WHERE key='snippet_format'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match value.as_deref() {
        None => Ok(false),
        Some("lines_v1") => Ok(true),
        _ => bail!("unsupported BM25 snippet format"),
    }
}

#[derive(Debug, Serialize)]
pub struct NormalizationReport {
    pub database_path: PathBuf,
    pub applied: bool,
    pub legacy_postings: u64,
    pub legacy_snippet_bytes: u64,
    pub distinct_legacy_lines: u64,
    pub line_snippet_bytes: u64,
    pub conflicting_lines: u64,
    pub converted_postings: u64,
    pub estimated_payload_savings: u64,
    pub normalized_enabled: bool,
}

pub fn normalize_bm25(path: &Path, apply: bool) -> Result<NormalizationReport> {
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
    let enabled = normalized_storage(&tx)?;
    let (legacy_postings,legacy_snippet_bytes) = tx.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(CAST(snippet AS BLOB))),0) FROM bm25_postings WHERE snippet!=''",
        [],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,u64>(1)?)))?;
    let (distinct_legacy_lines, line_snippet_bytes, conflicting_lines) = tx.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(CAST(snippet AS BLOB))),0),COALESCE(SUM(conflict),0)
         FROM (SELECT MIN(snippet) AS snippet,MIN(snippet)!=MAX(snippet) AS conflict
               FROM bm25_postings WHERE snippet!='' GROUP BY file_id,line_no)",
        [],
        |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u64>(2)?,
            ))
        },
    )?;
    let mut report = NormalizationReport {
        database_path: path.into(),
        applied: apply,
        legacy_postings,
        legacy_snippet_bytes,
        distinct_legacy_lines,
        line_snippet_bytes,
        conflicting_lines,
        converted_postings: 0,
        estimated_payload_savings: legacy_snippet_bytes.saturating_sub(line_snippet_bytes),
        normalized_enabled: enabled,
    };
    if apply && (legacy_postings > 0 || !enabled) {
        if conflicting_lines > 0 {
            bail!("conflicting snippets on {conflicting_lines} lines; no conversion performed");
        }
        tx.execute_batch(LINE_SCHEMA)?;
        tx.execute_batch(META_SCHEMA)?;
        let mismatched: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bm25_postings p JOIN bm25_lines l ON l.file_id=p.file_id AND l.line_no=p.line_no WHERE p.snippet!='' AND p.snippet!=l.snippet)",[],|r|r.get(0))?;
        if mismatched {
            bail!("legacy and normalized snippets conflict; no conversion performed");
        }
        tx.execute("INSERT OR IGNORE INTO bm25_lines(file_id,line_no,snippet) SELECT file_id,line_no,MIN(snippet) FROM bm25_postings WHERE snippet!='' GROUP BY file_id,line_no",[])?;
        let invalid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM bm25_postings p LEFT JOIN bm25_lines l ON l.file_id=p.file_id AND l.line_no=p.line_no WHERE l.file_id IS NULL OR (p.snippet!='' AND p.snippet!=l.snippet))",[],|r|r.get(0))?;
        if invalid {
            bail!("a posting has no matching line snippet; no conversion performed");
        }
        report.converted_postings =
            tx.execute("UPDATE bm25_postings SET snippet='' WHERE snippet!=''", [])? as u64;
        tx.execute("INSERT INTO bm25_storage_meta(key,value) VALUES('snippet_format','lines_v1') ON CONFLICT(key) DO UPDATE SET value=excluded.value",[])?;
        report.normalized_enabled = true;
        raios_core::security::record_audit_event(
            &tx,
            "bm25_snippets_normalized",
            "raios_db_maintenance",
            &serde_json::to_string(&report)?,
        )
        .context("normalization audit failed; transaction rolled back")?;
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::indexer::ProjectIndex;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("main.rs"), "alpha alpha bravo\ncharlie delta\n").unwrap();
        let db = temp.path().join("cache.db");
        let conn = Connection::open(&db).unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        ProjectIndex::load_or_build(&root, &db, false).unwrap();
        conn.execute_batch("UPDATE bm25_postings SET snippet=(SELECT snippet FROM bm25_lines l WHERE l.file_id=bm25_postings.file_id AND l.line_no=bm25_postings.line_no); DELETE FROM bm25_lines;").unwrap();
        (temp, root, db)
    }

    #[test]
    fn conversion_preserves_posting_order_scores_and_snippets_and_is_idempotent() {
        let (_temp, root, db) = fixture();
        std::fs::write(root.join("second.rs"), "rareword independent anotherword\n").unwrap();
        let before = ProjectIndex::load_or_build(&root, &db, false)
            .unwrap()
            .search("alpha bravo rareword");
        assert!(before.iter().any(|row| row.score > 0.0));
        let preview = normalize_bm25(&db, false).unwrap();
        assert_eq!(preview.legacy_postings, 5);
        assert_eq!(preview.distinct_legacy_lines, 2);
        assert!(preview.estimated_payload_savings > 0);
        let applied = normalize_bm25(&db, true).unwrap();
        assert_eq!(applied.converted_postings, 5);
        let after = ProjectIndex::load_or_build(&root, &db, false)
            .unwrap()
            .search("alpha bravo rareword");
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(a.path, b.path);
            assert_eq!(a.line, b.line);
            assert_eq!(a.snippet, b.snippet);
            assert_eq!(a.score.to_bits(), b.score.to_bits());
        }
        assert_eq!(normalize_bm25(&db, true).unwrap().converted_postings, 0);
        let conn = Connection::open(&db).unwrap();
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_postings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 8);
        ProjectIndex::load_or_build(&root, &db, true).unwrap();
        assert_eq!(normalize_bm25(&db, false).unwrap().legacy_postings, 0);
    }

    #[test]
    fn conflicts_and_failed_audits_leave_legacy_content_intact() {
        let (_temp, _root, db) = fixture();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TRIGGER refuse_normalization_audit BEFORE INSERT ON audit_log BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
        assert!(normalize_bm25(&db, true).is_err());
        assert_eq!(normalize_bm25(&db, false).unwrap().legacy_postings, 5);
        let lines: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_lines", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lines, 0);
        conn.execute_batch("DROP TRIGGER refuse_normalization_audit; UPDATE bm25_postings SET snippet='different' WHERE rowid=(SELECT MIN(rowid) FROM bm25_postings);").unwrap();
        assert_eq!(normalize_bm25(&db, false).unwrap().conflicting_lines, 1);
        assert!(normalize_bm25(&db, true).is_err());
        assert_eq!(normalize_bm25(&db, false).unwrap().legacy_postings, 5);
    }

    #[test]
    fn missing_normalized_snippet_is_an_error_instead_of_silent_search_loss() {
        let (_temp, root, db) = fixture();
        normalize_bm25(&db, true).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute("DELETE FROM bm25_lines", []).unwrap();
        assert!(ProjectIndex::load_or_build(&root, &db, false).is_err());
    }

    #[test]
    fn existing_cache_keeps_legacy_writes_until_explicit_conversion() {
        let (_temp, root, db) = fixture();
        let conn = Connection::open(&db).unwrap();
        conn.execute("DELETE FROM bm25_storage_meta", []).unwrap();
        std::fs::write(root.join("main.rs"), "newword newword anotherword\n").unwrap();
        ProjectIndex::load_or_build(&root, &db, true).unwrap();
        let preview = normalize_bm25(&db, false).unwrap();
        assert!(!preview.normalized_enabled);
        assert_eq!(preview.legacy_postings, 3);
        assert!(normalize_bm25(&db, true).unwrap().normalized_enabled);
        ProjectIndex::load_or_build(&root, &db, true).unwrap();
        assert_eq!(normalize_bm25(&db, false).unwrap().legacy_postings, 0);
    }
}
