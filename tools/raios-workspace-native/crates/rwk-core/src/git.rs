//! Git for the open project: status, stage, unstage, discard, commit and branches, through the
//! `git` executable. Every call is bounded (output size, run time), never goes through a shell,
//! never prompts, and takes paths only as pathspec arguments after `--`.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, ErrorCode, Result};
use crate::project::validate_relative_path;

/// Largest output of one git call, in bytes.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Longest a git call may run.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Most paths one stage, unstage or discard call takes.
pub const MAX_PATHS: usize = 2000;
/// Longest commit message, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;

/// What happened to a file, on one side (index or working tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing.
    Unchanged,
    /// Content changed.
    Modified,
    /// New file.
    Added,
    /// Removed.
    Deleted,
    /// Renamed (the entry carries the old path).
    Renamed,
    /// Copied.
    Copied,
    /// Type changed (file to symlink and so on).
    TypeChanged,
}

impl Change {
    fn of(letter: char) -> Self {
        match letter {
            'M' => Self::Modified,
            'A' => Self::Added,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'T' => Self::TypeChanged,
            _ => Self::Unchanged,
        }
    }

    /// The letter shown next to a file.
    pub fn letter(self) -> char {
        match self {
            Self::Unchanged => ' ',
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
        }
    }
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Project-relative path.
    pub path: String,
    /// The old path of a rename or copy.
    pub from: Option<String>,
    /// Change staged in the index.
    pub staged: Change,
    /// Change in the working tree not yet staged.
    pub unstaged: Change,
    /// Not tracked by git.
    pub untracked: bool,
    /// Has an unresolved merge conflict.
    pub conflicted: bool,
}

/// The state of the repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// The checked-out branch; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// The commit HEAD points at (short), `None` before the first commit.
    pub head: Option<String>,
    /// Every changed file, in git's order.
    pub entries: Vec<Entry>,
}

impl Status {
    /// Entries with something staged.
    pub fn staged(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.staged != Change::Unchanged && !entry.conflicted)
    }

    /// Entries with changes in the working tree that are not staged (untracked files included).
    pub fn changes(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| {
            entry.unstaged != Change::Unchanged || entry.untracked || entry.conflicted
        })
    }
}

/// Reads `git status --porcelain=v2 --branch -z` output.
pub fn parse_status(output: &str) -> Status {
    let mut status = Status::default();
    let mut fields = output.split('\0');
    while let Some(field) = fields.next() {
        if let Some(header) = field.strip_prefix("# ") {
            if let Some(name) = header.strip_prefix("branch.head ") {
                status.branch = (name != "(detached)").then(|| name.to_owned());
            } else if let Some(oid) = header.strip_prefix("branch.oid ") {
                status.head = (oid != "(initial)").then(|| oid.chars().take(7).collect());
            }
            continue;
        }
        let kind = field.chars().next();
        match kind {
            Some('1') => {
                let parts: Vec<&str> = field.splitn(9, ' ').collect();
                if let [_, xy, .., path] = parts.as_slice() {
                    status.entries.push(entry(xy, path, None));
                }
            }
            Some('2') => {
                let parts: Vec<&str> = field.splitn(10, ' ').collect();
                let from = fields.next().map(str::to_owned);
                if let [_, xy, .., path] = parts.as_slice() {
                    status.entries.push(entry(xy, path, from));
                }
            }
            Some('u') => {
                let parts: Vec<&str> = field.splitn(11, ' ').collect();
                if let [_, _, .., path] = parts.as_slice() {
                    status.entries.push(Entry {
                        path: (*path).to_owned(),
                        from: None,
                        staged: Change::Unchanged,
                        unstaged: Change::Unchanged,
                        untracked: false,
                        conflicted: true,
                    });
                }
            }
            Some('?') => {
                if let Some(path) = field.strip_prefix("? ") {
                    status.entries.push(Entry {
                        path: path.to_owned(),
                        from: None,
                        staged: Change::Unchanged,
                        unstaged: Change::Unchanged,
                        untracked: true,
                        conflicted: false,
                    });
                }
            }
            _ => {}
        }
    }
    status
}

fn entry(xy: &str, path: &str, from: Option<String>) -> Entry {
    let mut letters = xy.chars();
    Entry {
        path: path.to_owned(),
        from,
        staged: Change::of(letters.next().unwrap_or('.')),
        unstaged: Change::of(letters.next().unwrap_or('.')),
        untracked: false,
        conflicted: false,
    }
}

