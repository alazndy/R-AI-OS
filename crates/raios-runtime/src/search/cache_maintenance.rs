//! Explicit global maintenance for rebuildable search caches, including deleted
//! roots which a project-scoped index refresh can no longer visit.

use super::indexer::{INDEXED_EXTS, MAX_INDEX_FILE_BYTES, SKIP_DIRS};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Default, Serialize)]
pub struct CachePruneCounts {
    pub scanned_files: usize,
    pub candidate_files: usize,
    pub candidate_postings: u64,
    pub removed_files: usize,
    pub removed_postings: u64,
    pub reasons: BTreeMap<&'static str, usize>,
}

#[derive(Debug, Serialize)]
pub struct CachePruneReport {
    pub scanned_at_utc: String,
    pub database_path: PathBuf,
    pub applied: bool,
    pub bm25: CachePruneCounts,
    pub trigram: CachePruneCounts,
}

#[derive(Clone, Copy)]
enum CacheKind {
    Bm25,
    Trigram,
}

struct Candidate {
    id: i64,
    path: PathBuf,
}

/// Preview without migrations or writes. Applying obtains SQLite's writer lock,
/// scans again rather than trusting a saved preview, checks each path immediately
/// before deletion and records the change in the same audit-ledger transaction.
pub fn prune_search_cache(database_path: &Path, apply: bool) -> Result<CachePruneReport> {
    let flags = if apply {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let mut conn = Connection::open_with_flags(database_path, flags)
        .context("cannot open existing search-cache database")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    if apply {
        conn.pragma_update(None, "foreign_keys", true)?;
    }
    let tx = conn.transaction_with_behavior(if apply {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    })?;

    let (mut bm25, bm25_candidates) = scan_cache(&tx, CacheKind::Bm25)?;
    let (mut trigram, trigram_candidates) = scan_cache(&tx, CacheKind::Trigram)?;
    if apply {
        remove_candidates(&tx, CacheKind::Bm25, &bm25_candidates, &mut bm25)?;
        remove_candidates(&tx, CacheKind::Trigram, &trigram_candidates, &mut trigram)?;
    }
    let report = CachePruneReport {
        scanned_at_utc: chrono::Utc::now().to_rfc3339(),
        database_path: database_path.to_path_buf(),
        applied: apply,
        bm25,
        trigram,
    };
    if report.bm25.removed_files + report.trigram.removed_files > 0 {
        // Only aggregate counts go to the ledger, never cached paths/snippets.
        let data = serde_json::to_string(&serde_json::json!({
            "bm25": report.bm25,
            "trigram": report.trigram,
        }))?;
        raios_core::security::record_audit_event(
            &tx,
            "search_cache_pruned",
            "raios_db_maintenance",
            &data,
        )
        .context("cannot record maintenance audit; cache deletion rolled back")?;
    }
    tx.commit()?;
    Ok(report)
}

fn scan_cache(conn: &Connection, kind: CacheKind) -> Result<(CachePruneCounts, Vec<Candidate>)> {
    let (table, select) = match kind {
        CacheKind::Bm25 => ("bm25_files", "SELECT id, path FROM bm25_files"),
        CacheKind::Trigram => ("trigram_files", "SELECT id, path FROM trigram_files"),
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?;
    let mut counts = CachePruneCounts::default();
    let mut candidates = Vec::new();
    if !exists {
        return Ok((counts, candidates));
    }
    let mut stmt = conn.prepare(select)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, path) = row?;
        counts.scanned_files += 1;
        let path = PathBuf::from(path);
        if let Some(reason) = ineligible_reason(&path)? {
            counts.candidate_files += 1;
            counts.candidate_postings += posting_count(conn, kind, id)?;
            *counts.reasons.entry(reason).or_default() += 1;
            candidates.push(Candidate { id, path });
        }
    }
    Ok((counts, candidates))
}

fn ineligible_reason(path: &Path) -> Result<Option<&'static str>> {
    if !path.is_absolute() {
        bail!("refusing to classify relative cached paths against the current directory");
    }
    // Inspect metadata first: permission and I/O failures are not evidence of
    // absence, even if the path is also covered by an exclusion policy.
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Some("missing")),
        Err(error) => return Err(error).context("cannot safely inspect cached file metadata"),
    };
    if !metadata.is_file() {
        return Ok(Some("not_regular_file"));
    }
    if path.parent().is_some_and(|parent| {
        parent.components().any(|part| {
            part.as_os_str().to_str().is_some_and(|name| {
                name != ".worktrees" && name != "worktrees" && SKIP_DIRS.contains(&name)
            })
        })
    }) {
        return Ok(Some("excluded_directory"));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| INDEXED_EXTS.contains(&extension))
    {
        return Ok(Some("unsupported_extension"));
    }
    if metadata.len() > MAX_INDEX_FILE_BYTES {
        return Ok(Some("oversized"));
    }
    Ok(None)
}

fn posting_count(conn: &Connection, kind: CacheKind, id: i64) -> Result<u64> {
    let sql = match kind {
        CacheKind::Bm25 => "SELECT COUNT(*) FROM bm25_postings WHERE file_id=?1",
        CacheKind::Trigram => "SELECT COUNT(*) FROM trigram_postings WHERE file_id=?1",
    };
    Ok(conn.query_row(sql, [id], |row| row.get(0))?)
}

