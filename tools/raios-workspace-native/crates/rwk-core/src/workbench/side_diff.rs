//! Turns a unified diff into two aligned columns for the side-by-side view. Pure text: each
//! output line keeps the one-character prefix (` `, `-`, `+`, `@`) the diff highlighter colours
//! by, and both columns always have the same number of lines so they scroll together.

/// The two columns of a side-by-side diff.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SideBySide {
    /// The old file: context and removed lines.
    pub left: String,
    /// The new file: context and added lines.
    pub right: String,
}

/// Most lines of a diff that are aligned; the rest is cut so a huge diff cannot stall the UI.
pub const MAX_DIFF_LINES: usize = 20_000;

fn is_header(line: &str) -> bool {
    line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("new file mode")
        || line.starts_with("deleted file mode")
        || line.starts_with("similarity index")
        || line.starts_with("rename ")
        || line.starts_with("old mode")
        || line.starts_with("new mode")
}

fn flush(
    removed: &mut Vec<String>,
    added: &mut Vec<String>,
    left: &mut Vec<String>,
    right: &mut Vec<String>,
) {
    let rows = removed.len().max(added.len());
    for row in 0..rows {
        left.push(
            removed
                .get(row)
                .map_or_else(|| " ".to_owned(), |line| format!("-{line}")),
        );
        right.push(
            added
                .get(row)
                .map_or_else(|| " ".to_owned(), |line| format!("+{line}")),
        );
    }
    removed.clear();
    added.clear();
}

/// Aligns `diff` (the output of `git diff` for one file) into two columns.
pub fn side_by_side(diff: &str) -> SideBySide {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut in_hunk = false;
    for line in diff.lines().take(MAX_DIFF_LINES) {
        if line.starts_with("@@") {
            flush(&mut removed, &mut added, &mut left, &mut right);
            in_hunk = true;
            left.push(line.to_owned());
            right.push(line.to_owned());
            continue;
        }
        if !in_hunk || (is_header(line) && !line.starts_with("--- ") && !line.starts_with("+++ ")) {
            // Before the first hunk (and for the next file's header) nothing is shown.
            if line.starts_with("diff --git") {
                flush(&mut removed, &mut added, &mut left, &mut right);
                in_hunk = false;
            }
            continue;
        }
        match line.chars().next() {
            Some('-') => removed.push(line[1..].to_owned()),
            Some('+') => added.push(line[1..].to_owned()),
            Some('\\') => {} // "\ No newline at end of file"
            other => {
                flush(&mut removed, &mut added, &mut left, &mut right);
                let text = if other == Some(' ') { &line[1..] } else { line };
                left.push(format!(" {text}"));
                right.push(format!(" {text}"));
            }
        }
    }
    flush(&mut removed, &mut added, &mut left, &mut right);
    SideBySide {
        left: left.join("\n"),
        right: right.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1,4 +1,5 @@\n keep\n-old one\n-old two\n+new one\n+new two\n+new three\n tail\n";

    #[test]
    fn removed_and_added_blocks_are_paired_row_by_row() {
        let sbs = side_by_side(DIFF);

        assert_eq!(
            sbs.left.lines().collect::<Vec<_>>(),
            [
                "@@ -1,4 +1,5 @@",
                " keep",
                "-old one",
                "-old two",
                " ",
                " tail"
            ]
        );
        assert_eq!(
            sbs.right.lines().collect::<Vec<_>>(),
            [
                "@@ -1,4 +1,5 @@",
                " keep",
                "+new one",
                "+new two",
                "+new three",
                " tail"
            ]
        );
    }

    #[test]
    fn both_columns_always_have_the_same_number_of_lines() {
        for diff in [
            DIFF,
            "",
            "@@ -1 +1 @@\n-a\n+b\n+c\n+d",
            "@@ -1,2 +1 @@\n-a\n-b\n+c",
        ] {
            let sbs = side_by_side(diff);
            assert_eq!(
                sbs.left.lines().count(),
                sbs.right.lines().count(),
                "{diff:?}"
            );
        }
    }

    #[test]
    fn headers_and_no_newline_markers_are_not_shown() {
        let sbs = side_by_side("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n");

        assert!(!sbs.left.contains("diff --git") && !sbs.left.contains("No newline"));
        assert_eq!(sbs.left.lines().last(), Some("-a"));
        assert_eq!(sbs.right.lines().last(), Some("+b"));
    }

    #[test]
    fn an_empty_diff_gives_empty_columns() {
        assert_eq!(side_by_side(""), SideBySide::default());
    }

    #[test]
    fn a_huge_diff_is_cut_instead_of_stalling() {
        let huge = format!("@@ -1 +1 @@\n{}", " line\n".repeat(MAX_DIFF_LINES * 2));

        assert!(side_by_side(&huge).left.lines().count() <= MAX_DIFF_LINES);
    }
}
