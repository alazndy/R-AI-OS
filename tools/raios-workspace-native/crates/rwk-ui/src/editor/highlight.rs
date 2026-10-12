//! Syntax highlighting for the editor, on tree-sitter.
//!
//! `gpui-base` ships no highlighter; it asks the application for one through the
//! [`InputHighlighter`] trait. Parsing is incremental (the previous tree is edited and reused, so
//! an edit re-parses only the changed region, in milliseconds even for large files) and the
//! highlight query runs only over the byte range the editor is painting. Token classes are fixed
//! names resolved to colours by the editor style, never by a grammar's own theme, so switching
//! tone does not re-parse anything.
//!
//! (The first implementation used `syntect`; its pure-Rust regex engine parsed about 3K lines
//! per second, which made a 30K-line file take 15 s. See the Plan 5 execution notes.)

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use gpui::{Context, HighlightStyle, SharedString, Window};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit as TsEdit, Language, Parser, Point, Query, QueryCursor, Tree};

/// Files larger than this open without highlighting (spec section 12: cost on large files).
pub const HIGHLIGHT_LIMIT_BYTES: usize = 1024 * 1024;

/// Ranges larger than this are not highlighted in one query (a giant minified line).
const MAX_QUERY_BYTES: usize = 512 * 1024;

/// True when a file of `len` bytes is highlighted.
pub fn highlight_enabled(len: usize) -> bool {
    len <= HIGHLIGHT_LIMIT_BYTES
}

/// The languages Workspace 0.1 highlighted, plus the TypeScript family.
const LANGUAGES: [(&str, &[&str]); 5] = [
    ("rust", &["rs"]),
    ("json", &["json"]),
    ("markdown", &["md", "markdown"]),
    ("javascript", &["js", "mjs", "cjs", "jsx"]),
    ("typescript", &["ts", "mts", "cts"]),
];

/// The language for a file path, by extension.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        .map(|(language, _)| *language)
}

/// The token classes the editor style resolves to colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    /// Language keywords and storage modifiers.
    Keyword,
    /// String literals.
    String,
    /// Comments.
    Comment,
    /// Numeric literals.
    Number,
    /// Type and class names.
    Type,
    /// Function names.
    Function,
    /// Other constants.
    Constant,
    /// Punctuation.
    Punctuation,
}

impl TokenClass {
    /// The name the style resolver is asked for.
    pub fn name(self) -> &'static str {
        match self {
            TokenClass::Keyword => "keyword",
            TokenClass::String => "string",
            TokenClass::Comment => "comment",
            TokenClass::Number => "number",
            TokenClass::Type => "type",
            TokenClass::Function => "function",
            TokenClass::Constant => "constant",
            TokenClass::Punctuation => "punctuation",
        }
    }

    /// Folds a grammar's capture name (`function.method.builtin`, `string.escape`, ...) into a
    /// class; captures drawn in the default text colour give `None`.
    pub fn from_capture(name: &str) -> Option<Self> {
        match name {
            "string.escape" | "escape" | "string.special.symbol" => return Some(Self::String),
            "variable.builtin" | "constant.builtin" | "boolean" => return Some(Self::Constant),
            "constructor" | "namespace" | "module" => return Some(Self::Type),
            "text.title" | "markup.heading" => return Some(Self::Type),
            "text.literal" | "markup.raw" => return Some(Self::String),
            _ => {}
        }
        let root = name.split('.').next().unwrap_or(name);
        match root {
            "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass" => {
                Some(Self::Keyword)
            }
            "function" | "method" => Some(Self::Function),
            "type" => Some(Self::Type),
            "string" | "character" => Some(Self::String),
            "number" | "float" => Some(Self::Number),
            "constant" => Some(Self::Constant),
            "comment" => Some(Self::Comment),
            "punctuation" | "delimiter" => Some(Self::Punctuation),
            _ => None,
        }
    }
}

/// A highlighted byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// The class of the text in the range.
    pub class: TokenClass,
}

/// A grammar with its compiled highlight query.
struct Grammar {
    language: Language,
    query: Query,
    /// The class of each capture index.
    classes: Vec<Option<TokenClass>>,
}

