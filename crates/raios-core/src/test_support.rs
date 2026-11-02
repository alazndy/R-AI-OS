//! Helpers shared by the unit tests of every crate in the workspace.
//!
//! This lives in the library rather than behind `#[cfg(test)]` because a
//! crate's `cfg(test)` items are invisible to the crates that depend on it,
//! and the other crates' tests need the same guard. It is tiny and only used
//! from tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// The one lock every test that reads or writes `RAIOS_DB_PATH` must hold.
///
/// The variable is process-global, so two tests that each set it under their
/// own separate `Mutex` still clobber each other. Every crate's former
/// `DB_ENV_LOCK` is now an alias of this one.
pub static DB_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Points the workspace database at a private temp file for the lifetime of
/// the guard, so a test can never read or write the developer's real
/// `~/.config/raios/workspace.db` (or its tamper-evident audit ledger).
///
/// Holds [`DB_ENV_LOCK`] for its whole life: tests using it run one at a time
/// relative to each other, which is what makes the env-var swap safe. Do not
/// create one while already holding the lock — it is not re-entrant.
pub struct IsolatedDb {
    _lock: MutexGuard<'static, ()>,
    dir: PathBuf,
    previous: Option<OsString>,
}

impl IsolatedDb {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let lock = DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "raios-test-db-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create isolated test db dir");
        let previous = std::env::var_os("RAIOS_DB_PATH");
        std::env::set_var("RAIOS_DB_PATH", dir.join("workspace.db"));
        Self {
            _lock: lock,
            dir,
            previous,
        }
    }

    /// Directory holding the isolated database.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Default for IsolatedDb {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IsolatedDb {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
            None => std::env::remove_var("RAIOS_DB_PATH"),
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_db_redirects_and_restores_the_db_path() {
        let before = std::env::var_os("RAIOS_DB_PATH");
        {
            let iso = IsolatedDb::new();
            let inside = crate::db::db_path();
            assert!(inside.starts_with(iso.dir()), "{inside:?}");
            // A connection really opens there, not at the default location.
            let _conn = crate::db::open_db().expect("open isolated db");
            assert!(inside.exists());
        }
        assert_eq!(std::env::var_os("RAIOS_DB_PATH"), before);
    }
}
