//! The read-only Git diff view: a highlighter that colours lines by their first characters,
//! without a parser.

use std::ops::Range;

use gpui::{Context, HighlightStyle, SharedString, Window};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};

/// The style class of one diff line, `None` for context lines.
pub fn classify_diff_line(line: &str) -> Option<&'static str> {
    if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff --git")
        || line.starts_with("index ")
    {
        Some("diff_header")
    } else if line.starts_with("@@") {
        Some("diff_hunk")
    } else if line.starts_with('+') {
        Some("diff_added")
    } else if line.starts_with('-') {
        Some("diff_removed")
    } else {
        None
    }
}

/// Colours a unified diff by line.
#[derive(Default)]
pub struct DiffHighlighter {
    /// `(start_byte, end_byte, class)` per coloured line, in text order.
    lines: Vec<(usize, usize, &'static str)>,
}

impl DiffHighlighter {
    fn index(text: &str) -> Vec<(usize, usize, &'static str)> {
        let mut lines = Vec::new();
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.len();
            if let Some(class) = classify_diff_line(line.trim_end_matches(['\n', '\r'])) {
                lines.push((start, end, class));
            }
            start = end;
        }
        lines
    }
}

impl InputHighlighter for DiffHighlighter {
    fn language(&self) -> SharedString {
        SharedString::from("diff")
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.lines = Self::index(&text.to_string());
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let first = self
            .lines
            .partition_point(|(_, end, _)| *end <= range.start);
        self.lines[first..]
            .iter()
            .take_while(|(start, _, _)| *start < range.end)
            .filter_map(|(start, end, class)| {
                resolver.style(class).map(|style| (*start..*end, style))
            })
            .collect()
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines_are_classified_by_their_first_characters() {
        assert_eq!(
            classify_diff_line("diff --git a/x b/x"),
            Some("diff_header")
        );
        assert_eq!(classify_diff_line("+++ b/x"), Some("diff_header"));
        assert_eq!(classify_diff_line("--- a/x"), Some("diff_header"));
        assert_eq!(classify_diff_line("@@ -1,2 +1,3 @@"), Some("diff_hunk"));
        assert_eq!(classify_diff_line("+added"), Some("diff_added"));
        assert_eq!(classify_diff_line("-removed"), Some("diff_removed"));
        assert_eq!(classify_diff_line(" context"), None);
        assert_eq!(classify_diff_line(""), None);
    }

    #[test]
    fn only_the_lines_in_the_range_are_returned() {
        let text = "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new\n context\n";
        let highlighter = DiffHighlighter {
            lines: DiffHighlighter::index(text),
        };
        struct All;
        impl HighlightStyleResolver for All {
            fn style(&self, _: &str) -> Option<HighlightStyle> {
                Some(HighlightStyle::default())
            }
        }
        let removed = text.find("-old").unwrap();
        let styled = highlighter.styles(&(removed..removed + 4), &All);
        assert_eq!(styled.len(), 1);
        assert_eq!(&text[styled[0].0.clone()], "-old\n");
        assert_eq!(highlighter.styles(&(0..text.len()), &All).len(), 4);
    }
}
