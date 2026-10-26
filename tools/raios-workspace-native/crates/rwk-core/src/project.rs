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

    /// Creates an empty file `name` inside folder `parent` (empty for the project root) and
    /// returns its project-relative path. An existing entry is never replaced.
    pub fn create_file(&self, parent: &str, name: &str) -> Result<String> {
        let (dir, path) = self.entry_in(parent, name)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        dir.open_with(name, &options).map_err(entry_error)?;
        Ok(path)
    }

    /// Creates folder `name` inside folder `parent` and returns its project-relative path.
    pub fn create_folder(&self, parent: &str, name: &str) -> Result<String> {
        let (dir, path) = self.entry_in(parent, name)?;
        dir.create_dir(name).map_err(entry_error)?;
        Ok(path)
    }

    /// Gives the file or folder at `path` the name `new_name` in the same folder and returns
    /// its new path. An existing entry is never replaced.
    pub fn rename_entry(&self, path: &str, new_name: &str) -> Result<String> {
        validate_relative_path(path, false)?;
        let (parent, old_name) = split_path(path);
        let (dir, new_path) = self.entry_in(parent, new_name)?;
        if old_name == new_name {
            return Ok(new_path);
        }
        dir.symlink_metadata(old_name).map_err(path_error)?;
        // The check and the rename are two steps; a file created in between is the one risk, and
        // this is a single-user tool that holds the only handle on the project.
        if dir.symlink_metadata(new_name).is_ok() {
            return Err(exists());
        }
        dir.rename(old_name, &dir, new_name).map_err(entry_error)?;
        Ok(new_path)
    }

    /// Deletes the file (or symbolic link) at `path`, or the folder when it is empty. A folder
    /// with anything in it is refused: nothing is ever removed recursively.
    pub fn delete_entry(&self, path: &str) -> Result<()> {
        validate_relative_path(path, false)?;
        let (parent, name) = split_path(path);
        let dir = self.open_parent(parent)?;
        let metadata = dir.symlink_metadata(name).map_err(path_error)?;
        if metadata.is_dir() {
            dir.remove_dir(name).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    path_error(error)
                } else {
                    AppError::new(
                        ErrorCode::InvalidInput,
                        "Klasör boş değil; içindekileri tek tek silin.",
                    )
                }
            })
        } else {
            dir.remove_file(name).map_err(path_error)
        }
    }

    fn open_parent(&self, parent: &str) -> Result<Dir> {
        validate_relative_path(parent, true)?;
        if parent.is_empty() {
            Ok(self.dir.try_clone()?)
        } else {
            self.dir.open_dir(parent).map_err(path_error)
        }
    }

    /// The folder `parent` and the project-relative path of `name` inside it, after checking
    /// that `name` is a single acceptable name.
    fn entry_in(&self, parent: &str, name: &str) -> Result<(Dir, String)> {
        validate_entry_name(name)?;
        let dir = self.open_parent(parent)?;
        let path = if parent.is_empty() {
            name.to_owned()
        } else {
            format!("{parent}/{name}")
        };
        Ok((dir, path))
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

/// Longest file or folder name, in bytes (the limit of common filesystems).
const MAX_ENTRY_NAME_BYTES: usize = 255;

/// Validates the name of one new or renamed entry: a single component, not empty, not `.` or
/// `..`, no separators, NUL or control characters, at most 255 bytes, not the temporary-file
/// prefix the saver uses, and not a folder the tree hides (it would vanish from view).
pub fn validate_entry_name(name: &str) -> Result<()> {
    let refuse = |message: &str| Err(AppError::new(ErrorCode::InvalidInput, message));
    if name.is_empty() || name.trim() != name {
        return refuse("Ad boş olamaz ve başında ya da sonunda boşluk bulunamaz.");
    }
    if name == "." || name == ".." || name.contains(['/', '\\']) {
        return refuse("Ad tek bir parça olmalı: '/', '\\', '.' ve '..' kullanılamaz.");
    }
    if name.chars().any(char::is_control) {
        return refuse("Ad denetim karakteri içeremez.");
    }
    if name.len() > MAX_ENTRY_NAME_BYTES {
        return refuse("Ad en fazla 255 bayt olabilir.");
    }
    if name.starts_with(".raios-save-") || SKIPPED_DIRECTORIES.contains(&name) {
        return refuse("Bu ad ayrılmış; dosya ağacında görünmezdi.");
    }
    Ok(())
}

/// Splits a validated relative path into its folder (empty for the root) and its last name.
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", path),
    }
}

