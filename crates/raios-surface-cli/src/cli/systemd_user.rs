//! Shared plumbing for the user-scoped systemd unit pairs `raios` installs.
//!
//! The ANKA refresh timer and the database backup timer both follow the `hub.rs`
//! convention: unit text is generated from live values at install time, written
//! into the user's unit directory, and applied with `systemctl --user`. This
//! module owns the parts that must not drift between them — the directory, the
//! subprocess boundary, removal, and `ExecStart=` quoting.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Quotes a binary path for an `ExecStart=`/`ExecStartPost=` line.
///
/// systemd's `ExecStart=` parser splits the first token on unquoted whitespace
/// and expands `%` specifiers: a binary path containing a space, a quote, or a
/// literal `%` would otherwise be parsed as a different program (or a
/// specifier) instead of the binary that installed the unit.
pub(super) fn quote_exec_path(path: &Path) -> String {
    let raw = path.display().to_string();
    format!(
        "\"{}\"",
        raw.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

/// `systemctl --user …` with captured stderr, so a failed rollout step names
/// its own cause instead of printing a bare exit code.
pub(super) fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("could not run `systemctl --user`")?;
    if !output.status.success() {
        bail!(
            "`systemctl --user {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// The user unit directory, created when missing.
pub(super) fn user_unit_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not resolve the home directory")?;
    let dir = home.join(".config/systemd/user");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}

/// `Ok(true)` — deleted now; `Ok(false)` — was already absent (uninstall stays
/// idempotent); an error propagates as a rollback failure the operator must
/// see.
pub(super) fn remove_unit(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paths the unit parser would otherwise split or expand must survive
    /// verbatim: spaces stay one token, `%` becomes `%%` so systemd does not
    /// treat it as a specifier, and quotes/backslashes are escaped.
    #[test]
    fn exec_paths_are_one_quoted_token_with_specifiers_escaped() {
        assert_eq!(
            quote_exec_path(Path::new("/home/John Smith/raios 100%")),
            "\"/home/John Smith/raios 100%%\""
        );
        assert_eq!(
            quote_exec_path(Path::new("/opt/we\"ird\\path/raios")),
            "\"/opt/we\\\"ird\\\\path/raios\""
        );
        assert_eq!(
            quote_exec_path(Path::new("/home/alaz/.local/bin/raios")),
            "\"/home/alaz/.local/bin/raios\""
        );
    }

    #[test]
    fn remove_unit_reports_absent_units_without_failing() {
        let missing = std::env::temp_dir().join("raios-missing-unit.service");
        let _ = fs::remove_file(&missing);
        assert!(!remove_unit(&missing).expect("absence is not a failure"));

        let present = std::env::temp_dir().join("raios-present-unit.service");
        fs::write(&present, "[Unit]\n").expect("fixture write");
        assert!(remove_unit(&present).expect("deletion works"));
        assert!(!remove_unit(&present).expect("already gone is not a failure"));
    }
}
