//! Project-wide text replacement over the files a search named. The search says where the
//! text probably is; this module re-reads every file, replaces the literal text and saves
//! with the revision it read, so a file that changed in between is reported, not overwritten.

use std::collections::HashSet;

use crate::documents::Document;
use crate::error::{AppError, ErrorCode, Result};
use crate::project::ProjectRoot;
use crate::search::MAX_PATTERN_BYTES;

/// What to replace: the same pattern the search used (`raios locate` reads it as a regular
/// expression), so what the hits show is what gets replaced.
#[derive(Debug, Clone)]
pub struct Replacement {
    pattern: regex::Regex,
    with: String,
}

impl Replacement {
    /// A replacement, refused when `find` is empty, either side is too long, or `find` is not
    /// a valid pattern. In `with`, `$1` or `${name}` insert a captured group and `$$` is a
    /// literal dollar sign.
    pub fn new(find: &str, with: &str, ignore_case: bool) -> Result<Self> {
        if find.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Aranacak metin boş.",
            ));
        }
        if find.len() > MAX_PATTERN_BYTES || with.len() > MAX_PATTERN_BYTES {
            return Err(AppError::new(ErrorCode::InvalidInput, "Metin çok uzun."));
        }
        let pattern = regex::RegexBuilder::new(find)
            .case_insensitive(ignore_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|error| {
                AppError::new(
                    ErrorCode::InvalidInput,
                    format!("Desen geçersiz: {error}")
                        .lines()
                        .next()
                        .unwrap_or_default(),
                )
            })?;
        Ok(Self {
            pattern,
            with: with.to_owned(),
        })
    }

    /// `text` with every match replaced, and how many there were. Empty matches are ignored,
    /// so a pattern like `x*` never rewrites a whole file.
    pub fn apply(&self, text: &str) -> (String, usize) {
        let mut count = 0;
        let replaced = self
            .pattern
            .replace_all(text, |captures: &regex::Captures| {
                let whole = captures.get(0).map_or("", |m| m.as_str());
                if whole.is_empty() {
                    return String::new();
                }
                count += 1;
                let mut out = String::new();
                captures.expand(&self.with, &mut out);
                out
            });
        (replaced.into_owned(), count)
    }
}

/// One replacement inside a text, positioned the way editors and language servers count:
/// zero-based lines and UTF-16 columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Line and column where the match starts.
    pub start: (u32, u32),
    /// Line and column where it ends.
    pub end: (u32, u32),
    /// What takes its place.
    pub text: String,
}

impl Replacement {
    /// The changes `apply` would make, last match first, so each can be applied without
    /// moving the ones before it.
    pub fn changes(&self, text: &str) -> Vec<Change> {
        let mut line_starts = vec![0usize];
        line_starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        let position = |offset: usize| {
            let line = line_starts.partition_point(|&start| start <= offset) - 1;
            let column = text[line_starts[line]..offset].encode_utf16().count();
            (line as u32, column as u32)
        };
        let mut changes: Vec<Change> = self
            .pattern
            .captures_iter(text)
            .filter_map(|captures| {
                let whole = captures.get(0)?;
                if whole.as_str().is_empty() {
                    return None;
                }
                let mut out = String::new();
                captures.expand(&self.with, &mut out);
                Some(Change {
                    start: position(whole.start()),
                    end: position(whole.end()),
                    text: out,
                })
            })
            .collect();
        changes.reverse();
        changes
    }
}

/// What a replacement did, or would do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplaceReport {
    /// Files with at least one replacement, and the count in each.
    pub files: Vec<(String, usize)>,
    /// Files left alone because they have unsaved edits in the editor.
    pub skipped: Vec<String>,
    /// Files that could not be read or saved (binary, too big, changed meanwhile).
    pub failed: Vec<String>,
}

impl ReplaceReport {
    /// Total replacements.
    pub fn replacements(&self) -> usize {
        self.files.iter().map(|(_, count)| count).sum()
    }
}

/// Counts what `replacement` would change in `paths`; nothing is written.
pub fn preview(
    project: &ProjectRoot,
    paths: &[String],
    replacement: &Replacement,
    busy: &HashSet<String>,
) -> ReplaceReport {
    run(project, paths, replacement, busy, false)
}

