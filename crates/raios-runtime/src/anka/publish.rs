//! Owner-only, atomic publication of ANKA's on-disk state.
//!
//! The old writer broke two rules at once, both fixed here:
//!
//! - **No permissions window.** The temporary file is born `0600`, because the mode is
//!   passed to `open(2)` and applied at creation, before a single byte is written. The
//!   old path created it with the process umask and only `chmod`ed afterwards, leaving
//!   a world-readable interval. `chmod` after the fact cannot close a window that
//!   already happened, so there is no `chmod` in this module's publish path at all.
//! - **Unique temporary names.** Every writer gets its own name, so two publishers can
//!   never interleave bytes in one shared `index.tmp`.
//!
//! ## What a failure does and does not promise
//!
//! The phases are not interchangeable, so they are not reported interchangeably — see
//! [`PublishError`]:
//!
//! | Where it stopped | What readers see | What survives a crash |
//! | --- | --- | --- |
//! | before `rename` | the previous file, byte for byte | the previous file |
//! | `rename` succeeded, directory `fsync` failed | the **new** file | unconfirmed |
//! | fully successful | the new file | the new file |
//!
//! A published file replaces the previous inode atomically at `rename(2)`, so a crash
//! at any point leaves a complete old cache, a complete new cache, or a stray temporary
//! file — never a half-written index. The directory is `fsync`ed afterwards so the
//! rename itself is durable rather than merely visible; if that sync cannot be
//! confirmed the caller is told so instead of being handed a generic failure that
//! would suggest the old content is still in place.

use anyhow::{bail, Context, Result};
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Why a publish failed, and — the part that matters to a caller — how far it got.
///
/// Collapsing these into one error would be actively misleading: "the write failed"
/// means the previous file is intact, while "the sync failed" means the new content is
/// already what readers get. A caller deciding whether to retry, or whether state was
/// lost, needs that difference rather than a message.
#[derive(Debug)]
pub enum PublishError {
    /// Nothing was published. `path` still holds its previous content byte for byte,
    /// and the half-written temporary has been removed.
    NotPublished(anyhow::Error),
    /// The replacement already happened: `path` now holds the new content and readers
    /// see it. What is unconfirmed is durability — after a crash the directory entry
    /// may still resolve to the previous content. Retrying the same write is the
    /// remedy; reporting this as "nothing was published" would be false.
    PublishedUnsynced(anyhow::Error),
}

impl PublishError {
    /// `true` when the destination already holds the new content.
    #[allow(dead_code)] // used by tests; will be consumed by the CLI/MCP surfaces in step 4
    pub fn is_published(&self) -> bool {
        matches!(self, Self::PublishedUnsynced(_))
    }
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPublished(cause) => write!(
                f,
                "nothing was published, the previous file is intact: {cause:#}"
            ),
            Self::PublishedUnsynced(cause) => write!(
                f,
                "the file was replaced but its durability could not be confirmed: {cause:#}"
            ),
        }
    }
}

// The cause chain is rendered into `Display` above (anyhow's `{:#}` prints it in
// full), so `source` deliberately reports nothing rather than a flattened duplicate.
impl std::error::Error for PublishError {}

/// Create `path` only if it does not exist, already owner-only.
///
/// `mode(0o600)` is applied by `open(2)`, so the file is readable by its owner alone
/// from the instant it exists — the umask can only remove bits from the requested mode,
/// never add them. This is the property the old create-then-chmod sequence lacked.
pub(super) fn create_owner_only(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Replace `path` with `content`, atomically and without a permissions window.
///
/// See [`PublishError`] for which failure means what.
pub(super) fn atomic_write(path: &Path, content: &[u8]) -> Result<(), PublishError> {
    atomic_write_with(path, content, sync_directory)
}

/// [`atomic_write`] with the directory sync supplied by the caller, which is the only
/// seam tests need: everything up to and including the `rename` is the real code, and
/// the injected failure lands precisely where a crash-durability failure lands.
///
/// `pub(super)` so the tombstone publisher's own tests can drive the same seam and
/// prove that an unconfirmed tombstone publish reaches its caller as an error.
pub(super) fn atomic_write_with<F>(
    path: &Path,
    content: &[u8],
    sync_dir: F,
) -> Result<(), PublishError>
where
    F: FnOnce(&Path) -> Result<()>,
{
    // Deliberately no directory creation or hardening here. This function's job is to
    // replace one file; hidden `chmod`s inside a write mask the very permission
    // failures a caller needs to see, and every caller already creates its directory
    // owner-only on entry (`index_in`, `forget_in`, `AnkaLock::open`, `write_tombstones`).
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let (temporary, mut file) = create_temporary(path).map_err(PublishError::NotPublished)?;
    if let Err(error) = file
        .write_all(content)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("could not write {}", temporary.display()))
    {
        drop(file);
        remove_quietly(&temporary);
        return Err(PublishError::NotPublished(error));
    }
    // The handle must be closed before the rename so no reader can observe a locked
    // destination on platforms that refuse to replace an open file.
    drop(file);

    if let Err(error) = fs::rename(&temporary, path) {
        remove_quietly(&temporary);
        let error = anyhow::Error::from(error).context(format!(
            "could not publish {} from {}",
            path.display(),
            temporary.display()
        ));
        return Err(PublishError::NotPublished(error));
    }

    // Past this point the destination *is* the new content: a failure here can no
    // longer be described as "the previous file is intact".
    if let Err(error) = sync_dir(parent) {
        return Err(PublishError::PublishedUnsynced(error));
    }
    Ok(())
}