fn exists() -> AppError {
    AppError::new(ErrorCode::Conflict, "Bu adla bir kayıt zaten var.")
}

/// Like [`path_error`], with an existing entry reported as a conflict.
fn entry_error(error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        exists()
    } else {
        path_error(error)
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

    fn project() -> (tempfile::TempDir, ProjectRoot) {
        let dir = tempfile::tempdir().unwrap();
        let root = ProjectRoot::open(dir.path().to_path_buf()).unwrap();
        (dir, root)
    }

    #[test]
    fn entry_names_are_single_plain_components() {
        for good in ["a.txt", ".env", "ünï.rs", "with space.md", "a-b_c"] {
            assert!(validate_entry_name(good).is_ok(), "{good}");
        }
        let long = "x".repeat(256);
        for bad in [
            "",
            " a",
            "a ",
            ".",
            "..",
            "a/b",
            "a\\b",
            "nul\0x",
            "tab\tx",
            "line\nbreak",
            ".raios-save-1",
            "target",
            ".git",
            long.as_str(),
        ] {
            assert!(validate_entry_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn files_and_folders_are_created_renamed_and_deleted_inside_the_project() {
        let (dir, root) = project();
        assert_eq!(root.create_folder("", "src").unwrap(), "src");
        assert_eq!(root.create_file("src", "main.rs").unwrap(), "src/main.rs");
        assert!(dir.path().join("src/main.rs").is_file());
        // An existing name is a conflict and the file is untouched.
        std::fs::write(dir.path().join("src/main.rs"), "keep").unwrap();
        assert_eq!(
            root.create_file("src", "main.rs").unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "keep"
        );

        assert_eq!(
            root.rename_entry("src/main.rs", "lib.rs").unwrap(),
            "src/lib.rs"
        );
        assert!(!dir.path().join("src/main.rs").exists());
        assert!(dir.path().join("src/lib.rs").is_file());
        // A rename never replaces another entry.
        root.create_file("src", "other.rs").unwrap();
        assert_eq!(
            root.rename_entry("src/other.rs", "lib.rs")
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert!(root.rename_entry("src/missing.rs", "x.rs").is_err());

        // A folder with content is refused, an emptied one goes.
        assert!(root.delete_entry("src").is_err());
        assert!(dir.path().join("src/lib.rs").exists());
        root.delete_entry("src/lib.rs").unwrap();
        root.delete_entry("src/other.rs").unwrap();
        root.delete_entry("src").unwrap();
        assert!(!dir.path().join("src").exists());
    }

    #[test]
    fn file_operations_cannot_leave_the_project_or_touch_the_root() {
        let (dir, root) = project();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("victim.txt"), "x").unwrap();
        for parent in ["..", "../x", "/etc", "a/../b"] {
            assert!(root.create_file(parent, "f").is_err(), "{parent}");
        }
        for path in ["", "..", "../victim.txt", "/etc/passwd"] {
            assert!(root.delete_entry(path).is_err(), "{path:?}");
            assert!(root.rename_entry(path, "n").is_err(), "{path:?}");
        }
        // A symbolic link out of the project is removed as a link; its target stays.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
            assert!(root.create_file("out", "planted").is_err());
            root.delete_entry("out").unwrap();
            assert!(!dir.path().join("out").exists());
            assert!(outside.path().join("victim.txt").exists());
            assert!(!outside.path().join("planted").exists());
        }
    }

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
