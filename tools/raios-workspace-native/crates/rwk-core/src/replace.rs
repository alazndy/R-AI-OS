//! Project-wide text replacement over the files a search named. The search says where the
//! text probably is; this module re-reads every file, replaces the literal text and saves
//! with the revision it read, so a file that changed in between is reported, not overwritten.

use std::collections::HashSet;

use crate::documents::Document;
use crate::error::{AppError, ErrorCode, Result};
use crate::project::ProjectRoot;
use crate::search::MAX_PATTERN_BYTES;

/// What to replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// The literal text to find (never empty).
    pub find: String,
    /// What takes its place.
    pub with: String,
    /// Fold ASCII letters when matching.
    pub ignore_case: bool,
}

impl Replacement {
    /// A replacement, refused when `find` is empty or either side is too long.
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
        Ok(Self {
            find: find.to_owned(),
            with: with.to_owned(),
            ignore_case,
        })
    }

    /// `text` with every occurrence replaced, and how many there were. Matches do not overlap.
    pub fn apply(&self, text: &str) -> (String, usize) {
        let (haystack, needle) = (text.as_bytes(), self.find.as_bytes());
        let mut out = String::with_capacity(text.len());
        let (mut copied, mut index, mut count) = (0, 0, 0);
        while index + needle.len() <= haystack.len() {
            let window = &haystack[index..index + needle.len()];
            let hit = if self.ignore_case {
                window.eq_ignore_ascii_case(needle)
            } else {
                window == needle
            };
            if hit {
                out.push_str(&text[copied..index]);
                out.push_str(&self.with);
                index += needle.len();
                copied = index;
                count += 1;
            } else {
                index += 1;
            }
        }
        out.push_str(&text[copied..]);
        (out, count)
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
    fn replaces_every_literal_occurrence_without_overlap() {
        let r = Replacement::new("aa", "b", false).unwrap();
        assert_eq!(r.apply("aaaaa aa"), ("bb".to_owned() + "a b", 3));
        assert_eq!(r.apply("none"), ("none".to_owned(), 0));
        let r = Replacement::new("é", "e", false).unwrap();
        assert_eq!(r.apply("café é"), ("cafe e".to_owned(), 2));
    }

    #[test]
    fn ascii_case_can_be_ignored() {
        let r = Replacement::new("foo", "bar", true).unwrap();
        assert_eq!(r.apply("Foo FOO foo"), ("bar bar bar".to_owned(), 3));
        let r = Replacement::new("foo", "bar", false).unwrap();
        assert_eq!(r.apply("Foo foo").1, 1);
    }

    #[test]
    fn an_empty_or_huge_find_is_refused() {
        assert!(Replacement::new("", "x", false).is_err());
        assert!(Replacement::new(&"a".repeat(MAX_PATTERN_BYTES + 1), "x", false).is_err());
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
