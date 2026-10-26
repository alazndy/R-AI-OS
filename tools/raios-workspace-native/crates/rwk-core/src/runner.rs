//! Named commands for a project (build, test, lint), kept in a file the user owns outside the
//! project, and the `path:line:col` places a command's output points at.
//!
//! A task is a program and its arguments, never a shell line: nothing is expanded, and a
//! project cannot define tasks for itself, so opening an untrusted checkout runs nothing.
//! Someone who wants a shell line writes `["sh", "-c", "…"]` on purpose.

use std::path::Path;

use serde::Deserialize;

/// Most tasks listed for one project.
pub const MAX_TASKS: usize = 32;
/// Most arguments (program included) of one task.
pub const MAX_ARGS: usize = 64;
/// Longest task name, in characters.
pub const MAX_NAME_CHARS: usize = 48;
/// Longest single argument, in bytes.
pub const MAX_ARG_BYTES: usize = 1024;

/// One named command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTask {
    /// What the picker shows.
    pub name: String,
    /// The program (first element) and its arguments.
    pub command: Vec<String>,
}

#[derive(Deserialize)]
struct RawTask {
    name: String,
    command: Vec<String>,
}

#[derive(Deserialize, Default)]
struct RawFile {
    #[serde(default)]
    global: Vec<RawTask>,
    #[serde(default)]
    projects: std::collections::BTreeMap<String, Vec<RawTask>>,
}

fn valid(task: &RawTask) -> bool {
    let name = task.name.trim();
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.chars().any(char::is_control)
        && !task.command.is_empty()
        && task.command.len() <= MAX_ARGS
        && !task.command[0].trim().is_empty()
        && task
            .command
            .iter()
            .all(|arg| arg.len() <= MAX_ARG_BYTES && !arg.contains('\0'))
}

/// The tasks of `project` from the contents of the tasks file:
///
/// ```json
/// {"global": [{"name": "git log", "command": ["git", "log", "--oneline"]}],
///  "projects": {"/home/me/app": [{"name": "test", "command": ["cargo", "test"]}]}}
/// ```
///
/// Project tasks come first and replace a global task of the same name. Invalid entries are
/// skipped; an unreadable file gives no tasks.
pub fn tasks_for(file: &str, project: &Path) -> Vec<RunTask> {
    let Ok(raw) = serde_json::from_str::<RawFile>(file) else {
        return Vec::new();
    };
    let own = raw
        .projects
        .get(project.to_string_lossy().as_ref())
        .into_iter()
        .flatten();
    let mut tasks: Vec<RunTask> = Vec::new();
    for task in own.chain(raw.global.iter()).filter(|task| valid(task)) {
        let name = task.name.trim().to_owned();
        if tasks.len() < MAX_TASKS && !tasks.iter().any(|known| known.name == name) {
            tasks.push(RunTask {
                name,
                command: task.command.clone(),
            });
        }
    }
    tasks
}

/// Where the tasks of a project come from (the application reads its config directory).
pub trait TaskSource: Send + Sync {
    /// The tasks of `project`.
    fn load(&self, project: &Path) -> Vec<RunTask>;
}

/// A place in a file that some output mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The path as written.
    pub path: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column, when given.
    pub column: Option<usize>,
}

fn is_break(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '(' | ')' | '[' | ']' | '"' | '\'' | '`' | '<' | '>' | '='
        )
}

/// The location written at character `column` of `line`, if the word there is one.
pub fn location_at(line: &str, column: usize) -> Option<Location> {
    let characters: Vec<char> = line.chars().collect();
    if column >= characters.len() || is_break(characters[column]) {
        return None;
    }
    let mut from = column;
    while from > 0 && !is_break(characters[from - 1]) {
        from -= 1;
    }
    let mut to = column;
    while to + 1 < characters.len() && !is_break(characters[to + 1]) {
        to += 1;
    }
    let word: String = characters[from..=to].iter().collect();
    locations(&word).into_iter().next()
}

