//! Document value types. The file operations live on [`crate::project::ProjectRoot`].

use sha2::{Digest, Sha256};

use crate::project::ProjectId;

/// Largest regular file the workspace reads or saves, in bytes (2 MiB).
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// SHA-256 hex digest of a document's contents, used for optimistic conflict detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision(String);

impl Revision {
    /// Wraps a lowercase hex digest.
    pub fn new(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }

    /// Computes the revision of `bytes`: their lowercase hex SHA-256 digest.
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }

    /// The digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A UTF-8 text file read from, or about to be written to, a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Project the document belongs to.
    pub project_id: ProjectId,
    /// Path relative to the project root, using `/` separators.
    pub path: String,
    /// Full text contents.
    pub content: String,
    /// Revision of the contents this document was read at (or is based on, when saving).
    pub revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_compare_by_digest() {
        assert_eq!(Revision::new("ab12"), Revision::new("ab12"));
        assert_ne!(Revision::new("ab12"), Revision::new("ab13"));
        assert_eq!(Revision::new("ab12").as_str(), "ab12");
    }

    #[test]
    fn revisions_are_the_sha256_of_the_bytes() {
        assert_eq!(
            Revision::of(b"").as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Revision::of(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_one_byte_change_changes_the_revision() {
        assert_ne!(
            Revision::of(b"fn main() {}"),
            Revision::of(b"fn main() {} ")
        );
    }
}
