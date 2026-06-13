use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const PIN_FILE: &str = ".raios_tool_pin";

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ToolPinError {
    HashMismatch { expected: String, actual: String },
    Io(std::io::Error),
}

impl std::fmt::Display for ToolPinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HashMismatch { expected, actual } => write!(
                f,
                "tool_pin: manifest hash mismatch — expected {expected}, got {actual}. \
                 MCP tool list changed since last pin. Run `raios pin-reset` to re-pin."
            ),
            Self::Io(e) => write!(f, "tool_pin: I/O error: {e}"),
        }
    }
}

impl From<std::io::Error> for ToolPinError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ─── Hash ────────────────────────────────────────────────────────────────────

pub fn hash_manifest(manifest_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(manifest_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ─── Path resolution ─────────────────────────────────────────────────────────

fn local_pin_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(PIN_FILE)
}

fn config_pin_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("raios").join(PIN_FILE))
}

#[cfg(test)]
fn find_existing_pin_in(base: &Path) -> Option<PathBuf> {
    let p = base.join(PIN_FILE);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn find_existing_pin() -> Option<PathBuf> {
    let local = local_pin_path();
    if local.exists() {
        return Some(local);
    }
    config_pin_path().filter(|p| p.exists())
}

fn write_pin_file(path: &Path, hash: &str) -> Result<(), ToolPinError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, hash)?;
    Ok(())
}

// ─── Core logic (path-explicit for testability) ───────────────────────────────

#[cfg(test)]
fn verify_or_pin_in(base: &Path, manifest_json: &str) -> Result<bool, ToolPinError> {
    let actual = hash_manifest(manifest_json);
    let pin_path = base.join(PIN_FILE);
    match find_existing_pin_in(base) {
        None => {
            write_pin_file(&pin_path, &actual)?;
            Ok(true)
        }
        Some(path) => {
            let stored = std::fs::read_to_string(&path).map(|s| s.trim().to_string())?;
            if stored == actual {
                Ok(false)
            } else {
                Err(ToolPinError::HashMismatch {
                    expected: stored,
                    actual,
                })
            }
        }
    }
}

#[cfg(test)]
fn reset_pin_in(base: &Path) -> Result<(), ToolPinError> {
    let p = base.join(PIN_FILE);
    if p.exists() {
        std::fs::remove_file(&p)?;
    }
    Ok(())
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// First call writes the pin. Subsequent calls verify it.
/// Returns `Ok(true)` when pin was freshly written, `Ok(false)` when verified OK.
pub fn verify_or_pin(manifest_json: &str) -> Result<bool, ToolPinError> {
    let actual = hash_manifest(manifest_json);
    match find_existing_pin() {
        None => {
            write_pin_file(&local_pin_path(), &actual)?;
            Ok(true)
        }
        Some(path) => {
            let stored = std::fs::read_to_string(&path).map(|s| s.trim().to_string())?;
            if stored == actual {
                Ok(false)
            } else {
                Err(ToolPinError::HashMismatch {
                    expected: stored,
                    actual,
                })
            }
        }
    }
}

/// Delete all pin files so the next startup re-pins.
pub fn reset_pin() -> Result<(), ToolPinError> {
    let mut removed = false;
    let local = local_pin_path();
    if local.exists() {
        std::fs::remove_file(&local)?;
        removed = true;
    }
    if let Some(cfg) = config_pin_path() {
        if cfg.exists() {
            std::fs::remove_file(&cfg)?;
            removed = true;
        }
    }
    if !removed {
        eprintln!("No pin file found — nothing to reset.");
    }
    Ok(())
}

/// Read the currently stored hash without verifying.
pub fn current_pin() -> Option<String> {
    find_existing_pin()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn hash_is_deterministic() {
        let a = hash_manifest(r#"{"tools":["a","b"]}"#);
        let b = hash_manifest(r#"{"tools":["a","b"]}"#);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn different_manifests_differ() {
        let a = hash_manifest(r#"{"tools":["a"]}"#);
        let b = hash_manifest(r#"{"tools":["b"]}"#);
        assert_ne!(a, b);
    }

    #[test]
    fn first_call_writes_pin() {
        let tmp = TempDir::new().unwrap();
        let result = verify_or_pin_in(tmp.path(), r#"{"tools":["list_projects"]}"#).unwrap();
        assert!(result, "first call should return true (pin written)");
        assert!(tmp.path().join(PIN_FILE).exists());
    }

    #[test]
    fn second_call_same_manifest_ok() {
        let tmp = TempDir::new().unwrap();
        let manifest = r#"{"tools":["list_projects","git_status"]}"#;
        verify_or_pin_in(tmp.path(), manifest).unwrap();
        let result = verify_or_pin_in(tmp.path(), manifest).unwrap();
        assert!(!result, "second call should return false (verified)");
    }

    #[test]
    fn changed_manifest_returns_mismatch_error() {
        let tmp = TempDir::new().unwrap();
        verify_or_pin_in(tmp.path(), r#"{"tools":["list_projects"]}"#).unwrap();
        let err =
            verify_or_pin_in(tmp.path(), r#"{"tools":["list_projects","evil_tool"]}"#).unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("tool_pin:"));
        assert!(msg.contains("mismatch"));
    }

    #[test]
    fn reset_removes_pin_file() {
        let tmp = TempDir::new().unwrap();
        verify_or_pin_in(tmp.path(), r#"{"tools":["x"]}"#).unwrap();
        assert!(tmp.path().join(PIN_FILE).exists());
        reset_pin_in(tmp.path()).unwrap();
        assert!(!tmp.path().join(PIN_FILE).exists());
    }

    #[test]
    fn after_reset_next_call_re_pins() {
        let tmp = TempDir::new().unwrap();
        verify_or_pin_in(tmp.path(), r#"{"tools":["x"]}"#).unwrap();
        reset_pin_in(tmp.path()).unwrap();
        let result = verify_or_pin_in(tmp.path(), r#"{"tools":["x","y"]}"#).unwrap();
        assert!(result, "after reset, first call should re-pin");
        let stored = fs::read_to_string(tmp.path().join(PIN_FILE)).unwrap();
        assert_eq!(stored.trim(), hash_manifest(r#"{"tools":["x","y"]}"#));
    }
}