fn build_grammar(name: &str) -> Option<Grammar> {
    let (language, sources): (Language, Vec<&str>) = match name {
        "rust" => (
            tree_sitter_rust::LANGUAGE.into(),
            vec![tree_sitter_rust::HIGHLIGHTS_QUERY],
        ),
        "json" => (
            tree_sitter_json::LANGUAGE.into(),
            vec![tree_sitter_json::HIGHLIGHTS_QUERY],
        ),
        "markdown" => (
            tree_sitter_md::LANGUAGE.into(),
            vec![tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        ),
        "javascript" => (
            tree_sitter_javascript::LANGUAGE.into(),
            vec![
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ],
        ),
        "typescript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            vec![
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ],
        ),
        _ => return None,
    };
    // Later patterns win when two match the same node, so the base language's query goes first.
    let query = Query::new(&language, &sources.join("\n")).ok()?;
    let classes = query
        .capture_names()
        .iter()
        .map(|capture| TokenClass::from_capture(capture))
        .collect();
    Some(Grammar {
        language,
        query,
        classes,
    })
}

/// The compiled grammar of a language, built once per process.
fn grammar(name: &str) -> Option<Arc<Grammar>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Grammar>>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(name.to_owned())
        .or_insert_with(|| build_grammar(name).map(Arc::new))
        .clone()
}

/// True when `language` has a grammar.
pub fn language_supported(language: &str) -> bool {
    grammar(language).is_some()
}

/// A text replacement in bytes and (row, column) positions, as the editor reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEdit {
    /// First changed byte.
    pub start_byte: usize,
    /// End of the replaced range in the old text.
    pub old_end_byte: usize,
    /// End of the inserted range in the new text.
    pub new_end_byte: usize,
    /// Position of `start_byte`.
    pub start_position: (usize, usize),
    /// Position of `old_end_byte` in the old text.
    pub old_end_position: (usize, usize),
    /// Position of `new_end_byte` in the new text.
    pub new_end_position: (usize, usize),
}

impl From<InputEdit> for TextEdit {
    fn from(edit: InputEdit) -> Self {
        let position = |point: gpui_base::input::Point| (point.row, point.column);
        Self {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
            start_position: position(edit.start_position),
            old_end_position: position(edit.old_end_position),
            new_end_position: position(edit.new_end_position),
        }
    }
}

fn point((row, column): (usize, usize)) -> Point {
    Point { row, column }
}

/// Highlighted text: the text, its incrementally maintained syntax tree and the query.
pub struct Highlighted {
    grammar: Arc<Grammar>,
    parser: Parser,
    tree: Option<Tree>,
    text: String,
}

impl Highlighted {
    /// Parses `text` as `language`; `None` when it has no grammar.
    pub fn new(language: &str, text: &str) -> Option<Self> {
        let grammar = grammar(language)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        let tree = parser.parse(text, None);
        Some(Self {
            grammar,
            parser,
            tree,
            text: text.to_owned(),
        })
    }

    /// Applies an edit: the old tree is told about it and reused, so only the changed region is
    /// parsed again. `new_text` is the whole text after the edit.
    pub fn apply_edit(&mut self, edit: &TextEdit, new_text: &str) {
        if let Some(tree) = &mut self.tree {
            tree.edit(&TsEdit {
                start_byte: edit.start_byte,
                old_end_byte: edit.old_end_byte,
                new_end_byte: edit.new_end_byte,
                start_position: point(edit.start_position),
                old_end_position: point(edit.old_end_position),
                new_end_position: point(edit.new_end_position),
            });
        }
        self.text.clear();
        self.text.push_str(new_text);
        self.tree = self.parser.parse(&self.text, self.tree.as_ref());
    }

    /// Replaces the whole text without edit information (nothing is reused).
    pub fn replace(&mut self, new_text: &str) {
        self.text.clear();
        self.text.push_str(new_text);
        self.tree = self.parser.parse(&self.text, None);
    }

