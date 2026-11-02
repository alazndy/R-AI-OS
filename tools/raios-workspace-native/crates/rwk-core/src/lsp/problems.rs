//! The diagnostics of the open project, by file: what the Problems panel lists and the status
//! line counts.

use std::collections::BTreeMap;

use lsp_types::{Diagnostic, DiagnosticSeverity};

/// How serious a problem is, most serious first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Does not compile / does not work.
    Error,
    /// Suspicious.
    Warning,
    /// For information.
    Info,
    /// A suggestion.
    Hint,
}

impl Severity {
    /// A server that sends no severity is read as an error (the protocol leaves it open).
    pub fn of(diagnostic: &Diagnostic) -> Self {
        match diagnostic.severity {
            Some(DiagnosticSeverity::WARNING) => Self::Warning,
            Some(DiagnosticSeverity::INFORMATION) => Self::Info,
            Some(DiagnosticSeverity::HINT) => Self::Hint,
            _ => Self::Error,
        }
    }
}

/// One line of the Problems panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Project-relative path.
    pub path: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column.
    pub column: usize,
    /// How serious it is.
    pub severity: Severity,
    /// The server's message, first line only.
    pub message: String,
    /// The reporting tool (`rustc`, `eslint`, …), when it says.
    pub source: Option<String>,
}

/// Error and warning counts for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
}

/// The diagnostics of every open file.
#[derive(Debug, Default)]
pub struct Problems {
    by_file: BTreeMap<String, Vec<Diagnostic>>,
}

impl Problems {
    /// Replaces the diagnostics of `path`; an empty list removes the file.
    pub fn set(&mut self, path: &str, diagnostics: Vec<Diagnostic>) {
        if diagnostics.is_empty() {
            self.by_file.remove(path);
        } else {
            self.by_file.insert(path.to_owned(), diagnostics);
        }
    }

    /// Forgets everything (a project switch).
    pub fn clear(&mut self) {
        self.by_file.clear();
    }

    /// The diagnostics of one file.
    pub fn of(&self, path: &str) -> &[Diagnostic] {
        self.by_file.get(path).map_or(&[], Vec::as_slice)
    }

    /// Error and warning totals.
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for diagnostic in self.by_file.values().flatten() {
            match Severity::of(diagnostic) {
                Severity::Error => counts.errors += 1,
                Severity::Warning => counts.warnings += 1,
                _ => {}
            }
        }
        counts
    }

    /// Every problem, errors first, then by path and position.
    pub fn list(&self) -> Vec<Problem> {
        let mut problems: Vec<Problem> = self
            .by_file
            .iter()
            .flat_map(|(path, diagnostics)| {
                diagnostics.iter().map(move |diagnostic| Problem {
                    path: path.clone(),
                    line: diagnostic.range.start.line as usize + 1,
                    column: diagnostic.range.start.character as usize + 1,
                    severity: Severity::of(diagnostic),
                    message: diagnostic
                        .message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    source: diagnostic.source.clone(),
                })
            })
            .collect();
        problems.sort_by(|a, b| {
            (a.severity, &a.path, a.line, a.column).cmp(&(b.severity, &b.path, b.line, b.column))
        });
        problems
    }
}

/// The status line text: `2 hata · 1 uyarı`, or `None` when there is nothing to say.
pub fn summary(counts: Counts) -> Option<String> {
    match (counts.errors, counts.warnings) {
        (0, 0) => None,
        (e, 0) => Some(format!("{e} hata")),
        (0, w) => Some(format!("{w} uyarı")),
        (e, w) => Some(format!("{e} hata · {w} uyarı")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range};

    fn diagnostic(line: u32, severity: Option<DiagnosticSeverity>, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, 4), Position::new(line, 6)),
            severity,
            message: message.to_owned(),
            ..Diagnostic::default()
        }
    }

    #[test]
    fn counts_lists_and_summaries_follow_the_stored_diagnostics() {
        let mut problems = Problems::default();
        problems.set(
            "b.rs",
            vec![diagnostic(
                1,
                Some(DiagnosticSeverity::WARNING),
                "unused\nmore",
            )],
        );
        problems.set(
            "a.rs",
            vec![
                diagnostic(9, Some(DiagnosticSeverity::ERROR), "bad"),
                diagnostic(2, None, "no severity"),
                diagnostic(3, Some(DiagnosticSeverity::HINT), "hint"),
            ],
        );
        assert_eq!(
            problems.counts(),
            Counts {
                errors: 2,
                warnings: 1
            }
        );
        let list = problems.list();
        let order: Vec<_> = list.iter().map(|p| (p.path.as_str(), p.line)).collect();
        // Errors by position, then the warning, then the hint.
        assert_eq!(order, [("a.rs", 3), ("a.rs", 10), ("b.rs", 2), ("a.rs", 4)]);
        assert_eq!(list[2].message, "unused");
        assert_eq!(list[0].column, 5);
        assert_eq!(
            summary(problems.counts()).as_deref(),
            Some("2 hata · 1 uyarı")
        );

        problems.set("a.rs", Vec::new());
        assert_eq!(problems.of("a.rs"), &[] as &[Diagnostic]);
        assert_eq!(summary(problems.counts()).as_deref(), Some("1 uyarı"));
        problems.clear();
        assert_eq!(summary(problems.counts()), None);
    }
}
