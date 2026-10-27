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

/// The checked-out branch, read from `.git/HEAD` without starting git (cheap enough to ask on
/// every focus change). `None` for a detached HEAD or a folder that is not a repository. A
/// worktree's `.git` file is followed once.
pub fn head_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.strip_prefix("gitdir:")?.trim();
        let target = Path::new(target);
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            root.join(target)
        }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
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

fn check_branch_name(root: &Path, name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.contains(['\0', '\n']) {
        return Err(AppError::new(ErrorCode::InvalidInput, "Not a branch name."));
    }
    run(root, &["check-ref-format", "--branch", name], None)
        .map(drop)
        .map_err(|_| AppError::new(ErrorCode::InvalidInput, "Not a valid branch name."))
}

/// Creates branch `name` at the current commit and switches to it.
pub fn create_branch(root: &Path, name: &str) -> Result<()> {
    check_branch_name(root, name)?;
    run(root, &["switch", "-q", "-c", name], None).map(drop)
}

/// Deletes a branch that is fully merged; git refuses an unmerged one and the checked-out one.
pub fn delete_branch(root: &Path, name: &str) -> Result<()> {
    check_branch_name(root, name)?;
    run(root, &["branch", "-d", "--", name], None).map(drop)
}

/// One entry of the stash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// Position, 0 being the newest.
    pub index: usize,
    /// What git shows for it (`On main: message`).
    pub message: String,
}

/// Puts the changes, untracked files included, on the stash and cleans the working tree.
pub fn stash_push(root: &Path, message: &str) -> Result<()> {
    if message.len() > MAX_MESSAGE_BYTES || message.contains('\0') {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The message is too long.",
        ));
    }
    let output = run(
        root,
        // Stashing untracked files breaks under `--literal-pathspecs`, which `run` always adds.
        &[
            "--no-literal-pathspecs",
            "stash",
            "push",
            "--include-untracked",
            "-m",
            message,
        ],
        None,
    )?;
    if output.contains("No local changes") {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "There is nothing to stash.",
        ));
    }
    Ok(())
}

/// The stash, newest first.
pub fn stash_list(root: &Path) -> Result<Vec<Stash>> {
    let output = run(root, &["stash", "list", "--format=%gs"], None)?;
    Ok(output
        .lines()
        .enumerate()
        .map(|(index, message)| Stash {
            index,
            message: message.to_owned(),
        })
        .collect())
}

/// Applies stash entry `index` and drops it. A conflict leaves the entry in place.
pub fn stash_pop(root: &Path, index: usize) -> Result<()> {
    run(
        root,
        &["stash", "pop", "--index", &format!("stash@{{{index}}}")],
        None,
    )
    .map(drop)
}

/// The branch the current one follows (`origin/main`), if it follows one.
pub fn upstream(root: &Path) -> Option<String> {
    run(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        None,
    )
    .ok()
    .map(|name| name.trim().to_owned())
    .filter(|name| !name.is_empty())
}

/// Fast-forwards the current branch from its upstream; a diverged branch is an error, not a
/// merge.
pub fn pull(root: &Path) -> Result<()> {
    run(root, &["pull", "--ff-only", "-q"], None).map(drop)
}

/// Pushes the current branch to its upstream, or to `origin` as a new upstream when it has none.
/// Never forces.
pub fn push(root: &Path) -> Result<()> {
    if upstream(root).is_some() {
        return run(root, &["push", "-q"], None).map(drop);
    }
    let branch = run(root, &["branch", "--show-current"], None)?;
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "HEAD is detached; nothing to push.",
        ));
    }
    run(root, &["push", "-q", "-u", "origin", "--", branch], None).map(drop)
}

/// How a changed file is marked in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BadgeKind {
    /// New or untracked.
    Added,
    /// Changed, renamed or retyped.
    Modified,
    /// Removed.
    Deleted,
    /// Unresolved merge conflict.
    Conflict,
}

/// The mark next to a file or folder in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    /// The letter (`M`, `A`, `U`, `D`, `!`); a folder shows a dot.
    pub letter: char,
    /// What kind of change it is.
    pub kind: BadgeKind,
}

/// The tree marks of a status: one per changed file and, for each folder above them, a dot of
/// the most serious change inside (conflict, then deleted, modified, added).
pub fn badges(status: &Status) -> std::collections::HashMap<String, Badge> {
    let mut map = std::collections::HashMap::new();
    for entry in &status.entries {
        let badge = if entry.conflicted {
            Badge {
                letter: '!',
                kind: BadgeKind::Conflict,
            }
        } else if entry.untracked {
            Badge {
                letter: 'U',
                kind: BadgeKind::Added,
            }
        } else {
            let change = if entry.unstaged == Change::Unchanged {
                entry.staged
            } else {
                entry.unstaged
            };
            let kind = match change {
                Change::Added => BadgeKind::Added,
                Change::Deleted => BadgeKind::Deleted,
                _ => BadgeKind::Modified,
            };
            Badge {
                letter: change.letter(),
                kind,
            }
        };
        map.insert(entry.path.clone(), badge);
        let mut folder = entry.path.as_str();
        while let Some((parent, _)) = folder.rsplit_once('/') {
            let dot = Badge {
                letter: '•',
                kind: badge.kind,
            };
            let worse = |known: &Badge| rank(known.kind) >= rank(dot.kind);
            if !map.get(parent).is_some_and(worse) {
                map.insert(parent.to_owned(), dot);
            }
            folder = parent;
        }
    }
    map
}