/// Replaces in `paths`, saving each changed file.
pub fn replace_all(
    project: &ProjectRoot,
    paths: &[String],
    replacement: &Replacement,
    busy: &HashSet<String>,
) -> ReplaceReport {
    run(project, paths, replacement, busy, true)
}

fn run(
    project: &ProjectRoot,
    paths: &[String],
    replacement: &Replacement,
    busy: &HashSet<String>,
    write: bool,
) -> ReplaceReport {
    let mut report = ReplaceReport::default();
    let mut seen = HashSet::new();
    for path in paths {
        if !seen.insert(path.as_str()) {
            continue;
        }
        if busy.contains(path) {
            report.skipped.push(path.clone());
            continue;
        }
        let Ok(document) = project.read_document(path) else {
            report.failed.push(path.clone());
            continue;
        };
        let (content, count) = replacement.apply(&document.content);
        if count == 0 {
            continue;
        }
        if write
            && project
                .save_document(Document {
                    content,
                    ..document
                })
                .is_err()
        {
            report.failed.push(path.clone());
            continue;
        }
        report.files.push((path.clone(), count));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_every_match_and_expands_groups() {
        let r = Replacement::new("aa", "b", false).unwrap();
        assert_eq!(r.apply("aaaaa aa"), ("bba b".to_owned(), 3));
        assert_eq!(r.apply("none"), ("none".to_owned(), 0));
        let r = Replacement::new(r"fn (\w+)\(", "pub fn ${1}(", false).unwrap();
        let changes = r.changes("x\nfn a() fn é()");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].start, (1, 7));
        assert_eq!(changes[0].text, "pub fn é(");
        assert_eq!(changes[1].start, (1, 0));
        assert_eq!(
            r.apply("fn a() fn bc()"),
            ("pub fn a() pub fn bc()".to_owned(), 2)
        );
        let r = Replacement::new("cost", "$$5", false).unwrap();
        assert_eq!(r.apply("cost").0, "$5");
    }

    #[test]
    fn empty_matches_never_count_or_rewrite() {
        let r = Replacement::new("x*", "-", false).unwrap();
        assert_eq!(r.apply("abc"), ("abc".to_owned(), 0));
        assert_eq!(r.apply("axxb"), ("a-b".to_owned(), 1));
    }

    #[test]
    fn case_can_be_ignored() {
        let r = Replacement::new("foo", "bar", true).unwrap();
        assert_eq!(r.apply("Foo FOO foo"), ("bar bar bar".to_owned(), 3));
        let r = Replacement::new("foo", "bar", false).unwrap();
        assert_eq!(r.apply("Foo foo").1, 1);
    }

    #[test]
    fn an_empty_huge_or_invalid_find_is_refused() {
        assert!(Replacement::new("", "x", false).is_err());
        assert!(Replacement::new(&"a".repeat(MAX_PATTERN_BYTES + 1), "x", false).is_err());
        let error = Replacement::new("(", "x", false).unwrap_err();
        assert!(
            error.message.starts_with("Desen geçersiz"),
            "{}",
            error.message
        );
    }

    #[test]
    fn preview_writes_nothing_and_replace_all_skips_busy_files() {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [("a.txt", "foo foo"), ("b.txt", "foo"), ("c.txt", "bar")] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let project = ProjectRoot::open(dir.path().to_path_buf()).unwrap();
        let paths: Vec<String> = ["a.txt", "b.txt", "c.txt", "a.txt", "gone.txt"]
            .map(String::from)
            .to_vec();
        let r = Replacement::new("foo", "baz", false).unwrap();
        let busy: HashSet<String> = ["b.txt".to_owned()].into();

        let seen = preview(&project, &paths, &r, &busy);
        assert_eq!(seen.files, [("a.txt".to_owned(), 2)]);
        assert_eq!(seen.skipped, ["b.txt"]);
        assert_eq!(seen.failed, ["gone.txt"]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "foo foo"
        );

        let done = replace_all(&project, &paths, &r, &busy);
        assert_eq!(done.replacements(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "baz baz"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "foo"
        );
    }
}