/// Create the parent directory chain owner-only, at creation time.
///
/// `DirBuilder::mode` applies to every directory this call creates, so there is no
/// interval in which a fresh `.cache/raios/anka` is traversable by anyone else. An
/// existing directory is hardened to `0700` as well, which fixes caches created by
/// older code rather than leaving them permissive.
pub(super) fn ensure_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .with_context(|| format!("could not create directory {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("could not harden {}", path.display()))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path).with_context(|| format!("could not create {}", path.display()))?;
    Ok(())
}

/// A name no other writer can be using, opened with `O_EXCL` so a collision is
/// reported rather than shared.
fn create_temporary(path: &Path) -> Result<(PathBuf, File)> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("anka");
    let pid = std::process::id();

    // `create_new` is the authority: a candidate that already exists is skipped, so
    // uniqueness comes from the kernel rather than from this process's assumptions.
    for _ in 0..64 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!("{base}.{pid}.{sequence}.tmp"));
        match create_owner_only(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not create {}", candidate.display()))
            }
        }
    }
    bail!(
        "could not allocate a unique temporary name next to {}",
        path.display()
    )
}

fn sync_directory(directory: &Path) -> Result<()> {
    let handle =
        File::open(directory).with_context(|| format!("could not open {}", directory.display()))?;
    handle
        .sync_all()
        .with_context(|| format!("could not sync {}", directory.display()))
}

