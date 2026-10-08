//! The single interprocess lock serializing ANKA's read-modify-write sections.
//!
//! Before this existed, `forget` performed read-index → append-tombstone → write-index
//! with no coordination, and `write_tombstones` rewrites the whole tombstone file
//! itself: two concurrent `forget`s could lose one of them, and a `forget` racing an
//! `index` could publish an index built from a stale tombstone set — silently
//! resurrecting a record the user had just hidden.
//!
//! Everything that changes ANKA's state takes this lock; readers (`search`, `status`)
//! deliberately do not, because they only ever see a fully published index (see
//! [`crate::anka::publish`]).
//!
//! ## Why a crash cannot strand it
//!
//! The lock is an advisory lock on an open descriptor, released by the kernel when
//! that descriptor closes — including when the process dies. There is no lock-file
//! state to go stale, so a crashed indexer never blocks the next run, and a *busy* lock
//! only means "another writer is mid-update": nothing is written until it is free, so
//! the last valid cache and all durable tombstones stay intact either way.

use anyhow::{Context, Result};
use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use super::provenance::open_regular_file;
use super::publish::{create_owner_only, ensure_private_dir};

/// Lock file name inside the cache directory.
const LOCK_FILE: &str = "anka.lock";

/// Holder of ANKA's exclusive interprocess lock.
///
/// Dropping the value releases the lock, because the lock lives on the descriptor and
/// the descriptor closes with it — there is no unlock to forget and no lock-file state
/// to go stale. The value intentionally exposes no way to release early, so a critical
/// section cannot be exited by mistake while shared state is still being rewritten.
#[derive(Debug)]
pub struct AnkaLock {
    /// Held purely for its lifetime: the advisory lock exists on this descriptor and
    /// is released the moment it closes. `_file` rather than `file` because nothing
    /// reads it — keeping it alive *is* the whole contract.
    _file: File,
}

impl AnkaLock {
    /// Take the exclusive lock, waiting while another writer holds it.
    ///
    /// The cache directory is created owner-only first, so the lock file itself is
    /// never placed in a traversable directory.
    pub fn acquire(cache: &Path) -> Result<Self> {
        let file = Self::open(cache)?;
        file.lock()
            .with_context(|| format!("could not lock {}", cache.join(LOCK_FILE).display()))?;
        Ok(Self { _file: file })
    }

    /// Take the lock only if it is free right now.
    ///
    /// `Ok(None)` means someone else holds it. Nothing is written and no file other
    /// than the lock file itself is touched, which is what makes this the right entry
    /// point for callers that must fail fast rather than queue — the daily timer, for
    /// instance, would rather skip a run than pile up behind a manual rebuild.
    pub fn try_acquire(cache: &Path) -> Result<Option<Self>> {
        let file = Self::open(cache)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error)
                .with_context(|| format!("could not lock {}", cache.join(LOCK_FILE).display())),
        }
    }

    /// Open the lock file, creating it owner-only if it does not exist yet.
    ///
    /// Creation uses `O_EXCL`, so a file that is already there is opened through the
    /// regular-file check rather than blindly followed — a symlink planted where the
    /// lock should be is rejected instead of being locked and trusted.
    fn open(cache: &Path) -> Result<File> {
        ensure_private_dir(cache)?;
        let path = cache.join(LOCK_FILE);
        match create_owner_only(&path) {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => open_regular_file(&path)
                .with_context(|| format!("could not open lock file {}", path.display())),
            Err(error) => {
                Err(error).with_context(|| format!("could not create {}", path.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn file_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    use std::fs;

    #[test]
    fn the_lock_file_is_owner_only_when_it_is_first_created() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = temp.path().join("anka");

        let held = AnkaLock::acquire(&cache).expect("acquire");
        assert_eq!(file_mode(&cache.join(LOCK_FILE)), 0o600);
        drop(held);

        // Reopening an existing lock file must work as well as creating one.
        let reopened = AnkaLock::acquire(&cache).expect("reopen");
        drop(reopened);
    }

    /// The whole point of the lock: while one writer is inside its critical section,
    /// a second writer cannot enter, and it does not touch anything on the way out.
    #[test]
    fn a_second_writer_cannot_enter_until_the_first_releases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = temp.path().join("anka");
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let holder = std::thread::spawn({
            let cache = cache.clone();
            move || {
                let lock = AnkaLock::acquire(&cache).expect("holder acquires");
                held_tx.send(()).expect("signal held");
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("release signal");
                drop(lock);
            }
        });

        held_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("holder is up");
        assert!(
            AnkaLock::try_acquire(&cache)
                .expect("try acquire")
                .is_none(),
            "a free lock must report as busy while another writer holds it"
        );

        release_tx.send(()).expect("release");
        holder.join().expect("holder finished");

        let taken = AnkaLock::try_acquire(&cache)
            .expect("try acquire")
            .expect("the lock must be free once the holder is gone");
        drop(taken);
    }

    /// Failing fast must be side-effect free apart from the lock file itself: a busy
    /// lock leaves the published cache exactly where it was.
    #[test]
    fn failing_to_take_a_busy_lock_leaves_the_cache_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = temp.path().join("anka");
        fs::create_dir_all(&cache).expect("cache dir");
        let index = cache.join("index.json");
        fs::write(&index, br#"{"records":[]}"#).expect("seed index");

        let held = AnkaLock::acquire(&cache).expect("acquire");
        let listing_before = std::fs::read_dir(&cache)
            .expect("read dir")
            .flatten()
            .map(|entry| {
                (
                    entry.file_name(),
                    entry.metadata().expect("entry metadata").len(),
                )
            })
            .collect::<Vec<_>>();

        assert!(AnkaLock::try_acquire(&cache).expect("try").is_none());

        let listing_after = std::fs::read_dir(&cache)
            .expect("read dir")
            .flatten()
            .map(|entry| {
                (
                    entry.file_name(),
                    entry.metadata().expect("entry metadata").len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(listing_before, listing_after);
        assert_eq!(
            fs::read(&index).expect("index content"),
            br#"{"records":[]}"#
        );
        drop(held);
    }

    /// A symlink where the lock file should be is rejected, not followed: locking a
    /// file the caller does not own would hand out guarantees it cannot make.
    #[test]
    fn a_symlinked_lock_file_is_refused_instead_of_locked() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = temp.path().join("anka");
        let outside = temp.path().join("outside.lock");
        fs::write(&outside, b"").expect("outside file");
        fs::create_dir_all(&cache).expect("cache dir");
        std::os::unix::fs::symlink(&outside, cache.join(LOCK_FILE)).expect("plant symlink");

        let error = AnkaLock::acquire(&cache).expect_err("must refuse");
        assert!(
            error.to_string().contains("lock file"),
            "the refusal must name the lock file, got: {error}"
        );
    }
}
