//! The git module against a real repository in a temporary folder.

use std::path::Path;
use std::process::Command;

use rwk_core::git::{self, Change};

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "T"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    dir
}

fn never(_: &str) -> bool {
    false
}

#[test]
fn stage_unstage_commit_discard_and_switch_work_on_a_real_repository() {
    let dir = repository();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    std::fs::write(root.join("b c.txt"), "two\n").unwrap();

    let status = git::status(root).unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.head, None, "no commit yet");
    assert!(status.entries.iter().all(|e| e.untracked));

    // Stage both, take one back out (this is the repository without a first commit).
    git::stage(root, &["a.txt".into(), "b c.txt".into()]).unwrap();
    git::unstage(root, &["b c.txt".into()]).unwrap();
    let status = git::status(root).unwrap();
    let staged: Vec<_> = status
        .staged()
        .map(|e| (e.path.as_str(), e.staged))
        .collect();
    assert_eq!(staged, [("a.txt", Change::Added)]);
    assert!(status.changes().any(|e| e.path == "b c.txt" && e.untracked));

    assert!(git::commit(root, "", &never).is_err());
    git::commit(root, "first", &never).unwrap();
    assert!(git::status(root).unwrap().head.is_some());

    // A tracked change can be staged, unstaged and discarded; an untracked file is deleted.
    std::fs::write(root.join("a.txt"), "changed\n").unwrap();
    let status = git::status(root).unwrap();
    assert_eq!(status.changes().next().unwrap().unstaged, Change::Modified);
    git::stage(root, &["a.txt".into()]).unwrap();
    assert_eq!(git::status(root).unwrap().staged().count(), 1);
    git::unstage(root, &["a.txt".into()]).unwrap();
    assert_eq!(git::status(root).unwrap().staged().count(), 0);
    let status = git::status(root).unwrap();
    // The modified a.txt and the untracked b c.txt are both in the list.
    let everything: Vec<_> = status.changes().cloned().collect();
    assert_eq!(everything.len(), 2);
    git::discard(root, &everything).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(!root.join("b c.txt").exists());
    assert!(git::status(root).unwrap().entries.is_empty());

    // Branches: the current one is first; switching changes it; a bad name is refused.
    git(root, &["branch", "topic"]);
    assert_eq!(git::branches(root).unwrap(), ["main", "topic"]);
    git::switch(root, "topic").unwrap();
    assert_eq!(git::branches(root).unwrap()[0], "topic");
    assert_eq!(git::status(root).unwrap().branch.as_deref(), Some("topic"));
    assert!(git::switch(root, "--detach").is_err());
    assert!(git::switch(root, "missing").is_err());
}

#[test]
fn a_folder_that_is_not_a_repository_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    // A temporary folder can still sit inside a repository (TMPDIR in a checkout); stop git
    // from looking upward.
    std::env::set_var("GIT_CEILING_DIRECTORIES", dir.path().parent().unwrap());
    assert!(git::status(dir.path()).is_err());
}
