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
const LANGUAGES: [(&str, &[&str]); 20] = [
    ("rust", &["rs"]),
    ("json", &["json", "jsonc", "webmanifest"]),
    ("markdown", &["md", "markdown"]),
    ("javascript", &["js", "mjs", "cjs", "jsx"]),
    ("typescript", &["ts", "mts", "cts"]),
    ("tsx", &["tsx"]),
    ("python", &["py", "pyi", "pyw"]),
    ("bash", &["sh", "bash", "zsh"]),
    ("html", &["html", "htm"]),
    ("css", &["css"]),
    ("go", &["go"]),
    ("c", &["c", "h"]),
    ("cpp", &["cpp", "cc", "cxx", "hpp", "hh", "hxx"]),
    ("toml", &["toml"]),
    ("yaml", &["yml", "yaml"]),
    ("r", &["r"]),
    ("java", &["java"]),
    ("kotlin", &["kt", "kts"]),
    ("sql", &["sql"]),
    ("xml", &["xml", "svg", "xsd", "xsl", "plist"]),
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
    /// Keys and field names (TOML, YAML, JSON, CSS properties).
    Property,
    /// Markup tags (HTML, XML).
    Tag,
    /// Attributes and annotations.
    Attribute,
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
            TokenClass::Property => "property",
            TokenClass::Tag => "tag",
            TokenClass::Attribute => "attribute",
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
            "property" | "field" | "label" => Some(Self::Property),
            "tag" => Some(Self::Tag),
            "attribute" | "annotation" => Some(Self::Attribute),
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
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            vec![
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ],
        ),
        "python" => (
            tree_sitter_python::LANGUAGE.into(),
            vec![tree_sitter_python::HIGHLIGHTS_QUERY],
        ),
        "bash" => (
            tree_sitter_bash::LANGUAGE.into(),
            vec![tree_sitter_bash::HIGHLIGHT_QUERY],
        ),
        "html" => (
            tree_sitter_html::LANGUAGE.into(),
            vec![tree_sitter_html::HIGHLIGHTS_QUERY],
        ),
        "css" => (
            tree_sitter_css::LANGUAGE.into(),
            vec![tree_sitter_css::HIGHLIGHTS_QUERY],
        ),
        "go" => (
            tree_sitter_go::LANGUAGE.into(),
            vec![tree_sitter_go::HIGHLIGHTS_QUERY],
        ),
        "c" => (
            tree_sitter_c::LANGUAGE.into(),
            vec![tree_sitter_c::HIGHLIGHT_QUERY],
        ),
        "cpp" => (
            tree_sitter_cpp::LANGUAGE.into(),
            vec![
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ],
        ),
        "toml" => (
            tree_sitter_toml_ng::LANGUAGE.into(),
            vec![tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        ),
        "yaml" => (
            tree_sitter_yaml::LANGUAGE.into(),
            vec![tree_sitter_yaml::HIGHLIGHTS_QUERY],
        ),
        "r" => (
            tree_sitter_r::LANGUAGE.into(),
            vec![tree_sitter_r::HIGHLIGHTS_QUERY],
        ),
        "java" => (
            tree_sitter_java::LANGUAGE.into(),
            vec![tree_sitter_java::HIGHLIGHTS_QUERY],
        ),
        "sql" => (
            tree_sitter_sequel::LANGUAGE.into(),
            vec![tree_sitter_sequel::HIGHLIGHTS_QUERY],
        ),
        "xml" => (
            tree_sitter_xml::LANGUAGE_XML.into(),
            vec![tree_sitter_xml::XML_HIGHLIGHT_QUERY],
        ),
        "kotlin" => {
            let language: Language = tree_sitter_kotlin_ng::LANGUAGE.into();
            let query = kotlin_query(&language);
            return grammar_from(language, &query);
        }
        _ => return None,
    };
    // Later patterns win when two match the same node, so the base language's query goes first.
    grammar_from(language, &sources.join("\n"))
}