/// Every `path:line` or `path:line:col` in `text`, in order. Anything with `://` is a URL and
/// is skipped. Whether the path exists is for the caller to check.
pub fn locations(text: &str) -> Vec<Location> {
    let mut found = Vec::new();
    for word in text.split(is_break) {
        if word.contains("://") {
            continue;
        }
        let parts: Vec<&str> = word
            .trim_end_matches([':', ',', '.', ';'])
            .split(':')
            .collect();
        let number = |part: &str| part.parse::<usize>().ok().filter(|n| *n > 0);
        let (path, line, column) = match parts.as_slice() {
            [path, line, column] => match (number(line), number(column)) {
                (Some(line), column) => (*path, line, column),
                _ => continue,
            },
            [path, line] => match number(line) {
                Some(line) => (*path, line, None),
                None => continue,
            },
            _ => continue,
        };
        let path = path.strip_prefix("./").unwrap_or(path);
        if path.is_empty() || path.len() > 512 || path.contains('\0') {
            continue;
        }
        found.push(Location {
            path: path.to_owned(),
            line,
            column,
        });
    }
    found
}

/// The last location in `text` whose file exists in the project, as a project-relative path
/// and line. Errors scroll to the bottom, so the last one is the freshest. `exists` answers
/// for a project-relative path; absolute paths inside `root` are made relative first.
pub fn last_project_location(
    text: &str,
    root: &Path,
    exists: &dyn Fn(&str) -> bool,
) -> Option<(String, usize)> {
    locations(text).into_iter().rev().find_map(|location| {
        let relative = match Path::new(&location.path).strip_prefix(root) {
            Ok(inside) => inside.to_string_lossy().into_owned(),
            Err(_) if Path::new(&location.path).is_absolute() => return None,
            Err(_) => location.path.clone(),
        };
        crate::project::validate_relative_path(&relative, false).ok()?;
        exists(&relative).then_some((relative, location.line))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
        "global": [
            {"name": "log", "command": ["git", "log", "--oneline"]},
            {"name": "test", "command": ["echo", "global test"]},
            {"name": "", "command": ["x"]},
            {"name": "empty", "command": []}
        ],
        "projects": {
            "/work/app": [{"name": "test", "command": ["cargo", "test"]}],
            "/work/other": [{"name": "build", "command": ["make"]}]
        }
    }"#;

    #[test]
    fn project_tasks_come_first_and_replace_global_ones_of_the_same_name() {
        let tasks = tasks_for(FILE, Path::new("/work/app"));
        let names: Vec<_> = tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["test", "log"]);
        assert_eq!(tasks[0].command, ["cargo", "test"]);
        // Another project sees only its own and the global ones.
        let names: Vec<_> = tasks_for(FILE, Path::new("/work/other"))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["build", "log", "test"]);
        assert!(tasks_for("not json", Path::new("/work/app")).is_empty());
    }

    #[test]
    fn locations_are_found_in_compiler_style_output() {
        let text = "error[E0308]: mismatched types\n  --> src/main.rs:12:5\nsee (lib/util.py:7) and https://x.dev:80/a:3\nFAIL ./t/a_test.go:9: boom";
        let found = locations(text);
        let got: Vec<_> = found
            .iter()
            .map(|l| (l.path.as_str(), l.line, l.column))
            .collect();
        assert_eq!(
            got,
            [
                ("src/main.rs", 12, Some(5)),
                ("lib/util.py", 7, None),
                ("t/a_test.go", 9, None)
            ]
        );
    }

    #[test]
    fn only_existing_project_files_count_and_the_last_one_wins() {
        let root = Path::new("/work/app");
        let text = "a.rs:1:1 gone.rs:5 /work/app/src/b.rs:20 /etc/passwd:1 ../up.rs:2";
        let exists = |path: &str| path == "a.rs" || path == "src/b.rs";
        assert_eq!(
            last_project_location(text, root, &exists),
            Some(("src/b.rs".to_owned(), 20))
        );
        assert_eq!(last_project_location("gone.rs:5", root, &exists), None);
    }

    #[test]
    fn the_location_under_a_column_is_the_word_there() {
        let line = "error at src/main.rs:12:5, see (lib.rs:3) ok";
        let at = |column| location_at(line, column).map(|l| (l.path, l.line));
        assert_eq!(at(12), Some(("src/main.rs".to_owned(), 12)));
        assert_eq!(at(20), Some(("src/main.rs".to_owned(), 12)));
        assert_eq!(at(0), None);
        assert_eq!(at(36), Some(("lib.rs".to_owned(), 3)));
        assert_eq!(at(8), None, "a space is not a location");
        assert_eq!(at(500), None);
    }
}
