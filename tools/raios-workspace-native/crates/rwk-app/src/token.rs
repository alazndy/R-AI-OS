//! The daemon's session token, read from the file `aiosd` writes (`<config>/raios/.session_token`).
//!
//! The same rules as the daemon applies when it validates: the file must be owner-only (mode 600
//! or 400) and at most eight hours old. Anything else fails closed with an error; the token is
//! read again on every call because the daemon rotates it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rwk_core::control::daemon::TokenSource;
use rwk_core::error::{AppError, ErrorCode, Result};

const DIRECTORY: &str = "raios";
const FILE: &str = ".session_token";
/// How old the token file may be.
pub const MAX_AGE: Duration = Duration::from_secs(8 * 60 * 60);

/// Where the token file lives inside a config home.
pub fn token_path(config_home: &Path) -> PathBuf {
    config_home.join(DIRECTORY).join(FILE)
}

/// Reads the token from the daemon's session token file.
pub struct FileTokenSource {
    path: PathBuf,
}

impl FileTokenSource {
    /// A source that reads `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

fn unauthorized(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Unauthorized, message)
}

/// Validates the file's mode and age, then returns its trimmed content.
fn read_token(path: &Path, now: SystemTime) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| unauthorized("The daemon's session token file was not found."))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 && mode != 0o400 {
            return Err(unauthorized(
                "The session token file must be readable by its owner only.",
            ));
        }
    }
    #[cfg(not(unix))]
    {
        // The owner-only check of this file is implemented for Unix; elsewhere it fails closed.
        let _ = &metadata;
        return Err(unauthorized(
            "Reading the session token is not supported on this platform yet.",
        ));
    }
    #[cfg(unix)]
    {
        let age = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or(Duration::ZERO);
        if age > MAX_AGE {
            return Err(unauthorized("The session token has expired."));
        }
        let text = std::fs::read_to_string(path)
            .map_err(|_| unauthorized("The session token file could not be read."))?;
        let token = text.trim();
        if token.is_empty() {
            return Err(unauthorized("The session token file is empty."));
        }
        Ok(token.to_owned())
    }
}

impl TokenSource for FileTokenSource {
    fn bearer_token(&self) -> Result<String> {
        read_token(&self.path, SystemTime::now())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn file_with(mode: u32, content: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join(FILE);
        std::fs::write(&path, content).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        (directory, path)
    }

    #[test]
    fn an_owner_only_fresh_file_yields_the_trimmed_token() {
        let (_dir, path) = file_with(0o600, "abc123\n");

        assert_eq!(read_token(&path, SystemTime::now()).unwrap(), "abc123");
    }

    #[test]
    fn a_read_only_owner_file_is_accepted_too() {
        let (_dir, path) = file_with(0o400, "abc123");

        assert!(read_token(&path, SystemTime::now()).is_ok());
    }

    #[test]
    fn a_file_others_can_read_is_refused() {
        for mode in [0o644, 0o640, 0o660, 0o666] {
            let (_dir, path) = file_with(mode, "abc123");

            let error = read_token(&path, SystemTime::now()).unwrap_err();

            assert_eq!(error.code, ErrorCode::Unauthorized, "mode {mode:o}");
        }
    }

    #[test]
    fn a_file_older_than_eight_hours_is_refused() {
        let (_dir, path) = file_with(0o600, "abc123");
        let later = SystemTime::now() + MAX_AGE + Duration::from_secs(60);

        let error = read_token(&path, later).unwrap_err();

        assert_eq!(error.code, ErrorCode::Unauthorized);
    }

    #[test]
    fn a_missing_or_empty_file_is_refused() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(read_token(&directory.path().join(FILE), SystemTime::now()).is_err());
        let (_dir, path) = file_with(0o600, " \n");
        assert!(read_token(&path, SystemTime::now()).is_err());
    }

    #[test]
    fn the_path_is_inside_the_raios_config_directory() {
        assert_eq!(
            token_path(Path::new("/home/u/.config")),
            PathBuf::from("/home/u/.config/raios/.session_token")
        );
    }
}
