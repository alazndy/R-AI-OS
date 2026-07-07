use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Maximum age of a token before it expires (8 hours)
const TOKEN_MAX_AGE: Duration = Duration::from_secs(8 * 60 * 60);

/// Draws 32 bytes directly from the OS CSPRNG and hex-encodes them into a
/// 256-bit secret. Used for session tokens and API keys — anywhere a bearer
/// credential needs to be generated. Deliberately does not hash any
/// additional inputs (pid, timestamps, uuids): the OS RNG is already the
/// primitive, and mixing in predictable data adds complexity without adding
/// security.
pub fn generate_secret_hex() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("OS CSPRNG unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Security helper for managing bootstrap session tokens.
/// The token is stored at `~/.config/raios/.session_token` with owner-only permissions.
pub struct SessionTokenManager {
    token_path: PathBuf,
}

impl Default for SessionTokenManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionTokenManager {
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios");
        let token_path = config_dir.join(".session_token");
        Self { token_path }
    }

    /// Constructs a manager pointing at an explicit path instead of the
    /// default `~/.config/raios/.session_token`. Not test-only: this is how
    /// downstream crates (e.g. raios-runtime's daemon bootstrap) inject a
    /// tempdir path in their own tests, since #[cfg(test)] items in this
    /// crate aren't visible to other crates' test builds.
    pub fn with_path(path: PathBuf) -> Self {
        Self { token_path: path }
    }

    /// Generates a new secure random session token, saves it to disk with owner-only permissions.
    pub fn generate_and_save(&self) -> Result<String> {
        // Create config dir if not exists
        if let Some(parent) = self.token_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let token = generate_secret_hex();

        // Write token to file
        fs::write(&self.token_path, &token)?;

        // Restrict to owner-only access (chmod 600 on Unix, owner-only DACL on Windows)
        crate::security::harden_file_perms(&self.token_path)?;

        Ok(token)
    }

    /// Reads the token from disk, verifying its permissions and expiry.
    pub fn get_valid_token(&self) -> Result<String> {
        if !self.token_path.exists() {
            return Err(anyhow!("Session token file does not exist"));
        }

        let metadata = fs::metadata(&self.token_path)?;

        // Verify Unix permissions are strictly owner-only (mode 0o600 or 0o400)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o600 && mode != 0o400 {
                return Err(anyhow!(
                    "Insecure permissions on token file: {:o}. Must be owner-only (600 or 400).",
                    mode
                ));
            }
        }
        #[cfg(windows)]
        {
            verify_windows_only_owner_can_read(&self.token_path)?;
        }

        // Verify token file is not older than 8 hours
        let modified = metadata.modified()?;
        let elapsed = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);

        if elapsed > TOKEN_MAX_AGE {
            return Err(anyhow!("Session token has expired"));
        }

        let content = fs::read_to_string(&self.token_path)?;
        Ok(content.trim().to_string())
    }

    /// Performs timing-safe comparison of the provided token against the stored token.
    pub fn validate_token(&self, provided: &str) -> bool {
        let stored = match self.get_valid_token() {
            Ok(t) => t,
            Err(_) => return false,
        };

        // Use a simple constant-time comparison helper to prevent timing attacks.
        constant_time_compare(provided.as_bytes(), stored.as_bytes())
    }

    /// Clears the token from disk.
    pub fn clear(&self) -> Result<()> {
        if self.token_path.exists() {
            fs::remove_file(&self.token_path)?;
        }
        Ok(())
    }
}