fn rank(kind: BadgeKind) -> u8 {
    match kind {
        BadgeKind::Added => 0,
        BadgeKind::Modified => 1,
        BadgeKind::Deleted => 2,
        BadgeKind::Conflict => 3,
    }
}

/// One block of a file's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ …` line.
    pub header: String,
    /// The lines after it, each with its leading space, `+` or `-` (and `\` markers).
    pub lines: Vec<String>,
}

/// A file's diff split into hunks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// Everything before the first hunk (`diff --git`, `index`, `---`, `+++`).
    pub head: String,
    /// The hunks, in file order.
    pub hunks: Vec<Hunk>,
}

/// Splits `git diff` output for one file into its head and hunks.
pub fn parse_diff(text: &str) -> FileDiff {
    let mut diff = FileDiff::default();
    for line in text.lines() {
        if line.starts_with("@@") {
            diff.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
        } else if let Some(hunk) = diff.hunks.last_mut() {
            hunk.lines.push(line.to_owned());
        } else {
            diff.head.push_str(line);
            diff.head.push('\n');
        }
    }
    diff
}

impl FileDiff {
    /// A patch with only hunk `index`, that `git apply` accepts.
    pub fn patch_for(&self, index: usize) -> Option<String> {
        let hunk = self.hunks.get(index)?;
        let mut patch = self.head.clone();
        patch.push_str(&hunk.header);
        patch.push('\n');
        for line in &hunk.lines {
            patch.push_str(line);
            patch.push('\n');
        }
        Some(patch)
    }
}

/// The diff of one tracked file: the working tree against the index, or (`staged`) the index
/// against HEAD. Untracked, binary and unchanged files give no hunks.
pub fn file_diff(root: &Path, path: &str, staged: bool) -> Result<FileDiff> {
    validate_relative_path(path, false)?;
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-renames", "-U3"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    Ok(parse_diff(&run(root, &args, None)?))
}

/// Stages (or, with `stage` false, unstages) hunk `index` of `path`. `header` is the hunk the
/// user saw; when the file changed since and that hunk is no longer there, nothing happens.
pub fn apply_hunk(root: &Path, path: &str, index: usize, header: &str, stage: bool) -> Result<()> {
    let diff = file_diff(root, path, !stage)?;
    let stale = || AppError::new(ErrorCode::Io, "The file changed; reload the changes.");
    if diff.hunks.get(index).map(|hunk| hunk.header.as_str()) != Some(header) {
        return Err(stale());
    }
    let patch = diff.patch_for(index).ok_or_else(stale)?;
    let mut args = vec!["apply", "--cached", "--recount", "--whitespace=nowarn"];
    if !stage {
        args.push("--reverse");
    }
    args.push("-");
    run(root, &args, Some(&patch)).map(drop)
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
    fn badges_mark_files_and_the_folders_above_with_the_worst_change() {
        let status = parse_status(
            "1 .M N... 100644 100644 100644 a b src/deep/a.rs\0\
             1 .D N... 100644 100644 000000 a b src/gone.rs\0\
             u UU N... 100644 100644 100644 100644 a b c lib/clash.rs\0\
             ? lib/new.rs\0\
             1 A. N... 000000 100644 100644 0 c top.rs\0",
        );
        let marks = badges(&status);
        let at = |path: &str| marks.get(path).map(|b| (b.letter, b.kind));
        assert_eq!(at("src/deep/a.rs"), Some(('M', BadgeKind::Modified)));
        assert_eq!(at("src/gone.rs"), Some(('D', BadgeKind::Deleted)));
        assert_eq!(at("lib/clash.rs"), Some(('!', BadgeKind::Conflict)));
        assert_eq!(at("lib/new.rs"), Some(('U', BadgeKind::Added)));
        assert_eq!(at("top.rs"), Some(('A', BadgeKind::Added)));
        // A folder shows the worst change inside it, whatever order the files came in.
        assert_eq!(at("src"), Some(('•', BadgeKind::Deleted)));
        assert_eq!(at("src/deep"), Some(('•', BadgeKind::Modified)));
        assert_eq!(at("lib"), Some(('•', BadgeKind::Conflict)));
        assert_eq!(at("clean"), None);
    }

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
    fn the_branch_is_read_from_head_without_git() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(head_branch(dir.path()), None);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();
        assert_eq!(head_branch(dir.path()).as_deref(), Some("feat/x"));
        std::fs::write(dir.path().join(".git/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(
            head_branch(dir.path()),
            None,
            "a detached HEAD has no branch"
        );

        // A worktree has a `.git` file that points at the real directory.
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(
            tree.path().join(".git"),
            format!("gitdir: {}\n", dir.path().join(".git").display()),
        )
        .unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(head_branch(tree.path()).as_deref(), Some("main"));
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