fn git_error(error: std::io::Error) -> AppError {
    match error.kind() {
        std::io::ErrorKind::NotFound => AppError::new(ErrorCode::Io, "Git is not installed."),
        _ => AppError::from(error),
    }
}

/// Runs `git <args>` in `root` and returns its standard output. `stdin` is written to the
/// child. A non-zero exit is an error carrying git's own first line of complaint.
fn run(root: &Path, args: &[&str], stdin: Option<&str>) -> Result<String> {
    let mut child = Command::new("git")
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(root)
        // Never wait on a credential, a pager or an editor; status must not take the index lock.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", "true")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(git_error)?;

    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A write error means git already exited; its exit status says why.
        let _ = pipe.write_all(text.as_bytes());
    }
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || read_bounded(stdout.as_mut()));
    let err_reader = std::thread::spawn(move || read_bounded(stderr.as_mut()));

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(ErrorCode::Io, "Git took too long."));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (out, too_big) = out_reader.join().unwrap_or_default();
    let (err, _) = err_reader.join().unwrap_or_default();
    if too_big {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Git output is too large.",
        ));
    }
    if !status.success() {
        let message = String::from_utf8_lossy(&err);
        let first = message
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Git failed.");
        return Err(AppError::new(ErrorCode::Io, first.trim().to_owned()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Reads at most [`MAX_OUTPUT_BYTES`]; the flag says the output was longer. The rest is
/// drained so the child never blocks on a full pipe.
fn read_bounded<R: Read>(reader: Option<&mut R>) -> (Vec<u8>, bool) {
    let Some(reader) = reader else {
        return (Vec::new(), false);
    };
    let mut bytes = Vec::new();
    let _ = reader
        .take(MAX_OUTPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes);
    let too_big = bytes.len() > MAX_OUTPUT_BYTES;
    bytes.truncate(MAX_OUTPUT_BYTES);
    let _ = std::io::copy(reader, &mut std::io::sink());
    (bytes, too_big)
}

fn check_paths(paths: &[String]) -> Result<()> {
    if paths.is_empty() || paths.len() > MAX_PATHS {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose between 1 and 2000 files.",
        ));
    }
    paths
        .iter()
        .try_for_each(|path| validate_relative_path(path, false))
}

fn with_paths<'a>(head: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    head.iter()
        .copied()
        .chain(std::iter::once("--"))
        .chain(paths.iter().map(String::as_str))
        .collect()
}

/// The repository's status. An error means `root` is not inside a repository (or git is
/// missing).
pub fn status(root: &Path) -> Result<Status> {
    let output = run(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
        None,
    )?;
    Ok(parse_status(&output))
}

/// Stages the given files (new, changed and deleted alike).
pub fn stage(root: &Path, paths: &[String]) -> Result<()> {
    check_paths(paths)?;
    run(root, &with_paths(&["add", "-A"], paths), None).map(drop)
}

/// Takes the given files out of the index; their working-tree content stays.
pub fn unstage(root: &Path, paths: &[String]) -> Result<()> {
    check_paths(paths)?;
    // Before the first commit there is no HEAD to reset to.
    if run(root, &["rev-parse", "--verify", "-q", "HEAD"], None).is_ok() {
        run(root, &with_paths(&["reset", "-q"], paths), None).map(drop)
    } else {
        run(
            root,
            &with_paths(&["rm", "-r", "--cached", "-q"], paths),
            None,
        )
        .map(drop)
    }
}

/// Throws away the working-tree changes of the given files: tracked files go back to the
/// index, untracked files are deleted. This cannot be undone; callers ask first.
pub fn discard(root: &Path, entries: &[Entry]) -> Result<()> {
    let tracked: Vec<String> = entries
        .iter()
        .filter(|entry| !entry.untracked)
        .map(|entry| entry.path.clone())
        .collect();
    let untracked: Vec<String> = entries
        .iter()
        .filter(|entry| entry.untracked)
        .map(|entry| entry.path.clone())
        .collect();
    if !tracked.is_empty() {
        check_paths(&tracked)?;
        run(root, &with_paths(&["restore"], &tracked), None)?;
    }
    if !untracked.is_empty() {
        check_paths(&untracked)?;
        run(root, &with_paths(&["clean", "-f", "-q"], &untracked), None)?;
    }
    Ok(())
}

