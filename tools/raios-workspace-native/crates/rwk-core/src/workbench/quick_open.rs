//! Bounded project-wide filename discovery using the project's filesystem capability.

use crate::error::Result;
use crate::project::ProjectRoot;
use std::collections::VecDeque;

/// A project-relative file list and whether the traversal budget was exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListing {
    /// Files eligible under the same rules as the file tree.
    pub files: Vec<String>,
    /// More entries may exist beyond the requested traversal limit.
    pub truncated: bool,
}

/// Recursively list files without following symlinks or bypassing ProjectRoot exclusions.
/// Every visited directory or file consumes one unit of `max_entries`.
pub fn list_project_files(project: &ProjectRoot, max_entries: usize) -> Result<FileListing> {
    let mut directories = VecDeque::from([String::new()]);
    let mut files = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    'walk: while let Some(directory) = directories.pop_front() {
        for entry in project.list(&directory)? {
            if visited >= max_entries {
                truncated = true;
                break 'walk;
            }
            visited += 1;
            if entry.is_directory {
                directories.push_back(entry.path);
            } else {
                files.push(entry.path);
            }
        }
    }
    files.sort();
    Ok(FileListing { files, truncated })
}

/// Match every query word against the filename, retaining paths to disambiguate names.
/// The displayed result count is bounded independently of the discovery budget.
pub fn matching_files(files: &[String], query: &str, max_results: usize) -> Vec<String> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    files
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
            words.iter().all(|word| name.contains(word))
        })
        .take(max_results)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_shares_tree_exclusions_and_never_follows_symlinks() {
        let dir = tempfile::tempdir().expect("project");
        std::fs::create_dir(dir.path().join("src")).expect("src");
        std::fs::write(dir.path().join("src/main.rs"), "main").expect("main");
        std::fs::create_dir(dir.path().join("target")).expect("target");
        std::fs::write(dir.path().join("target/noise.rs"), "noise").expect("noise");
        #[cfg(unix)]
        std::os::unix::fs::symlink("src", dir.path().join("loop")).expect("symlink");
        let project = ProjectRoot::open(dir.path().to_path_buf()).expect("open");
        assert_eq!(
            list_project_files(&project, 10).expect("list"),
            FileListing {
                files: vec!["src/main.rs".into()],
                truncated: false,
            }
        );
        assert!(list_project_files(&project, 1).expect("bounded").truncated);
        assert!(list_project_files(&project, 0)
            .expect("zero")
            .files
            .is_empty());
    }

    #[test]
    fn filename_filter_keeps_duplicate_paths_and_bounds_results() {
        let files = vec![
            "other/main.rs".into(),
            "src/main.rs".into(),
            "main.rs/unrelated.txt".into(),
        ];
        assert_eq!(matching_files(&files, "MAIN rs", 10), files[..2]);
        assert_eq!(matching_files(&files, "main", 1), files[..1]);
        assert!(matching_files(&files, "no-match", 10).is_empty());
    }
}