/// Verifies that only the current user (plus SYSTEM/Administrators, which
/// are effectively root-equivalent on Windows and can already bypass any
/// ACL) has access granted on `path`, by parsing `icacls` output.
///
/// This is the read-side counterpart to `harden_file_perms`'s write-side
/// `icacls /inheritance:r /grant:r`. Confirmed against real output on
/// GitHub's Windows runner: even after that call, `NT AUTHORITY\SYSTEM` and
/// `BUILTIN\Administrators` remain listed — `/inheritance:r` only strips
/// *inherited* ACEs, and these two get re-added as part of the volume's
/// default security descriptor for newly created objects, not via
/// inheritance. Unlike a regular user account, though, their presence isn't
/// a real access-control weakening: both already have unconditional access
/// to any file regardless of what its ACL says, the same way root on Unix
/// isn't blocked by chmod 600. The actual protection this check provides is
/// against *other standard user accounts* (a second local user, a
/// compromised low-privilege process, Users/Everyone/Authenticated Users
/// groups) — exactly the Unix chmod 600 threat model's closest Windows
/// analogue.
#[cfg(windows)]
fn verify_windows_only_owner_can_read(path: &std::path::Path) -> Result<()> {
    let username = std::env::var("USERNAME")
        .map_err(|_| anyhow!("USERNAME env var not set; cannot verify token file permissions"))?;

    let output = std::process::Command::new("icacls")
        .arg(path)
        .output()
        .map_err(|e| anyhow!("failed to run icacls: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!("icacls failed to inspect token file permissions"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path_str = path.display().to_string();
    let mut grantees = Vec::new();

    for (i, raw_line) in stdout.lines().enumerate() {
        // icacls prints "<path> ACCOUNT:(perms)" on the first line, then
        // "                     ACCOUNT:(perms)" (indented, no path) for
        // each further grantee, until a blank line ends the ACE list.
        let line = if i == 0 {
            raw_line.strip_prefix(&path_str).unwrap_or(raw_line).trim()
        } else {
            raw_line.trim()
        };
        if line.is_empty() || line.starts_with("Successfully processed") {
            break;
        }
        if let Some(colon_idx) = line.find(":(") {
            grantees.push(line[..colon_idx].trim().to_string());
        }
    }

    const TRUSTED_SYSTEM_ACCOUNTS: [&str; 2] = ["SYSTEM", "Administrators"];

    let is_trusted = |g: &str| {
        let short_name = g.rsplit('\\').next().unwrap_or(g);
        short_name.eq_ignore_ascii_case(&username)
            || TRUSTED_SYSTEM_ACCOUNTS
                .iter()
                .any(|acct| short_name.eq_ignore_ascii_case(acct))
    };

    if grantees.is_empty() || !grantees.iter().all(|g| is_trusted(g)) {
        return Err(anyhow!(
            "Insecure permissions on token file: expected only '{username}' (or SYSTEM/Administrators) to have access, found {grantees:?}\nraw icacls output:\n{stdout}"
        ));
    }
    Ok(())
}

/// Timing-safe comparison of two byte slices.
/// Evaluates in constant time depending only on length.
pub fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }

    // Use a secondary check for completeness, but the logical OR chain is primary.
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_token_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let token_path = tmp.path().join(".session_token");
        let manager = SessionTokenManager::with_path(token_path);

        let token = manager.generate_and_save().unwrap();
        assert_eq!(token.len(), 64); // SHA-256 hex length is 64 characters

        // Validate correct token
        assert!(manager.validate_token(&token));

        // Validate wrong token
        assert!(!manager.validate_token("wrong_token"));

        manager.clear().unwrap();
        assert!(!manager.token_path.exists());
    }

    #[test]
    fn generate_secret_hex_produces_64_char_hex_string() {
        let secret = generate_secret_hex();
        assert_eq!(secret.len(), 64);
        assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_secret_hex_is_not_deterministic() {
        assert_ne!(generate_secret_hex(), generate_secret_hex());
    }

    #[test]
    fn test_token_uses_constant_time_comparison() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hell"));
    }

    // generate_and_save() already calls harden_file_perms(), so on Windows
    // test_token_lifecycle above exercises verify_windows_only_owner_can_read
    // via validate_token(). These two make the read-side check explicit.
    #[cfg(windows)]
    #[test]
    fn get_valid_token_accepts_a_hardened_file() {
        let tmp = TempDir::new().unwrap();
        let manager = SessionTokenManager::with_path(tmp.path().join(".session_token"));
        manager.generate_and_save().unwrap();

        if let Err(e) = manager.get_valid_token() {
            panic!("expected a freshly hardened file to be accepted, got: {e}");
        }
    }

    /// Deliberately grants a normal, non-privileged account access, rather
    /// than relying on whatever a fresh temp file's default ACL happens to
    /// be — that default already includes SYSTEM/Administrators (confirmed
    /// on GitHub's Windows runner), which are meant to pass, so a test
    /// relying on ambient defaults could pass for the wrong reason or start
    /// failing if the runner's default ACL ever changes.
    #[cfg(windows)]
    #[test]
    fn get_valid_token_rejects_a_file_with_an_extra_grantee() {
        let tmp = TempDir::new().unwrap();
        let token_path = tmp.path().join(".session_token");
        std::fs::write(&token_path, "not-hardened").unwrap();

        let status = std::process::Command::new("icacls")
            .arg(&token_path)
            .arg("/grant")
            .arg("Everyone:R")
            .status()
            .unwrap();
        assert!(status.success(), "test setup: failed to grant Everyone:R");

        let manager = SessionTokenManager::with_path(token_path);
        assert!(manager.get_valid_token().is_err());
    }
}