/// Checks a commit message: not empty, not huge, nothing that looks like a credential.
pub fn check_message(message: &str, secret: &dyn Fn(&str) -> bool) -> Result<()> {
    if message.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Write a commit message.",
        ));
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The commit message is too long.",
        ));
    }
    if secret(message) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The commit message looks like it contains a secret.",
        ));
    }
    Ok(())
}

/// Commits what is staged. Hooks and signing run as the user configured them.
pub fn commit(root: &Path, message: &str, secret: &dyn Fn(&str) -> bool) -> Result<()> {
    check_message(message, secret)?;
    run(root, &["commit", "-q", "-F", "-"], Some(message)).map(drop)
}

/// The local branches, current one first.
pub fn branches(root: &Path) -> Result<Vec<String>> {
    let output = run(
        root,
        &[
            "for-each-ref",
            "--format=%(HEAD)%(refname:short)",
            "refs/heads",
        ],
        None,
    )?;
    let mut names: Vec<String> = output
        .lines()
        .map(|line| line.trim_start_matches('*').trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    let current = output
        .lines()
        .find_map(|line| line.strip_prefix('*').map(|name| name.trim().to_owned()));
    if let Some(current) = current {
        names.retain(|name| name != &current);
        names.insert(0, current);
    }
    Ok(names)
}

/// Switches to an existing local branch. Git itself refuses when local changes would be
/// overwritten.
pub fn switch(root: &Path, branch: &str) -> Result<()> {
    if branch.is_empty() || branch.starts_with('-') || branch.contains(['\0', '\n']) {
        return Err(AppError::new(ErrorCode::InvalidInput, "Not a branch name."));
    }
    run(root, &["switch", "-q", "--no-guess", branch], None).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_output_is_read_into_entries() {
        let output = "# branch.oid 0123456789abcdef\0# branch.head main\0\
            1 .M N... 100644 100644 100644 aaa bbb src/a.rs\0\
            1 A. N... 000000 100644 100644 000 ccc new file.rs\0\
            1 MD N... 100644 100644 000000 aaa bbb both.rs\0\
            2 R. N... 100644 100644 100644 aaa bbb R100 moved.rs\0old.rs\0\
            u UU N... 100644 100644 100644 100644 a b c clash.rs\0\
            ? loose.txt\0";
        let status = parse_status(output);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.head.as_deref(), Some("0123456"));
        let paths: Vec<_> = status.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "src/a.rs",
                "new file.rs",
                "both.rs",
                "moved.rs",
                "clash.rs",
                "loose.txt"
            ]
        );
        assert_eq!(status.entries[0].unstaged, Change::Modified);
        assert_eq!(status.entries[1].staged, Change::Added);
        assert_eq!(
            (status.entries[2].staged, status.entries[2].unstaged),
            (Change::Modified, Change::Deleted)
        );
        assert_eq!(status.entries[3].from.as_deref(), Some("old.rs"));
        assert!(status.entries[4].conflicted);
        assert!(status.entries[5].untracked);

        // Staged and changed lists overlap for a file with both kinds of change.
        let staged: Vec<_> = status.staged().map(|e| e.path.as_str()).collect();
        let changes: Vec<_> = status.changes().map(|e| e.path.as_str()).collect();
        assert_eq!(staged, ["new file.rs", "both.rs", "moved.rs"]);
        assert_eq!(changes, ["src/a.rs", "both.rs", "clash.rs", "loose.txt"]);
    }

    #[test]
    fn a_detached_head_and_a_fresh_repository_have_no_branch_or_head() {
        let status = parse_status("# branch.oid (initial)\0# branch.head (detached)\0");
        assert_eq!((status.branch, status.head), (None, None));
    }

    #[test]
    fn commit_messages_are_checked_before_git_runs() {
        let never = |_: &str| false;
        assert!(check_message("  \n", &never).is_err());
        assert!(check_message("fix", &never).is_ok());
        assert!(check_message("fix", &|_| true).is_err());
        assert!(check_message(&"x".repeat(MAX_MESSAGE_BYTES + 1), &never).is_err());
    }

    #[test]
    fn paths_must_be_plain_project_paths() {
        assert!(check_paths(&[]).is_err());
        assert!(check_paths(&["../x".to_owned()]).is_err());
        assert!(check_paths(&["/etc/passwd".to_owned()]).is_err());
        assert!(check_paths(&["src/a.rs".to_owned()]).is_ok());
    }
}
