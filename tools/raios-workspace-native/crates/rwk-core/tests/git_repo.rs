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

#[test]
fn one_hunk_can_be_staged_and_unstaged_alone() {
    let dir = repository();
    let root = dir.path();
    let original: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    std::fs::write(root.join("f.txt"), &original).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);

    // Two changes far enough apart to be two hunks.
    let edited = original
        .replace("line 2\n", "line two\n")
        .replace("line 28\n", "line twenty-eight\n");
    std::fs::write(root.join("f.txt"), &edited).unwrap();

    let diff = git::file_diff(root, "f.txt", false).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    assert!(git::file_diff(root, "f.txt", true)
        .unwrap()
        .hunks
        .is_empty());

    // A stale header is refused and changes nothing.
    assert!(git::apply_hunk(root, "f.txt", 0, "@@ -9,9 +9,9 @@", true).is_err());
    assert!(git::file_diff(root, "f.txt", true)
        .unwrap()
        .hunks
        .is_empty());

    git::apply_hunk(root, "f.txt", 1, &diff.hunks[1].header, true).unwrap();
    let staged = git::file_diff(root, "f.txt", true).unwrap();
    assert_eq!(staged.hunks.len(), 1);
    assert!(staged.hunks[0]
        .lines
        .iter()
        .any(|line| line == "+line twenty-eight"));
    // The other change is still only in the working tree.
    let rest = git::file_diff(root, "f.txt", false).unwrap();
    assert_eq!(rest.hunks.len(), 1);
    assert!(rest.hunks[0].lines.iter().any(|line| line == "+line two"));

    git::apply_hunk(root, "f.txt", 0, &staged.hunks[0].header, false).unwrap();
    assert!(git::file_diff(root, "f.txt", true)
        .unwrap()
        .hunks
        .is_empty());
    assert_eq!(git::file_diff(root, "f.txt", false).unwrap().hunks.len(), 2);
    assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), edited);
}

#[test]
fn branches_stash_pull_and_push_work_against_a_local_remote() {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let dir = repository();
    let root = dir.path();
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);

    // No upstream yet: push sets one on origin, and a second push just pushes.
    assert_eq!(git::upstream(root), None);
    git::push(root).unwrap();
    assert_eq!(git::upstream(root).as_deref(), Some("origin/main"));
    std::fs::write(root.join("a.txt"), "two\n").unwrap();
    git(root, &["commit", "-q", "-am", "second"]);
    git::push(root).unwrap();

    // A second clone moves the remote ahead; pull fast-forwards, a diverged branch is refused.
    let other = tempfile::tempdir().unwrap();
    git(
        other.path(),
        &["clone", "-q", remote.path().to_str().unwrap(), "."],
    );
    git(other.path(), &["config", "user.email", "t@example.com"]);
    git(other.path(), &["config", "user.name", "T"]);
    git(other.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(other.path().join("b.txt"), "b\n").unwrap();
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-q", "-m", "remote work"]);
    git(other.path(), &["push", "-q"]);
    git::pull(root).unwrap();
    assert!(root.join("b.txt").exists());
    std::fs::write(root.join("c.txt"), "c\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "local work"]);
    std::fs::write(other.path().join("d.txt"), "d\n").unwrap();
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-q", "-m", "more remote work"]);
    git(other.path(), &["push", "-q"]);
    assert!(git::pull(root).is_err(), "diverged branches are not merged");

    // Stash takes untracked files too and pop brings them back; an empty stash is an error.
    assert!(git::stash_push(root, "nothing").is_err());
    std::fs::write(root.join("a.txt"), "dirty\n").unwrap();
    std::fs::write(root.join("loose.txt"), "x").unwrap();
    git::stash_push(root, "work in progress").unwrap();
    assert!(!root.join("loose.txt").exists());
    let stashes = git::stash_list(root).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("work in progress"));
    git::stash_pop(root, 0).unwrap();
    assert!(root.join("loose.txt").exists());
    assert!(git::stash_list(root).unwrap().is_empty());

    // Branches: create switches to it, a bad name is refused, delete refuses the current one.
    git::create_branch(root, "feature/x").unwrap();
    assert_eq!(git::branches(root).unwrap()[0], "feature/x");
    assert!(git::create_branch(root, "bad name").is_err());
    assert!(git::create_branch(root, "-x").is_err());
    assert!(git::delete_branch(root, "feature/x").is_err());
    git::switch(root, "main").unwrap_or_else(|_| {
        git(root, &["stash", "-q", "-u"]);
        git::switch(root, "main").unwrap()
    });
    git::delete_branch(root, "feature/x").unwrap();
    assert_eq!(git::branches(root).unwrap(), ["main"]);
}

#[test]
fn blame_names_the_author_and_summary_of_a_committed_line() {
    let dir = repository();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "first"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();

    let old = git::blame_line(root, "a.txt", 1).unwrap();
    assert!(old.committed);
    assert_eq!((old.author.as_str(), old.summary.as_str()), ("T", "first"));
    assert!(!git::blame_line(root, "a.txt", 3).unwrap().committed);
}
