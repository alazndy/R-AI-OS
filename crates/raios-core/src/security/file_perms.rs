//! Cross-platform "owner-only" file permission hardening for on-disk secrets
//! (session tokens, API keys). Unix has always had this via chmod; Windows
//! previously had nothing, leaving those files readable by any other local
//! account.

use std::io;
use std::path::Path;

/// Restricts a file to the owning user only.
///
/// Unix: chmod 0600.
/// Windows: strips inherited ACEs and grants Full Control solely to the
/// current user via `icacls` (shelling out rather than adding an ACL crate
/// dependency, consistent with the existing `tasklist`/`taskkill` shellouts
/// already used elsewhere in this codebase for Windows-specific process
/// management).
pub fn harden_file_perms(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }

    #[cfg(windows)]
    {
        let username = std::env::var("USERNAME")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "USERNAME env var not set"))?;
        let status = std::process::Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{username}:F"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "icacls failed to restrict file permissions",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hardens_a_freshly_written_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("secret");
        std::fs::write(&path, b"top-secret").unwrap();

        harden_file_perms(&path).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn errors_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("does-not-exist");
        assert!(harden_file_perms(&path).is_err());
    }
}
