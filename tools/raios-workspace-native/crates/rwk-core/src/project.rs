//! Project identity and the filesystem capability the workspace holds for one project.
//!
//! The UI only ever sees opaque [`ProjectId`]s and relative paths. A client-supplied absolute
//! path never grants access: the capability is created from a user-selected directory, and every
//! later operation is relative to it. Symbolic links are never followed out of the project.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cap_std::fs::{Dir, File, OpenOptions};

use crate::documents::{Document, Revision, MAX_FILE_BYTES};
use crate::error::{AppError, ErrorCode, Result};

/// Most entries one directory listing returns before it is refused.
const MAX_DIRECTORY_ENTRIES: usize = 2000;
/// Longest accepted project-relative path, in bytes.
const MAX_RELATIVE_PATH_BYTES: usize = 4096;
/// Directories the file tree never shows.
const SKIPPED_DIRECTORIES: [&str; 7] = [
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    ".cache",
    ".pnpm",
];

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
    dir: Dir,
}

impl ProjectRoot {
    /// Opens a user-selected directory and returns the capability for it.
    pub fn open(root: PathBuf) -> Result<Self> {
        let root = root.canonicalize().map_err(path_error)?;
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).map_err(path_error)?;
        let name = root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let info = ProjectInfo {
            id: ProjectId::new(uuid::Uuid::new_v4().to_string()),
            root,
            name,
        };
        Ok(Self { info, dir })
    }

    /// Information about this project.
    pub fn info(&self) -> &ProjectInfo {
        &self.info
    }

    /// Lists one directory level. `relative` is empty for the project root. Directories come
    /// first, names sort case-insensitively, symbolic links and build noise are skipped.
    pub fn list(&self, relative: &str) -> Result<Vec<FileEntry>> {
        validate_relative_path(relative, true)?;
        let dir = if relative.is_empty() {
            self.dir.try_clone()?
        } else {
            self.dir.open_dir(relative).map_err(path_error)?
        };
        let mut entries = Vec::new();
        for entry in dir.entries()? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                continue;
            }
            let file_type = entry.file_type()?;
            // Symlink targets are never traversed by the file tree.
            if file_type.is_symlink() || !(file_type.is_file() || file_type.is_dir()) {
                continue;
            }
            entries.push(FileEntry {
                path: if relative.is_empty() {
                    name.clone()
                } else {
                    format!("{relative}/{name}")
                },
                name,
                is_directory: file_type.is_dir(),
            });
            if entries.len() > MAX_DIRECTORY_ENTRIES {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "Directory contains too many entries; open a narrower project.",
                ));
            }
        }
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    /// Reads a regular UTF-8 file of at most [`MAX_FILE_BYTES`] and returns it with its revision.
    pub fn read_document(&self, relative: &str) -> Result<Document> {
        validate_relative_path(relative, false)?;
        let mut file = self.dir.open(relative).map_err(path_error)?;
        if !file.metadata()?.is_file() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Only regular files can be edited.",
            ));
        }
        let bytes = read_bounded(&mut file)?;
        let revision = Revision::of(&bytes);
        let content = String::from_utf8(bytes)
            .map_err(|_| AppError::new(ErrorCode::InvalidInput, "This file is not UTF-8 text."))?;
        Ok(Document {
            project_id: self.info.id.clone(),
            path: relative.into(),
            content,
            revision,
        })
    }

    /// Saves `edited.content` if `edited.revision` still matches the file on disk; otherwise
    /// fails with a conflict. Returns the document with its new revision.
    pub fn save_document(&self, edited: Document) -> Result<Document> {
        if edited.project_id != self.info.id {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Document belongs to another project.",
            ));
        }
        validate_relative_path(&edited.path, false)?;
        if edited.content.len() > MAX_FILE_BYTES {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Text files are limited to 2 MiB.",
            ));
        }
        // A capability-relative open follows no link outside the granted root. This exact file
        // handle is kept through the revision check and the write.
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        let mut file = self
            .dir
            .open_with(&edited.path, &options)
            .map_err(path_error)?;
        if !file.metadata()?.is_file() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Only regular files can be saved.",
            ));
        }
        if Revision::of(&read_bounded(&mut file)?) != edited.revision {
            return Err(conflict(
                "File changed on disk. Your edits are retained; reload or reconcile before saving.",
            ));
        }
        let path = Path::new(&edited.path);
        let parent = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) => self.dir.open_dir(parent).map_err(path_error)?,
            None => self.dir.try_clone()?,
        };
        let name = path
            .file_name()
            .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "Invalid file name."))?;
        let temporary = format!(".raios-save-{}", uuid::Uuid::new_v4());
        let outcome = replace_file(&parent, &temporary, name, &file, &edited);
        if outcome.is_err() {
            let _ = parent.remove_file(&temporary);
        }
        outcome?;
        Ok(Document {
            revision: Revision::of(edited.content.as_bytes()),
            ..edited
        })
    }

    /// Unstaged `git diff` of one file.
    pub fn git_diff(&self, relative: &str) -> Result<String> {
        validate_relative_path(relative, false)?;
        // `--` and `--literal-pathspecs` make the path a plain pathspec even when it starts
        // with `-`; external diff drivers, text conversion and fsmonitor hooks stay off.
        let mut child = Command::new("git")
            .args([
                "--no-pager",
                "--literal-pathspecs",
                "-c",
                "core.fsmonitor=false",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                relative,
            ])
            .current_dir(&self.info.root)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => {
                    AppError::new(ErrorCode::Io, "Git is not installed.")
                }
                _ => AppError::from(error),
            })?;
        let mut bytes = Vec::new();
        let Some(stdout) = child.stdout.take() else {
            return Err(AppError::new(ErrorCode::Io, "Git output unavailable."));
        };
        stdout
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Diff is larger than 2 MiB.",
            ));
        }
        if !child.wait()?.success() {
            return Err(AppError::new(
                ErrorCode::Io,
                "Git diff failed; check whether this folder is a repository.",
            ));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Largest file the workspace will read or save, in bytes.
    pub const fn max_file_bytes() -> usize {
        MAX_FILE_BYTES
    }
}

