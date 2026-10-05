//! Project identity and the filesystem capability the workspace holds for one project.
//!
//! The UI only ever sees opaque [`ProjectId`]s and relative paths. A client-supplied absolute
//! path never grants access: the capability is created from a user-selected directory.

use std::path::PathBuf;

use cap_std::fs::Dir;

use crate::documents::{Document, MAX_FILE_BYTES};
use crate::error::Result;

/// Opaque identifier of an opened project. Not derived from, and not usable as, a path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(String);

impl ProjectId {
    /// Wraps a generated identifier.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Display and registry information about an opened project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Opaque identifier.
    pub id: ProjectId,
    /// Canonical project root.
    pub root: PathBuf,
    /// Directory name shown in the UI.
    pub name: String,
}

/// One entry of a lazily loaded directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the project root, using `/` separators.
    pub path: String,
    /// Final path component.
    pub name: String,
    /// True for directories.
    pub is_directory: bool,
}

/// Capability to read and write inside one project directory, and nowhere else.
pub struct ProjectRoot {
    info: ProjectInfo,
    // Consumed by the P1b implementations of the file operations below.
    #[allow(dead_code)]
    dir: Dir,
}

impl ProjectRoot {
    /// Opens a user-selected directory and returns the capability for it.
    pub fn open(_root: PathBuf) -> Result<Self> {
        unimplemented!("P1b")
    }

    /// Information about this project.
    pub fn info(&self) -> &ProjectInfo {
        &self.info
    }

    /// Lists one directory level. `relative` is empty for the project root.
    pub fn list(&self, _relative: &str) -> Result<Vec<FileEntry>> {
        unimplemented!("P1b")
    }

    /// Reads a regular UTF-8 file of at most [`MAX_FILE_BYTES`] and returns it with its revision.
    pub fn read_document(&self, _relative: &str) -> Result<Document> {
        unimplemented!("P1b")
    }

    /// Saves `edited.content` if `edited.revision` still matches the file on disk; otherwise
    /// fails with a conflict. Returns the document with its new revision.
    pub fn save_document(&self, _edited: Document) -> Result<Document> {
        unimplemented!("P1b")
    }

    /// Unstaged `git diff` of one file.
    pub fn git_diff(&self, _relative: &str) -> Result<String> {
        unimplemented!("P1b")
    }

    /// Largest file the workspace will read or save, in bytes.
    pub const fn max_file_bytes() -> usize {
        MAX_FILE_BYTES
    }
}

/// Validates a project-relative path. Rejects absolute paths, `.` and `..` components, empty
/// components (so `a//b` and a trailing `/`), NUL and backslashes. `allow_empty` permits the
/// empty string, meaning the project root itself.
pub fn validate_relative_path(_relative: &str, _allow_empty: bool) -> Result<()> {
    unimplemented!("P1b")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn project_ids_compare_by_value_and_expose_their_text() {
        assert_eq!(ProjectId::new("p-1"), ProjectId::new("p-1"));
        assert_ne!(ProjectId::new("p-1"), ProjectId::new("p-2"));
        assert_eq!(ProjectId::new("p-1").as_str(), "p-1");
    }

    #[test]
    fn the_file_size_ceiling_is_two_mebibytes() {
        assert_eq!(ProjectRoot::max_file_bytes(), 2 * 1024 * 1024);
    }

    /// Executable specification of the path rules; enabled by Plan 3 (P1b).
    #[test]
    #[ignore = "P1b: executable spec for validate_relative_path"]
    fn relative_path_rules() {
        for ok in ["src/main.rs", "a/b/c.txt", "file", ".env", "dir/.hidden"] {
            assert!(validate_relative_path(ok, false).is_ok(), "{ok}");
        }
        assert!(validate_relative_path("", true).is_ok());
        assert_eq!(
            validate_relative_path("", false).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        for bad in [
            "/etc/passwd",
            "../outside",
            "a/../../b",
            "a//b",
            "a/./b",
            "nul\0byte",
            "a/",
            "C:\\windows",
        ] {
            assert_eq!(
                validate_relative_path(bad, true).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{bad:?}"
            );
        }
    }
}
