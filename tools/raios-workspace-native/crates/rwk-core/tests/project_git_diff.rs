//! `ProjectRoot::git_diff` against real temporary git repositories.

use std::fs;
use std::path::Path;
use std::process::Command;

use rwk_core::error::ErrorCode;
use rwk_core::project::ProjectRoot;
use tempfile::TempDir;

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(directory)
        .status()
        .expect("git must be installed to run this test");
    assert!(status.success(), "git {args:?} failed");
}

/// A repository with `a.txt` committed as "one\n".
fn repository() -> (TempDir, ProjectRoot) {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-q"]);
    fs::write(directory.path().join("a.txt"), "one\n").unwrap();
    git(directory.path(), &["add", "a.txt"]);
    git(directory.path(), &["commit", "-q", "-m", "initial"]);
    let root = ProjectRoot::open(directory.path().to_path_buf()).unwrap();
    (directory, root)
}

#[test]
fn an_unchanged_file_has_an_empty_diff() {
    let (_directory, root) = repository();
    assert_eq!(root.git_diff("a.txt").unwrap(), "");
}

#[test]
fn a_modified_file_shows_the_unstaged_change() {
    let (directory, root) = repository();
    fs::write(directory.path().join("a.txt"), "two\n").unwrap();

    let diff = root.git_diff("a.txt").unwrap();
    assert!(diff.contains("-one"), "{diff}");
    assert!(diff.contains("+two"), "{diff}");
}

#[test]
fn staged_changes_are_not_part_of_the_unstaged_diff() {
    let (directory, root) = repository();
    fs::write(directory.path().join("a.txt"), "two\n").unwrap();
    git(directory.path(), &["add", "a.txt"]);

    assert_eq!(root.git_diff("a.txt").unwrap(), "");
}

#[test]
fn a_path_that_looks_like_an_option_is_only_ever_a_pathspec() {
    let (directory, root) = repository();

    let diff = root.git_diff("--output=pwned.txt").unwrap();

    assert_eq!(diff, "");
    assert!(!directory.path().join("pwned.txt").exists());
}

#[test]
fn bad_paths_are_rejected_before_git_runs() {
    let (_directory, root) = repository();
    for bad in ["", "../outside", "/etc/passwd", "a//b"] {
        let error = root.git_diff(bad).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{bad:?}");
    }
}

#[test]
fn a_folder_that_is_not_a_repository_reports_a_git_error() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("a.txt"), "x").unwrap();
    let root = ProjectRoot::open(directory.path().to_path_buf()).unwrap();

    let error = root.git_diff("a.txt").unwrap_err();

    assert_eq!(error.code, ErrorCode::Io);
    assert!(error.message.contains("repository"), "{}", error.message);
}

#[test]
fn a_diff_larger_than_two_mebibytes_is_refused() {
    let (directory, root) = repository();
    let big = "x\n".repeat(1_600_000);
    fs::write(directory.path().join("a.txt"), big).unwrap();

    let error = root.git_diff("a.txt").unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("2 MiB"), "{}", error.message);
}