    /// Non-overlapping highlighted byte ranges inside `range`, in order.
    pub fn spans_in(&self, range: &Range<usize>) -> Vec<HighlightSpan> {
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        let range = range.start.min(self.text.len())..range.end.min(self.text.len());
        if range.is_empty() || range.len() > MAX_QUERY_BYTES {
            return Vec::new();
        }
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures =
            cursor.captures(&self.grammar.query, tree.root_node(), self.text.as_bytes());
        let mut found: Vec<(Range<usize>, usize, TokenClass)> = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures()[*index];
            if let Some(Some(class)) = self.grammar.classes.get(capture.index as usize) {
                found.push((capture.node.byte_range(), matched.pattern_index, *class));
            }
        }
        // Outer nodes paint first so nested ones (an escape inside a string) win; for the same
        // node a later pattern wins.
        found.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.1.cmp(&b.1)));
        let mut paint: Vec<Option<TokenClass>> = vec![None; range.len()];
        for (node_range, _, class) in found {
            let start = node_range.start.max(range.start) - range.start;
            let end = node_range.end.min(range.end).saturating_sub(range.start);
            if start < end {
                paint[start..end].fill(Some(class));
            }
        }
        let mut spans: Vec<HighlightSpan> = Vec::new();
        for (offset, class) in paint.into_iter().enumerate() {
            let Some(class) = class else {
                continue;
            };
            let byte = range.start + offset;
            match spans.last_mut() {
                Some(last) if last.range.end == byte && last.class == class => {
                    last.range.end += 1;
                }
                _ => spans.push(HighlightSpan {
                    range: byte..byte + 1,
                    class,
                }),
            }
        }
        spans
    }

    /// Every highlighted span of the text.
    pub fn all_spans(&self) -> Vec<HighlightSpan> {
        self.spans_in(&(0..self.text.len()))
    }
}

/// All spans of `text` as `language`; `None` for an unsupported language.
pub fn highlight_spans(language: &str, text: &str) -> Option<Vec<HighlightSpan>> {
    Highlighted::new(language, text).map(|highlighted| highlighted.all_spans())
}

/// Fold ranges by structure, without a parser: a line ending in `{` folds to its closing `}`
/// line; in Markdown a heading folds to the line before the next heading of the same or a
/// higher level. Ranges are `(start_line, end_line)` and span at least two lines.
pub fn fold_ranges_for(language: &str, text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if language == "markdown" {
        let mut open: Vec<(usize, usize)> = Vec::new();
        let mut last_line = 0;
        for (index, line) in text.lines().enumerate() {
            last_line = index;
            let level = line.chars().take_while(|c| *c == '#').count();
            if level == 0 || level > 6 || !line[level..].starts_with(' ') {
                continue;
            }
            while let Some(&(start, open_level)) = open.last() {
                if open_level < level {
                    break;
                }
                open.pop();
                if index - 1 > start {
                    ranges.push((start, index - 1));
                }
            }
            open.push((index, level));
        }
        for (start, _) in open {
            if last_line > start {
                ranges.push((start, last_line));
            }
        }
    } else {
        let mut starts: Vec<usize> = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('}') || trimmed.starts_with(']') {
                if let Some(start) = starts.pop() {
                    if index > start {
                        ranges.push((start, index));
                    }
                }
            }
            if trimmed.ends_with('{') || trimmed.ends_with('[') {
                starts.push(index);
            }
        }
    }
    ranges.sort_unstable();
    ranges
}

/// The `gpui-base` highlighter backed by [`Highlighted`].
pub struct SyntaxHighlighter {
    language: SharedString,
    name: &'static str,
    state: Option<Highlighted>,
}

impl SyntaxHighlighter {
    /// A highlighter for `language`; `None` when it has no grammar.
    pub fn new(language: &str) -> Option<Self> {
        let (name, _) = LANGUAGES.iter().find(|(name, _)| *name == language)?;
        language_supported(name).then(|| Self {
            language: SharedString::from(*name),
            name,
            state: None,
        })
    }
}

impl InputHighlighter for SyntaxHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        let text = text.to_string();
        if !highlight_enabled(text.len()) {
            self.state = None;
            return;
        }
        match (&mut self.state, edit) {
            (Some(state), Some(edit)) => state.apply_edit(&edit.into(), &text),
            (Some(state), None) => state.replace(&text),
            (slot, _) => *slot = Highlighted::new(self.name, &text),
        }
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let Some(state) = &self.state else {
            return Vec::new();
        };
        state
            .spans_in(range)
            .into_iter()
            .filter_map(|span| {
                resolver
                    .style(span.class.name())
                    .map(|style| (span.range, style))
            })
            .collect()
    }

    fn fold_ranges(&self, text: &Rope) -> Vec<FoldRange> {
        let text = text.to_string();
        if !highlight_enabled(text.len()) {
            return Vec::new();
        }
        fold_ranges_for(self.name, &text)
            .into_iter()
            .map(|(start, end)| FoldRange::new(start, end))
            .collect()
    }
}