fn remove_candidates(
    conn: &Connection,
    kind: CacheKind,
    candidates: &[Candidate],
    counts: &mut CachePruneCounts,
) -> Result<()> {
    let (delete_postings, delete_file) = match kind {
        CacheKind::Bm25 => (
            "DELETE FROM bm25_postings WHERE file_id=?1",
            "DELETE FROM bm25_files WHERE id=?1 AND path=?2",
        ),
        CacheKind::Trigram => (
            "DELETE FROM trigram_postings WHERE file_id=?1",
            "DELETE FROM trigram_files WHERE id=?1 AND path=?2",
        ),
    };
    let mut postings_stmt = conn.prepare(delete_postings)?;
    let mut file_stmt = conn.prepare(delete_file)?;
    for candidate in candidates {
        if ineligible_reason(&candidate.path)?.is_none() {
            continue;
        }
        // Explicit child deletion also supports older caches without a cascade.
        counts.removed_postings += postings_stmt.execute([candidate.id])? as u64;
        counts.removed_files += file_stmt.execute(params![
            candidate.id,
            candidate.path.to_string_lossy().as_ref()
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{indexer::ProjectIndex, trigram};
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("cache.db");
        let root = tmp.path().join("live");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("main.rs"), "fn durable_search_marker() {}\n").unwrap();
        ProjectIndex::load_or_build(&root, &db, false).unwrap();
        trigram::ensure_index(&root, &db, false).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE audit_log (id INTEGER PRIMARY KEY, timestamp TEXT, event_type TEXT,
             actor TEXT, data TEXT, prev_hash TEXT, hash TEXT);
             CREATE TABLE mem_items (body TEXT);
             INSERT INTO mem_items VALUES ('owner memory');",
        )
        .unwrap();
        (tmp, db, root)
    }

    fn seed(conn: &Connection, path: &Path) {
        conn.execute(
            "INSERT INTO bm25_files(path,mtime_secs,doc_length) VALUES(?1,0,1)",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO bm25_postings(token,file_id,line_no,snippet) VALUES('stale',?1,1,'cached')", [id]).unwrap();
        conn.execute(
            "INSERT INTO trigram_files(path,mtime_secs) VALUES(?1,0)",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO trigram_postings(trigram,file_id) VALUES('sta',?1)",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn preview_preserves_bytes_and_apply_prunes_deleted_roots_and_generated_output() {
        let (tmp, db, root) = fixture();
        let generated = root.join(".firebase/bundle.js");
        fs::create_dir_all(generated.parent().unwrap()).unwrap();
        fs::write(&generated, "generated deployment bundle").unwrap();
        let missing = tmp.path().join("deleted-project/main.rs");
        let conn = Connection::open(&db).unwrap();
        seed(&conn, &generated);
        seed(&conn, &missing);
        drop(conn);
        let before = fs::read(&db).unwrap();
        let preview = prune_search_cache(&db, false).unwrap();
        assert_eq!(preview.bm25.candidate_files, 2);
        assert_eq!(preview.trigram.candidate_postings, 2);
        assert_eq!(preview.bm25.removed_files, 0);
        assert_eq!(before, fs::read(&db).unwrap());
        let applied = prune_search_cache(&db, true).unwrap();
        assert_eq!(applied.bm25.removed_files, 2);
        assert_eq!(applied.trigram.removed_postings, 2);
        let conn = Connection::open(&db).unwrap();
        let memory: String = conn
            .query_row("SELECT body FROM mem_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(memory, "owner memory");
        let audits: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audits, 1);
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
        assert!(!ProjectIndex::load_or_build(&root, &db, false)
            .unwrap()
            .search("durable")
            .is_empty());
        assert!(
            !trigram::locate(&root, &db, "durable_search_marker", false, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(prune_search_cache(&db, true).unwrap().bm25.removed_files, 0);
    }

    #[test]
    fn restored_file_is_rechecked_before_deletion() {
        let (tmp, db, _) = fixture();
        let path = tmp.path().join("restored.rs");
        let conn = Connection::open(&db).unwrap();
        seed(&conn, &path);
        let (mut counts, candidates) = scan_cache(&conn, CacheKind::Bm25).unwrap();
        assert_eq!(counts.candidate_files, 1);
        fs::write(&path, "fn restored() {}\n").unwrap();
        remove_candidates(&conn, CacheKind::Bm25, &candidates, &mut counts).unwrap();
        assert_eq!(counts.removed_files, 0);
    }

    #[test]
    fn metadata_errors_and_audit_failures_do_not_commit_partial_pruning() {
        let (tmp, db, _) = fixture();
        let conn = Connection::open(&db).unwrap();
        seed(&conn, &tmp.path().join("missing.rs"));
        conn.execute("DROP TABLE audit_log", []).unwrap();
        assert!(prune_search_cache(&db, true).is_err());
        let files: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(files, 2);
        let relative = Path::new("relative.rs");
        seed(&conn, relative);
        assert!(prune_search_cache(&db, true).is_err());
        // ELOOP is an inspection error, not a missing file.
        #[cfg(unix)]
        {
            let loop_path = tmp.path().join("loop.rs");
            std::os::unix::fs::symlink(&loop_path, &loop_path).unwrap();
            assert!(ineligible_reason(&loop_path).is_err());
        }
    }

    #[test]
    fn missing_database_is_not_created_and_size_limit_matches_indexing_policy() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("missing.db");
        assert!(prune_search_cache(&db, false).is_err());
        assert!(prune_search_cache(&db, true).is_err());
        assert!(!db.exists());
        let path = tmp.path().join("large.rs");
        fs::write(&path, vec![b'x'; MAX_INDEX_FILE_BYTES as usize + 1]).unwrap();
        assert_eq!(ineligible_reason(&path).unwrap(), Some("oversized"));
    }
}
