//! Content-addressed storage for Factory artifacts.
//!
//! Artifact bytes never enter SQLite. The control plane stores only the
//! resulting digest/path metadata through its existing `cp_artifacts` table.

use raios_core::product_factory::FactoryInvariantError;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MAX_FACTORY_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFactoryArtifact {
    pub sha256: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub media_type: String,
    pub content_ref: String,
}

#[derive(Debug, Clone)]
pub struct FactoryArtifactStore {
    root: PathBuf,
}

impl FactoryArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("artifacts")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_bytes(
        &self,
        bytes: &[u8],
        media_type: &str,
    ) -> Result<StoredFactoryArtifact, FactoryInvariantError> {
        if bytes.len() > MAX_FACTORY_ARTIFACT_BYTES {
            return Err(FactoryInvariantError::ArtifactRejected {
                reason: format!("artifact exceeds {} byte limit", MAX_FACTORY_ARTIFACT_BYTES),
            });
        }
        if media_type.trim().is_empty() || media_type.len() > 160 {
            return Err(FactoryInvariantError::ArtifactRejected {
                reason: "media type is missing or too long".into(),
            });
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            if raios_core::security::looks_like_secret(text).is_some() {
                return Err(FactoryInvariantError::ArtifactRejected {
                    reason: "content resembles a secret".into(),
                });
            }
        }

        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let directory = self.root.join(&sha256[..2]);
        let path = directory.join(&sha256);
        fs::create_dir_all(&directory)
            .map_err(|_| FactoryInvariantError::ArtifactStorageUnavailable)?;

        if !path.exists() {
            let temporary = directory.join(format!(".{}.tmp", Uuid::new_v4()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| FactoryInvariantError::ArtifactStorageUnavailable)?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| FactoryInvariantError::ArtifactStorageUnavailable)?;
            match fs::rename(&temporary, &path) {
                Ok(()) => {}
                Err(_) if path.exists() => {
                    let _ = fs::remove_file(&temporary);
                }
                Err(_) => {
                    let _ = fs::remove_file(&temporary);
                    return Err(FactoryInvariantError::ArtifactStorageUnavailable);
                }
            }
        }

        Ok(StoredFactoryArtifact {
            content_ref: format!("sha256:{sha256}"),
            sha256,
            path,
            size_bytes: bytes.len() as u64,
            media_type: media_type.into(),
        })
    }
}

impl Default for FactoryArtifactStore {
    fn default() -> Self {
        Self::new(Self::default_root())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_content_by_digest_without_duplicate_files() {
        let temporary = tempfile::tempdir().unwrap();
        let store = FactoryArtifactStore::new(temporary.path());
        let first = store
            .store_bytes(b"verified build report", "text/plain")
            .unwrap();
        let second = store
            .store_bytes(b"verified build report", "text/plain")
            .unwrap();

        assert_eq!(first.sha256, second.sha256);
        assert_eq!(first.path, second.path);
        assert_eq!(
            std::fs::read(&first.path).unwrap(),
            b"verified build report"
        );
        assert!(first.path.starts_with(temporary.path()));
        assert_eq!(first.content_ref, format!("sha256:{}", first.sha256));
    }

    #[test]
    fn rejects_secret_like_or_oversized_content() {
        let temporary = tempfile::tempdir().unwrap();
        let store = FactoryArtifactStore::new(temporary.path());
        assert!(matches!(
            store.store_bytes(b"AKIA1234567890ABCDEF", "text/plain"),
            Err(FactoryInvariantError::ArtifactRejected { .. })
        ));
        assert!(matches!(
            store.store_bytes(
                &vec![0; MAX_FACTORY_ARTIFACT_BYTES + 1],
                "application/octet-stream"
            ),
            Err(FactoryInvariantError::ArtifactRejected { .. })
        ));
    }
}