/// Writes the new content next to the target, re-checks the target's revision, and renames the
/// temporary file over it so a reader never sees a half-written file.
fn replace_file(
    parent: &Dir,
    temporary: &str,
    name: &OsStr,
    original: &File,
    edited: &Document,
) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut replacement = parent.open_with(temporary, &options)?;
    replacement.set_permissions(original.metadata()?.permissions())?;
    replacement.write_all(edited.content.as_bytes())?;
    replacement.sync_all()?;
    // Check the current pathname again before the atomic replacement.
    let mut current = parent.open(name).map_err(path_error)?;
    if Revision::of(&read_bounded(&mut current)?) != edited.revision {
        return Err(conflict(
            "File changed during save; edits were not applied.",
        ));
    }
    parent.rename(temporary, parent, name)?;
    Ok(())
}

/// Validates a project-relative path. Rejects absolute paths, `.` and `..` components, empty
/// components (so `a//b` and a trailing `/`), NUL and backslashes. `allow_empty` permits the
/// empty string, meaning the project root itself.
pub fn validate_relative_path(relative: &str, allow_empty: bool) -> Result<()> {
    if relative.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(invalid_path())
        };
    }
    let acceptable = relative.len() <= MAX_RELATIVE_PATH_BYTES
        && !relative.contains(['\0', '\\'])
        && relative
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if acceptable {
        Ok(())
    } else {
        Err(invalid_path())
    }
}

fn invalid_path() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "Use a relative path within the selected project.",
    )
}

fn conflict(message: &str) -> AppError {
    AppError::new(ErrorCode::Conflict, message)
}

/// Maps an I/O error from a capability-relative operation: a missing path is `NotFound`;
/// everything else, including an attempt to leave the project, is invalid input.
fn path_error(error: std::io::Error) -> AppError {
    let code = if error.kind() == std::io::ErrorKind::NotFound {
        ErrorCode::NotFound
    } else {
        ErrorCode::InvalidInput
    };
    AppError::new(code, error.to_string())
}

/// Reads at most [`MAX_FILE_BYTES`], failing instead of silently truncating.
fn read_bounded(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Text files are limited to 2 MiB.",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
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

    #[test]
    fn an_over_long_path_is_refused() {
        let long = "a/".repeat(MAX_RELATIVE_PATH_BYTES) + "b";
        assert_eq!(
            validate_relative_path(&long, false).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}