fn remove_quietly(path: &Path) {
    // Best effort: a leftover temporary file is inert, whereas failing the caller for
    // not being able to delete it would hide the original error.
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    fn file_mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    fn temporary_files(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .expect("read dir")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".tmp"))
            })
            .collect()
    }

    /// The property the create-then-chmod sequence could not offer: at the moment the
    /// file first exists, before anything is written to it, it is already `0600`.
    #[test]
    fn a_temporary_file_is_owner_only_before_a_byte_is_written() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("index.json");

        let file = create_owner_only(&target).expect("create");
        assert_eq!(
            file_mode(&target),
            0o600,
            "the file must be owner-only at creation, not after a later chmod"
        );

        // Still `0600` once written and synced, and after publication.
        let mut file = file;
        file.write_all(b"{\"records\":[]}").expect("write");
        file.sync_all().expect("sync");
        drop(file);
        assert_eq!(file_mode(&target), 0o600);
    }

    #[test]
    fn a_published_file_is_owner_only_and_leaves_no_temporary_behind() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("index.json");
        let content = br#"{"records":[],"indexed_sources":1}"#;

        atomic_write(&target, content).expect("publish");

        assert_eq!(fs::read(&target).expect("read back"), content);
        assert_eq!(file_mode(&target), 0o600);
        assert!(
            temporary_files(temp.path()).is_empty(),
            "a successful publish must leave no temporary file"
        );
    }

    /// Directory hardening is its own responsibility, tested on its own helper rather
    /// than smuggled in as a side effect of a file write: a fresh chain comes out
    /// `0700`, and one made permissive by older code is brought back to `0700`.
    #[test]
    fn directories_are_owner_only_when_created_and_hardened_when_already_permissive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let created = temp.path().join("cache").join("anka");

        ensure_private_dir(&created).expect("create");
        assert_eq!(file_mode(&created), 0o700);
        // The chain is created owner-only too, not just the leaf.
        assert_eq!(file_mode(&temp.path().join("cache")), 0o700);

        fs::set_permissions(&created, fs::Permissions::from_mode(0o755)).expect("relax");
        ensure_private_dir(&created).expect("harden");
        assert_eq!(
            file_mode(&created),
            0o700,
            "an existing permissive directory must be brought back to owner-only"
        );
    }

    /// An existing permissive target is replaced by an owner-only one rather than
    /// inheriting its mode.
    #[test]
    fn publishing_over_a_world_readable_file_makes_it_owner_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("anka-tombstones");
        fs::write(&target, b"old-id\n").expect("seed");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("relax perms");
        assert_eq!(file_mode(&target), 0o644);

        atomic_write(&target, b"new-id\n").expect("publish");

        assert_eq!(file_mode(&target), 0o600);
        assert_eq!(fs::read(&target).expect("read back"), b"new-id\n");
    }

    /// Concurrency is the reason the temporary name cannot be a constant: N writers
    /// race on one destination, and the result must be exactly one writer's bytes.
    #[test]
    fn concurrent_publishes_never_interleave_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = Arc::new(temp.path().join("index.json"));
        let payloads: Vec<Vec<u8>> = (0..8)
            .map(|index| {
                let mut payload = b"{\"writer\":\"".to_vec();
                payload.extend_from_slice(index.to_string().as_bytes());
                payload.push(b'"');
                payload.extend(std::iter::repeat_n(b'x', 64 * 1024));
                payload.extend_from_slice(b"}");
                payload
            })
            .collect();

        let mut handles = Vec::new();
        for payload in payloads.clone() {
            let target = Arc::clone(&target);
            handles.push(std::thread::spawn(move || {
                atomic_write(&target, &payload).expect("publish");
            }));
        }
        for handle in handles {
            handle.join().expect("publisher finished");
        }

        let published = fs::read(&*target).expect("read back");
        assert_eq!(
            file_mode(&target),
            0o600,
            "every publish path keeps the destination owner-only"
        );
        assert!(
            payloads.contains(&published),
            "the destination must equal exactly one writer's payload, not a mixture"
        );
        assert!(temporary_files(temp.path()).is_empty());
    }

    /// A failed publish leaves the previous content intact rather than truncating it:
    /// the temporary is written first and only swapped in on success. A `rename`
    /// failure happens *before* the swap, so this is the `NotPublished` phase.
    #[test]
    fn a_destination_that_is_a_directory_fails_without_losing_anything() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("index.json");
        fs::create_dir(&target).expect("destination is a directory");

        let error = atomic_write(&target, b"{}").expect_err("rename must fail");
        assert!(
            !error.is_published(),
            "a failure before the rename must report as NotPublished, got: {error}"
        );
        assert!(
            matches!(error, PublishError::NotPublished(_)),
            "the phase must be carried, got: {error}"
        );
        assert!(target.is_dir(), "the destination is untouched on failure");
        assert!(
            temporary_files(temp.path()).is_empty(),
            "a failed publish cleans up its own temporary file"
        );
    }

    /// The phase the old contract got wrong: once `rename` has happened, the
    /// destination already holds the new content, so a failing directory `fsync` cannot
    /// be reported as "nothing was published". The caller is told the content changed
    /// but its durability is unconfirmed — which is exactly the state a crash would
    /// leave behind.
    #[test]
    fn a_failing_directory_sync_after_the_rename_reports_the_content_as_replaced() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("index.json");
        atomic_write(&target, b"old content").expect("seed publish");

        let error = atomic_write_with(&target, b"new content", |_| {
            Err(anyhow::anyhow!("injected: directory sync unavailable"))
        })
        .expect_err("the injected sync failure must surface");

        assert!(
            error.is_published(),
            "past the rename the content IS replaced, got: {error}"
        );
        assert!(matches!(error, PublishError::PublishedUnsynced(_)));
        assert!(
            error
                .to_string()
                .contains("durability could not be confirmed"),
            "the message must say what actually happened, got: {error}"
        );
        assert_eq!(
            fs::read(&target).expect("read back"),
            b"new content",
            "readers see the new content even though the sync failed"
        );
        assert!(
            temporary_files(temp.path()).is_empty(),
            "the rename already consumed the temporary file"
        );
    }
}
