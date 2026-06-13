use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Maximum age of a token before it expires (8 hours)
const TOKEN_MAX_AGE: Duration = Duration::from_secs(8 * 60 * 60);

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

    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self { token_path: path }
    }

    /// Generates a new secure random session token, saves it to disk with owner-only permissions.
    pub fn generate_and_save(&self) -> Result<String> {
        // Create config dir if not exists
        if let Some(parent) = self.token_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Generate 32 bytes of secure random hex string using system time, pid, and counter as entropy source,
        // hashed via SHA-256 for a cryptographically strong 256-bit key.
        let mut hasher = Sha256::new();
        hasher.update(uuid::Uuid::new_v4().as_bytes());
        hasher.update(format!("{}", std::process::id()).as_bytes());
        hasher.update(format!("{:?}", SystemTime::now()).as_bytes());
        let token = format!("{:x}", hasher.finalize());

        // Write token to file
        fs::write(&self.token_path, &token)?;

        // Set owner-only permissions (chmod 600) on Unix-like systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&self.token_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.token_path, perms)?;
        }

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

/// Timing-safe comparison of two byte slices.
/// Evaluates in constant time depending only on length.
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
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
    fn test_token_uses_constant_time_comparison() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hell"));
    }
}