/// The Kotlin grammar ships no highlight query, so this one is written here. Every pattern is
/// compiled on its own and only the ones this grammar version accepts are kept, so a renamed
/// node in a newer grammar loses a colour instead of the whole language.
fn kotlin_query(language: &Language) -> String {
    const KEYWORDS: &[&str] = &[
        "package",
        "import",
        "class",
        "interface",
        "object",
        "fun",
        "val",
        "var",
        "typealias",
        "if",
        "else",
        "when",
        "for",
        "while",
        "do",
        "return",
        "break",
        "continue",
        "throw",
        "try",
        "catch",
        "finally",
        "is",
        "in",
        "as",
        "this",
        "super",
        "companion",
        "constructor",
        "init",
        "by",
        "where",
        "get",
        "set",
        "override",
        "open",
        "abstract",
        "final",
        "private",
        "public",
        "protected",
        "internal",
        "data",
        "sealed",
        "enum",
        "annotation",
        "inner",
        "suspend",
        "inline",
        "lateinit",
        "const",
        "vararg",
        "operator",
        "infix",
        "tailrec",
    ];
    let mut patterns: Vec<String> = vec![
        "(line_comment) @comment".into(),
        "(block_comment) @comment".into(),
        "(shebang) @comment".into(),
        "(string_literal) @string".into(),
        "(multiline_string_literal) @string".into(),
        "(character_literal) @string".into(),
        "(escape_sequence) @string.escape".into(),
        "(number_literal) @number".into(),
        "(float_literal) @number".into(),
        "(annotation) @attribute".into(),
        "(user_type (identifier) @type)".into(),
        "(class_declaration name: (identifier) @type)".into(),
        "(object_declaration name: (identifier) @type)".into(),
        "(function_declaration name: (identifier) @function)".into(),
        "(call_expression (identifier) @function)".into(),
        "(call_expression (navigation_expression (identifier) @function .))".into(),
        "(this_expression) @constant".into(),
    ];
    patterns.extend(KEYWORDS.iter().map(|word| format!("\"{word}\" @keyword")));
    patterns.extend(
        [
            "\"true\" @constant",
            "\"false\" @constant",
            "\"null\" @constant",
        ]
        .map(String::from),
    );
    patterns
        .into_iter()
        .filter(|pattern| Query::new(language, pattern).is_ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn grammar_from(language: Language, source: &str) -> Option<Grammar> {
    let query = Query::new(&language, source).ok()?;
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

    /// Length of the parsed text in bytes.
    pub fn text_len(&self) -> usize {
        self.text.len()
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
    } else if matches!(language, "python" | "yaml" | "html" | "xml") {
        // Structure by indentation: a line folds with the deeper lines under it.
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut last = 0;
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            while let Some(&(start, open_indent)) = stack.last() {
                if open_indent < indent {
                    break;
                }
                stack.pop();
                if last > start {
                    ranges.push((start, last));
                }
            }
            stack.push((index, indent));
            last = index;
        }
        for (start, _) in stack {
            if last > start {
                ranges.push((start, last));
            }
        }
    } else if language == "toml" {
        // A `[table]` header folds with the keys under it, up to the next header.
        let mut open: Option<usize> = None;
        let mut last = 0;
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if line.trim_start().starts_with('[') {
                if let Some(start) = open {
                    if last > start {
                        ranges.push((start, last));
                    }
                }
                open = Some(index);
            }
            last = index;
        }
        if let Some(start) = open {
            if last > start {
                ranges.push((start, last));
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
        // The editor lays text runs out from these ranges, so they must cover `range` without
        // gaps: unstyled stretches get the default style.
        let end = range.end.min(state.text_len());
        let mut covered = range.start.min(end);
        let mut styles = Vec::new();
        for span in state.spans_in(range) {
            let Some(style) = resolver.style(span.class.name()) else {
                continue;
            };
            if span.range.start > covered {
                styles.push((covered..span.range.start, HighlightStyle::default()));
            }
            covered = span.range.end;
            styles.push((span.range, style));
        }
        if covered < end {
            styles.push((covered..end, HighlightStyle::default()));
        }
        styles
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

    struct EveryClassRed;

    impl HighlightStyleResolver for EveryClassRed {
        fn style(&self, _name: &str) -> Option<HighlightStyle> {
            Some(HighlightStyle {
                color: Some(gpui::red()),
                ..HighlightStyle::default()
            })
        }
    }

    #[test]
    fn styles_cover_the_whole_range_without_gaps() {
        // The editor lays text runs out from these ranges; a gap shifts every later colour.
        let source = "// note\nfn main() { let x = 1; }\n\nstruct A;\n";
        let mut highlighter = SyntaxHighlighter::new("rust").expect("rust is supported");
        highlighter.state = Highlighted::new("rust", source);
        for range in [0..source.len(), 9..30, 0..4] {
            let styles = highlighter.styles(&range, &EveryClassRed);
            let mut at = range.start;
            for (piece, _) in &styles {
                assert_eq!(piece.start, at, "gap or overlap in {styles:?}");
                at = piece.end;
            }
            assert_eq!(at, range.end, "range not covered: {styles:?}");
        }
        let all = highlighter.styles(&(0..source.len()), &EveryClassRed);
        assert!(all.iter().any(|(_, style)| style.color.is_none()));
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
    fn indentation_and_toml_tables_fold() {
        let python = "def a():\n    x = 1\n    if x:\n        y = 2\n\nz = 3\n";
        assert_eq!(fold_ranges_for("python", python), vec![(0, 3), (2, 3)]);
        let yaml = "a:\n  b: 1\n  c:\n    d: 2\ne: 3\n";
        assert_eq!(fold_ranges_for("yaml", yaml), vec![(0, 3), (2, 3)]);
        let toml = "[a]\nx = 1\n\n[b]\ny = 2\nz = 3\n";
        assert_eq!(fold_ranges_for("toml", toml), vec![(0, 1), (3, 5)]);
    }

    #[test]
    fn every_language_in_the_table_has_a_working_grammar_and_colours_its_keywords_or_keys() {
        let samples: [(&str, &str); 20] = [
            ("rust", "fn main() { let x = 1; }"),
            ("json", "{\"a\": 1}"),
            ("markdown", "# Title\n\ntext"),
            ("javascript", "const a = 1; function f() {}"),
            ("typescript", "const a: number = 1; interface I {}"),
            ("tsx", "const a = <div className=\"x\">hi</div>;"),
            ("python", "def f(x):\n    return 1  # c\n"),
            ("bash", "if [ -f x ]; then echo \"hi\"; fi"),
            ("html", "<div class=\"a\">hi</div>"),
            ("css", "a { color: red; }"),
            ("go", "package main\nfunc main() {}\n"),
            ("c", "int main(void) { return 0; }"),
            ("cpp", "class A { public: int f(); };"),
            ("toml", "[package]\nname = \"x\"\n"),
            ("yaml", "name: x\nlist:\n  - 1\n"),
            ("r", "f <- function(x) { x + 1 }"),
            ("java", "class A { void f() { int x = 1; } }"),
            ("kotlin", "fun main() { val x = \"a\" // c\n }"),
            ("sql", "SELECT a FROM t WHERE b = 1;"),
            ("xml", "<a b=\"c\">d</a>"),
        ];
        for (language, text) in samples {
            let spans = highlight_spans(language, text)
                .unwrap_or_else(|| panic!("{language} has no grammar"));
            assert!(!spans.is_empty(), "{language} coloured nothing");
            let covered: usize = spans.iter().map(|span| span.range.len()).sum();
            assert!(
                covered * 5 >= text.len().min(8),
                "{language}: {covered} bytes coloured"
            );
        }
        // A file's extension finds its language.
        for (path, language) in [
            ("a.tsx", "tsx"),
            ("a.kt", "kotlin"),
            ("deploy.yml", "yaml"),
            ("Cargo.toml", "toml"),
            ("run.sh", "bash"),
            ("x.R", "r"),
        ] {
            assert_eq!(language_for_path(path), Some(language), "{path}");
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