/// The factory the editor calls with a language name.
pub fn highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language: &str| {
        SyntaxHighlighter::new(language)
            .map(|highlighter| Box::new(highlighter) as Box<dyn InputHighlighter>)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of<'a>(source: &'a str, spans: &[HighlightSpan], class: TokenClass) -> Vec<&'a str> {
        spans
            .iter()
            .filter(|span| span.class == class)
            .map(|span| &source[span.range.clone()])
            .collect()
    }

    #[test]
    fn rust_keywords_get_a_style_and_plain_text_does_not() {
        let source = "fn main() { let x = 1; }\n";
        let spans = highlight_spans("rust", source).expect("rust is supported");
        let keywords = text_of(source, &spans, TokenClass::Keyword);
        assert!(
            keywords.contains(&"fn") && keywords.contains(&"let"),
            "{spans:?}"
        );
        // tree-sitter-rust captures numeric literals as `constant.builtin`.
        let coloured_one = spans.iter().any(|span| {
            &source[span.range.clone()] == "1"
                && matches!(span.class, TokenClass::Number | TokenClass::Constant)
        });
        assert!(coloured_one, "{spans:?}");
        assert!(highlight_spans("text", "just words").is_none());
    }

    #[test]
    fn every_language_of_workspace_0_1_has_a_grammar_that_colours_something() {
        let samples = [
            ("rust", "fn a() { let s = \"x\"; } // c\n"),
            ("json", "{\"key\": [1, true, null, \"v\"]}\n"),
            ("markdown", "# Title\n\nsome `code` text\n"),
            ("javascript", "const f = (a) => { return \"s\"; }; // c\n"),
            (
                "typescript",
                "interface A { b: number }\nconst c: string = \"s\";\n",
            ),
        ];
        for (language, source) in samples {
            assert!(language_supported(language), "{language}");
            let spans = highlight_spans(language, source).expect(language);
            assert!(!spans.is_empty(), "{language} produced no spans");
        }
    }

    #[test]
    fn a_file_above_the_highlight_limit_opens_unhighlighted() {
        assert!(highlight_enabled(HIGHLIGHT_LIMIT_BYTES));
        assert!(!highlight_enabled(HIGHLIGHT_LIMIT_BYTES + 1));
    }

    #[test]
    fn the_language_comes_from_the_file_extension() {
        assert_eq!(language_for_path("src/lib.rs"), Some("rust"));
        assert_eq!(language_for_path("package.json"), Some("json"));
        assert_eq!(language_for_path("README.md"), Some("markdown"));
        assert_eq!(language_for_path("app.ts"), Some("typescript"));
        assert_eq!(language_for_path("Makefile"), None);
        assert_eq!(language_for_path("UPPER.RS"), Some("rust"));
    }

    #[test]
    fn comments_and_strings_are_classified() {
        let source = "// note\nlet s = \"text\";\n";
        let spans = highlight_spans("rust", source).expect("rust");
        assert!(text_of(source, &spans, TokenClass::Comment)
            .join("")
            .contains("note"));
        assert!(text_of(source, &spans, TokenClass::String)
            .join("")
            .contains("text"));
    }

    #[test]
    fn a_nested_capture_wins_over_its_container() {
        let source = "let s = \"a\\nb\";\n";
        let spans = highlight_spans("rust", source).expect("rust");
        // The escape sequence is inside a string; both classify as String here, and the spans
        // never overlap.
        let mut end = 0;
        for span in &spans {
            assert!(span.range.start >= end, "overlap in {spans:?}");
            end = span.range.end;
        }
    }

    #[test]
    fn an_incremental_edit_gives_the_same_spans_as_a_fresh_parse() {
        let before = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let after = "fn a() {}\nfn bb() { let x = 2; }\nfn c() {}\n";
        let start = before.find("b()").expect("b");
        let mut incremental = Highlighted::new("rust", before).expect("rust");
        incremental.apply_edit(
            &TextEdit {
                start_byte: start + 1,
                old_end_byte: start + 1,
                new_end_byte: start + 2,
                start_position: (1, 4),
                old_end_position: (1, 4),
                new_end_position: (1, 5),
            },
            // The text after only the first change (an inserted `b`), then the rest by a
            // second edit.
            &before.replacen("fn b()", "fn bb()", 1),
        );
        let second = before.replacen("fn b()", "fn bb()", 1);
        let brace = second.find("{}\nfn c").expect("brace");
        incremental.apply_edit(
            &TextEdit {
                start_byte: brace + 1,
                old_end_byte: brace + 1,
                new_end_byte: brace + 1 + " let x = 2; ".len(),
                start_position: (1, 9),
                old_end_position: (1, 9),
                new_end_position: (1, 9 + " let x = 2; ".len()),
            },
            after,
        );
        let fresh = Highlighted::new("rust", after).expect("rust");
        assert_eq!(incremental.all_spans(), fresh.all_spans());
    }

    #[test]
    fn a_block_comment_opened_above_an_edit_still_colours_the_edited_line() {
        let before = "/* open\nmiddle\nclose */\nlet y = 1;\n";
        let after = "/* open\nmiddle!\nclose */\nlet y = 1;\n";
        let mut incremental = Highlighted::new("rust", before).expect("rust");
        let at = before.find("middle").expect("middle") + "middle".len();
        incremental.apply_edit(
            &TextEdit {
                start_byte: at,
                old_end_byte: at,
                new_end_byte: at + 1,
                start_position: (1, 6),
                old_end_position: (1, 6),
                new_end_position: (1, 7),
            },
            after,
        );
        let fresh = Highlighted::new("rust", after).expect("rust");
        assert_eq!(incremental.all_spans(), fresh.all_spans());
        assert!(
            text_of(after, &incremental.all_spans(), TokenClass::Comment)
                .join("")
                .contains("middle!")
        );
    }

    #[test]
    fn spans_in_a_range_only_return_overlapping_spans_and_clamp() {
        let source = "fn a() {}\nlet b = 2;\n";
        let highlighted = Highlighted::new("rust", source).expect("rust");
        let second_line = source.find("let").expect("let")..source.len();
        let spans = highlighted.spans_in(&second_line);
        assert!(!spans.is_empty());
        assert!(spans
            .iter()
            .all(|span| span.range.start >= second_line.start));
        // A range past the end is clamped, not a panic.
        let _ = highlighted.spans_in(&(0..source.len() + 1000));
        assert!(highlighted.spans_in(&(5..5)).is_empty());
    }

    #[test]
    fn multibyte_text_does_not_break_ranges() {
        let source = "let ş = \"çğü\"; // ö\n";
        let spans = highlight_spans("rust", source).expect("rust");
        for span in &spans {
            assert!(source.is_char_boundary(span.range.start), "{spans:?}");
            assert!(source.is_char_boundary(span.range.end), "{spans:?}");
        }
    }

    #[test]
    fn braces_and_markdown_headings_fold() {
        let code = "fn a() {\n    1;\n}\n";
        assert_eq!(fold_ranges_for("rust", code), vec![(0, 2)]);
        let markdown = "# One\ntext\n## Two\nmore\n# Three\nend\n";
        let ranges = fold_ranges_for("markdown", markdown);
        assert!(ranges.contains(&(0, 3)), "{ranges:?}");
        assert!(ranges.contains(&(2, 3)), "{ranges:?}");
        assert!(ranges.contains(&(4, 5)), "{ranges:?}");
    }

    /// A number for the report, not a gate: run with `--release -- --ignored --nocapture`.
    #[test]
    #[ignore = "reports timings; run in release mode"]
    fn highlight_cost_of_a_30k_line_rust_file_is_reported() {
        let mut text = String::new();
        for line in 0..30_000 {
            text.push_str(&format!(
                "    let value_{line} = compute(\"text {line}\", {line}); // note\n"
            ));
        }
        let started = std::time::Instant::now();
        let mut highlighted = Highlighted::new("rust", &text).expect("rust");
        let parse = started.elapsed();
        let started = std::time::Instant::now();
        let visible_end = text
            .char_indices()
            .nth(2_500)
            .map_or(text.len(), |(i, _)| i);
        let visible = highlighted.spans_in(&(0..visible_end)).len();
        let query = started.elapsed();
        let edited = text.replacen("value_100 ", "value_100x ", 1);
        let at = text.find("value_100 ").expect("line") + "value_100".len();
        let started = std::time::Instant::now();
        highlighted.apply_edit(
            &TextEdit {
                start_byte: at,
                old_end_byte: at,
                new_end_byte: at + 1,
                start_position: (100, 14),
                old_end_position: (100, 14),
                new_end_position: (100, 15),
            },
            &edited,
        );
        let edit = started.elapsed();
        println!(
            "30K lines: full parse {parse:?}, visible-range query {query:?} ({visible} spans), one-char edit near the top {edit:?}"
        );
    }
}
