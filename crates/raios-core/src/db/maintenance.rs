use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

pub const DEFAULT_SNAPSHOT_RETENTION: usize = 3;
pub const MAX_SNAPSHOT_RETENTION: usize = 10;

#[derive(Debug, Clone, Serialize)]
pub struct DbMaintenanceCheck {
    pub database_path: PathBuf,
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub page_size: i64,
    pub page_count: i64,
    pub freelist_pages: i64,
    pub journal_mode: String,
    pub check_mode: String,
    pub integrity_messages: Vec<String>,
    pub healthy: bool,
    pub snapshot_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbSnapshotReport {
    pub snapshot_path: PathBuf,
    pub checksum_path: PathBuf,
    pub snapshot_bytes: u64,
    pub sha256: String,
    pub check_mode: String,
    pub integrity_messages: Vec<String>,
    pub retained_snapshots: usize,
    pub pruned_snapshots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbCheckpointReport {
    pub mode: String,
    pub busy: i64,
    pub wal_frames: i64,
    pub checkpointed_frames: i64,
    pub wal_bytes_before: u64,
    pub wal_bytes_after: u64,
}

pub fn check_workspace_database(full: bool) -> Result<DbMaintenanceCheck> {
    let database_path = super::db_path();
    check_database(&database_path, &default_snapshot_dir(&database_path), full)
}

pub fn backup_workspace_database(retain: usize) -> Result<DbSnapshotReport> {
    let database_path = super::db_path();
    backup_database(
        &database_path,
        &default_snapshot_dir(&database_path),
        retain,
    )
}

pub fn checkpoint_workspace_database(truncate: bool) -> Result<DbCheckpointReport> {
    checkpoint_database(&super::db_path(), truncate)
}

pub fn check_database(
    database_path: &Path,
    snapshot_dir: &Path,
    full: bool,
) -> Result<DbMaintenanceCheck> {
    let conn = open_read_only(database_path)?;
    let integrity_messages = integrity_check(&conn, full)?;
    let page_size = pragma_i64(&conn, "PRAGMA page_size")?;
    let page_count = pragma_i64(&conn, "PRAGMA page_count")?;
    let freelist_pages = pragma_i64(&conn, "PRAGMA freelist_count")?;
    let journal_mode = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .context("failed to read SQLite journal mode")?;

    Ok(DbMaintenanceCheck {
        database_path: database_path.to_path_buf(),
        database_bytes: file_size(database_path)?,
        wal_bytes: optional_file_size(&companion_path(database_path, "-wal"))?,
        shm_bytes: optional_file_size(&companion_path(database_path, "-shm"))?,
        page_size,
        page_count,
        freelist_pages,
        journal_mode,
        check_mode: if full { "full" } else { "quick" }.to_string(),
        healthy: integrity_messages.len() == 1 && integrity_messages[0] == "ok",
        integrity_messages,
        snapshot_count: managed_snapshots(snapshot_dir)?.len(),
    })
}

pub fn backup_database(
    database_path: &Path,
    snapshot_dir: &Path,
    retain: usize,
) -> Result<DbSnapshotReport> {
    if !(1..=MAX_SNAPSHOT_RETENTION).contains(&retain) {
        bail!("snapshot retention must be between 1 and {MAX_SNAPSHOT_RETENTION}");
    }

    let source_check = check_database(database_path, snapshot_dir, false)?;
    if !source_check.healthy {
        bail!(
            "refusing to back up a database that failed integrity_check: {}",
            source_check.integrity_messages.join("; ")
        );
    }

    create_private_dir(snapshot_dir)?;
    let snapshot_path = unique_snapshot_path(snapshot_dir);
    let checksum_path = checksum_path(&snapshot_path);
    create_private_file(&snapshot_path)?;

    let result = (|| -> Result<DbSnapshotReport> {
        let source = open_read_only(database_path)?;
        let mut destination = Connection::open(&snapshot_path).with_context(|| {
            format!(
                "failed to open snapshot destination {}",
                snapshot_path.display()
            )
        })?;
        destination
            .busy_timeout(Duration::from_secs(5))
            .context("failed to configure snapshot busy timeout")?;

        {
            let backup = Backup::new(&source, &mut destination)
                .context("failed to initialize SQLite online backup")?;
            backup
                .run_to_completion(512, Duration::from_millis(5), None)
                .context("SQLite online backup did not complete")?;
        }
        drop(destination);
        drop(source);

        set_private_file_permissions(&snapshot_path)?;
        let snapshot = check_database(&snapshot_path, snapshot_dir, false)?;
        if !snapshot.healthy {
            bail!(
                "snapshot failed integrity_check: {}",
                snapshot.integrity_messages.join("; ")
            );
        }
        remove_empty_snapshot_companions(&snapshot_path)?;

        // Make the replacement snapshot and its directory entry durable before
        // deleting any older recovery point. If persistence fails, retention
        // must leave the existing backups untouched.
        sync_file(&snapshot_path)?;
        let sha256 = sha256_file(&snapshot_path)?;
        write_checksum(&checksum_path, &snapshot_path, &sha256)?;
        sync_snapshot_dir(snapshot_dir)?;

        let pruned_snapshots = prune_snapshots(snapshot_dir, retain)?;
        sync_snapshot_dir(snapshot_dir)?;
        let retained_snapshots = managed_snapshots(snapshot_dir)?.len();

        Ok(DbSnapshotReport {
            snapshot_path: snapshot_path.clone(),
            checksum_path: checksum_path.clone(),
            snapshot_bytes: file_size(&snapshot_path)?,
            sha256,
            check_mode: snapshot.check_mode,
            integrity_messages: snapshot.integrity_messages,
            retained_snapshots,
            pruned_snapshots,
        })
    })();

    if result.is_err() {
        remove_regular_file_if_present(&snapshot_path);
        remove_regular_file_if_present(&checksum_path);
        remove_regular_file_if_present(&companion_path(&snapshot_path, "-wal"));
        remove_regular_file_if_present(&companion_path(&snapshot_path, "-shm"));
    }
    result
}

pub fn checkpoint_database(database_path: &Path, truncate: bool) -> Result<DbCheckpointReport> {
    let wal_path = companion_path(database_path, "-wal");
    let wal_bytes_before = optional_file_size(&wal_path)?;
    let conn = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| {
            format!(
                "failed to open existing database: {}",
                database_path.display()
            )
        })?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("failed to configure checkpoint busy timeout")?;
    let query = if truncate {
        "PRAGMA wal_checkpoint(TRUNCATE)"
    } else {
        "PRAGMA wal_checkpoint(PASSIVE)"
    };
    let (busy, wal_frames, checkpointed_frames) = conn
        .query_row(query, [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .with_context(|| format!("failed to execute {query}"))?;
    drop(conn);

    Ok(DbCheckpointReport {
        mode: if truncate { "truncate" } else { "passive" }.to_string(),
        busy,
        wal_frames,
        checkpointed_frames,
        wal_bytes_before,
        wal_bytes_after: optional_file_size(&wal_path)?,
    })
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open database read-only: {}", path.display()))
}

fn integrity_check(conn: &Connection, full: bool) -> Result<Vec<String>> {
    let query = if full {
        "PRAGMA integrity_check"
    } else {
        "PRAGMA quick_check"
    };
    let mut stmt = conn
        .prepare(query)
        .with_context(|| format!("failed to prepare SQLite check: {query}"))?;
    let rows = stmt
        .query_map([], |row| row.get(0))
        .with_context(|| format!("failed to execute SQLite check: {query}"))?;
    rows.collect::<rusqlite::Result<Vec<String>>>()
        .with_context(|| format!("failed to collect SQLite check results: {query}"))
}

fn pragma_i64(conn: &Connection, query: &str) -> Result<i64> {
    conn.query_row(query, [], |row| row.get(0))
        .with_context(|| format!("failed to execute {query}"))
}

fn default_snapshot_dir(database_path: &Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

fn companion_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn checksum_path(snapshot_path: &Path) -> PathBuf {
    companion_path(snapshot_path, ".sha256")
}

fn unique_snapshot_path(snapshot_dir: &Path) -> PathBuf {
    let timestamp = Utc::now().format("%Y%m%dT%H%M%S%9fZ");
    let nonce = Uuid::new_v4().simple().to_string();
    snapshot_dir.join(format!("workspace-{timestamp}-{}.db", &nonce[..12]))
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create snapshot directory {}", path.display()))?;
    set_private_dir_permissions(path)
}

fn create_private_file(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("failed to create private snapshot file {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to secure snapshot directory {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to secure snapshot file {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn write_checksum(path: &Path, snapshot_path: &Path, sha256: &str) -> Result<()> {
    let filename = snapshot_path
        .file_name()
        .ok_or_else(|| anyhow!("snapshot path has no filename"))?
        .to_string_lossy();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create checksum file {}", path.display()))?;
    writeln!(file, "{sha256}  {filename}")
        .with_context(|| format!("failed to write checksum file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync checksum file {}", path.display()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open snapshot for hashing: {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash snapshot {}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn managed_snapshots(snapshot_dir: &Path) -> Result<Vec<PathBuf>> {
    if !snapshot_dir.exists() {
        return Ok(Vec::new());
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(snapshot_dir).with_context(|| {
        format!(
            "failed to read snapshot directory {}",
            snapshot_dir.display()
        )
    })? {
        let entry = entry.context("failed to read snapshot directory entry")?;
        let file_type = entry
            .file_type()
            .context("failed to read snapshot file type")?;
        if !file_type.is_file() {
            continue;
        }
        let name: OsString = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("workspace-") && name.ends_with(".db") {
            snapshots.push(entry.path());
        }
    }
    snapshots.sort();
    Ok(snapshots)
}

fn prune_snapshots(snapshot_dir: &Path, retain: usize) -> Result<Vec<PathBuf>> {
    let snapshots = managed_snapshots(snapshot_dir)?;
    let prune_count = snapshots.len().saturating_sub(retain);
    let mut pruned = Vec::with_capacity(prune_count);
    for snapshot in snapshots.into_iter().take(prune_count) {
        fs::remove_file(&snapshot)
            .with_context(|| format!("failed to prune old snapshot {}", snapshot.display()))?;
        remove_regular_file(&checksum_path(&snapshot))?;
        remove_regular_file(&companion_path(&snapshot, "-wal"))?;
        remove_regular_file(&companion_path(&snapshot, "-shm"))?;
        pruned.push(snapshot);
    }
    Ok(pruned)
}

fn remove_empty_snapshot_companions(snapshot_path: &Path) -> Result<()> {
    let wal_path = companion_path(snapshot_path, "-wal");
    let wal_bytes = optional_file_size(&wal_path)?;
    if wal_bytes != 0 {
        bail!(
            "snapshot has an unexpected non-empty WAL companion ({} bytes): {}",
            wal_bytes,
            wal_path.display()
        );
    }
    remove_regular_file(&wal_path)?;
    remove_regular_file(&companion_path(snapshot_path, "-shm"))
}

fn remove_regular_file(path: &Path) -> Result<()> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(path)
            .with_context(|| format!("failed to remove managed file {}", path.display())),
        Ok(_) => bail!(
            "refusing to remove non-regular managed path {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect managed file {}", path.display()))
        }
    }
}

fn remove_regular_file_if_present(path: &Path) {
    if path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_file())
    {
        let _ = fs::remove_file(path);
    }
}

fn sync_file(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("failed to reopen {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

#[cfg(unix)]
fn sync_snapshot_dir(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("failed to open snapshot directory {}", path.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync snapshot directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_snapshot_dir(_path: &Path) -> Result<()> {
    Ok(())
}

fn file_size(path: &Path) -> Result<u64> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))
        .map(|metadata| metadata.len())
}

fn optional_file_size(path: &Path) -> Result<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("failed to stat {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_database(dir: &Path) -> PathBuf {
        let path = dir.join("workspace.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE facts (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
             INSERT INTO facts (body) VALUES ('durable');",
        )
        .unwrap();
        path
    }

    #[test]
    fn check_reports_integrity_and_storage_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let database = source_database(temp.path());
        let snapshots = temp.path().join("backups");

        let report = check_database(&database, &snapshots, false).unwrap();

        assert!(report.healthy);
        assert_eq!(report.check_mode, "quick");
        assert_eq!(report.integrity_messages, vec!["ok"]);
        assert_eq!(report.journal_mode, "wal");
        assert!(report.database_bytes > 0);
        assert!(report.page_count > 0);
        assert_eq!(report.snapshot_count, 0);
    }

    #[test]
    fn online_backup_is_private_hashed_and_integrity_checked() {
        let temp = tempfile::tempdir().unwrap();
        let database = source_database(temp.path());
        let snapshots = temp.path().join("backups");

        let report = backup_database(&database, &snapshots, 3).unwrap();

        assert_eq!(report.integrity_messages, vec!["ok"]);
        assert_eq!(report.retained_snapshots, 1);
        assert!(report.pruned_snapshots.is_empty());
        assert_eq!(report.sha256.len(), 64);
        assert!(!companion_path(&report.snapshot_path, "-wal").exists());
        assert!(!companion_path(&report.snapshot_path, "-shm").exists());
        let checksum = fs::read_to_string(&report.checksum_path).unwrap();
        assert!(checksum.starts_with(&report.sha256));
        let snapshot =
            Connection::open_with_flags(&report.snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let body: String = snapshot
            .query_row("SELECT body FROM facts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(body, "durable");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&snapshots).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&report.snapshot_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn retention_prunes_only_managed_snapshots_and_their_checksums() {
        let temp = tempfile::tempdir().unwrap();
        let database = source_database(temp.path());
        let snapshots = temp.path().join("backups");
        fs::create_dir_all(&snapshots).unwrap();
        fs::write(snapshots.join("operator-notes.txt"), "keep me").unwrap();

        for _ in 0..3 {
            backup_database(&database, &snapshots, 2).unwrap();
        }

        assert_eq!(managed_snapshots(&snapshots).unwrap().len(), 2);
        assert!(snapshots.join("operator-notes.txt").exists());
        let checksum_count = fs::read_dir(&snapshots)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".db.sha256"))
            .count();
        assert_eq!(checksum_count, 2);
    }

    #[test]
    fn retention_is_bounded_before_any_snapshot_is_created() {
        let temp = tempfile::tempdir().unwrap();
        let database = source_database(temp.path());
        let snapshots = temp.path().join("backups");

        assert!(backup_database(&database, &snapshots, 0).is_err());
        assert!(backup_database(&database, &snapshots, MAX_SNAPSHOT_RETENTION + 1).is_err());
        assert!(!snapshots.exists());
    }

    #[test]
    fn checkpoint_reports_wal_progress_without_touching_database_rows() {
        let temp = tempfile::tempdir().unwrap();
        let database = source_database(temp.path());

        let report = checkpoint_database(&database, true).unwrap();

        assert_eq!(report.mode, "truncate");
        assert_eq!(report.busy, 0);
        assert_eq!(report.wal_frames, 0);
        assert_eq!(report.checkpointed_frames, 0);
        let conn = open_read_only(&database).unwrap();
        let body: String = conn
            .query_row("SELECT body FROM facts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(body, "durable");
    }

    #[test]
    fn checkpoint_refuses_to_create_a_missing_database() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.db");

        assert!(checkpoint_database(&missing, false).is_err());
        assert!(!missing.exists());
    }
}
