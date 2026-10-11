//! Syntax highlighting for the editor.
//!
//! `gpui-base` ships no highlighter; it asks the application for one through the
//! [`InputHighlighter`] trait. This module implements it over `syntect` (pure-Rust regex engine).
//! Token classes are fixed names resolved to colours by the editor style, never by syntect's own
//! theme, so switching tone does not re-parse anything.

use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, TryLockError};
use std::time::Duration;

use gpui::{Context, HighlightStyle, SharedString, Window};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Files larger than this open without highlighting (spec section 12: cost on large files).
pub const HIGHLIGHT_LIMIT_BYTES: usize = 1024 * 1024;

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
    ("typescript", &["ts", "mts", "cts", "tsx"]),
];

/// The language for a file path, by extension.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        .map(|(language, _)| *language)
}

fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The grammar for a language name. TypeScript falls back to the JavaScript grammar when the
/// bundled set has none of its own.
fn syntax_for(language: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let (_, extensions) = LANGUAGES.iter().find(|(name, _)| *name == language)?;
    extensions
        .iter()
        .find_map(|extension| set.find_syntax_by_extension(extension))
        .or_else(|| {
            (language == "typescript")
                .then(|| set.find_syntax_by_extension("js"))
                .flatten()
        })
}

/// True when `language` has a grammar.
pub fn language_supported(language: &str) -> bool {
    syntax_for(language).is_some()
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
}

/// A highlighted byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// The class of the text in the range.
    pub class: TokenClass,
}

struct ScopeTable {
    entries: Vec<(Scope, TokenClass)>,
}

fn scope_table() -> &'static ScopeTable {
    static TABLE: OnceLock<ScopeTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let entry = |name: &str, class| Scope::new(name).ok().map(|scope| (scope, class));
        // Order matters: the first match for the innermost scope wins.
        let entries = [
            entry("comment", TokenClass::Comment),
            entry("string", TokenClass::String),
            entry("constant.numeric", TokenClass::Number),
            entry("constant", TokenClass::Constant),
            entry("entity.name.function", TokenClass::Function),
            entry("support.function", TokenClass::Function),
            entry("entity.name.type", TokenClass::Type),
            entry("entity.name.class", TokenClass::Type),
            entry("entity.name.struct", TokenClass::Type),
            entry("entity.name.enum", TokenClass::Type),
            entry("support.type", TokenClass::Type),
            entry("support.class", TokenClass::Type),
            entry("keyword", TokenClass::Keyword),
            // `fn`, `let`, `const` and modifiers are storage scopes in the bundled grammars.
            entry("storage", TokenClass::Keyword),
            entry("punctuation", TokenClass::Punctuation),
        ]
        .into_iter()
        .flatten()
        .collect();
        ScopeTable { entries }
    })
}

fn classify(stack: &ScopeStack) -> Option<TokenClass> {
    let table = scope_table();
    for scope in stack.as_slice().iter().rev() {
        for (prefix, class) in &table.entries {
            if prefix.is_prefix_of(*scope) {
                return Some(*class);
            }
        }
    }
    None
}

/// Lines parsed per lock hold. The UI thread waits at most this long when it needs the lock.
const CHUNK_LINES: usize = 100;
/// How often the UI is asked to repaint while a background parse is running.
const REPAINT_EVERY: Duration = Duration::from_millis(60);

/// Highlight state kept per line start, so an edit re-parses from its own line only.
#[derive(Clone)]
struct LineStart {
    parse: ParseState,
    stack: ScopeStack,
}

/// Highlighted text with the per-line parser state needed for incremental updates.
///
/// Parsing is resumable: [`Highlighted::begin`] fixes the text and the first line to
/// (re)compute, [`Highlighted::advance`] parses a bounded number of lines and says whether it is
/// done. Lines parsed so far are served while the rest is still pending.
pub struct Highlighted {
    syntax: &'static SyntaxReference,
    /// Byte length of the text the line table describes.
    text_len: usize,
    /// Byte offset of each line start.
    line_starts: Vec<usize>,
    /// Parser state at the start of each parsed line.
    states: Vec<LineStart>,
    /// Parser state at the start of the next unparsed line.
    cursor: LineStart,
    /// Spans per parsed line, in absolute byte offsets of the text they were computed for.
    spans: Vec<Vec<HighlightSpan>>,
}

impl Highlighted {
    /// An empty highlighter for `language`; `None` when it has no grammar.
    pub fn empty(language: &str) -> Option<Self> {
        let syntax = syntax_for(language)?;
        Some(Highlighted {
            syntax,
            text_len: 0,
            line_starts: Vec::new(),
            states: Vec::new(),
            cursor: LineStart {
                parse: ParseState::new(syntax),
                stack: ScopeStack::new(),
            },
            spans: Vec::new(),
        })
    }

    /// Highlights `text` completely; `None` when `language` has no grammar.
    pub fn new(language: &str, text: &str) -> Option<Self> {
        let mut highlighted = Self::empty(language)?;
        highlighted.rehighlight_from(text, 0);
        Some(highlighted)
    }

    /// The line a byte offset falls on, according to the text this was last begun for.
    pub fn line_of(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        }
    }

    /// Number of lines whose spans are available.
    pub fn parsed_lines(&self) -> usize {
        self.states.len()
    }

    /// True when every line of the text has been parsed.
    pub fn is_complete(&self) -> bool {
        self.states.len() >= self.line_starts.len()
    }

    /// Byte offset up to which spans are available.
    pub fn parsed_end(&self) -> usize {
        self.line_starts
            .get(self.states.len())
            .copied()
            .unwrap_or(self.text_len)
    }

    /// Fixes the text and keeps the lines before `first_line`: their spans stay, and the parser
    /// state at the start of `first_line` is rebuilt from the saved state of the line before it.
    pub fn begin(&mut self, text: &str, first_line: usize) {
        let first_line = first_line.min(self.states.len());
        self.states.truncate(first_line);
        self.spans.truncate(first_line);
        self.text_len = text.len();
        self.line_starts.clear();
        let mut position = 0;
        for line in LinesWithEndings::from(text) {
            self.line_starts.push(position);
            position += line.len();
        }
        self.cursor = if first_line == 0 {
            LineStart {
                parse: ParseState::new(self.syntax),
                stack: ScopeStack::new(),
            }
        } else {
            // Lines before the edit are unchanged: replay the line before `first_line` from its
            // saved start state to get the state at the start of `first_line`.
            let mut cursor = self.states[first_line - 1].clone();
            let start = self.line_starts[first_line - 1];
            let end = self
                .line_starts
                .get(first_line)
                .copied()
                .unwrap_or(text.len());
            if let Ok(ops) = cursor.parse.parse_line(&text[start..end], syntaxes()) {
                for (_, op) in ops {
                    let _ = cursor.stack.apply(&op);
                }
            }
            cursor
        };
    }

    /// Parses up to `max_lines` more lines of `text` (the text given to [`Self::begin`]).
    /// Returns true when the whole text has been parsed.
    pub fn advance(&mut self, text: &str, max_lines: usize) -> bool {
        let total = self.line_starts.len();
        let mut parsed = 0;
        while self.states.len() < total && parsed < max_lines {
            let index = self.states.len();
            let start = self.line_starts[index];
            let end = self
                .line_starts
                .get(index + 1)
                .copied()
                .unwrap_or(text.len());
            let line = &text[start..end];
            self.states.push(self.cursor.clone());
            let mut spans: Vec<HighlightSpan> = Vec::new();
            if let Ok(ops) = self.cursor.parse.parse_line(line, syntaxes()) {
                let mut last = 0;
                for (position, op) in ops {
                    push_span(
                        &mut spans,
                        &self.cursor.stack,
                        start + last,
                        start + position,
                    );
                    let _ = self.cursor.stack.apply(&op);
                    last = position;
                }
                push_span(
                    &mut spans,
                    &self.cursor.stack,
                    start + last,
                    start + line.len(),
                );
            }
            self.spans.push(spans);
            parsed += 1;
        }
        self.is_complete()
    }

    /// Recomputes from `first_line` to the end of `text` in one go.
    pub fn rehighlight_from(&mut self, text: &str, first_line: usize) {
        self.begin(text, first_line);
        self.advance(text, usize::MAX);
    }

    /// Every available span, in text order.
    pub fn all_spans(&self) -> Vec<HighlightSpan> {
        self.spans.iter().flatten().cloned().collect()
    }

    /// The available spans that overlap `range`.
    pub fn spans_in(&self, range: &Range<usize>) -> Vec<HighlightSpan> {
        let first = self.line_of(range.start);
        let last = self.line_of(range.end.saturating_sub(1).max(range.start));
        self.spans
            .iter()
            .skip(first)
            .take(last + 1 - first)
            .flatten()
            .filter(|span| span.range.start < range.end && span.range.end > range.start)
            .cloned()
            .collect()
    }
}

fn push_span(spans: &mut Vec<HighlightSpan>, stack: &ScopeStack, start: usize, end: usize) {
    if end <= start {
        return;
    }
    let Some(class) = classify(stack) else {
        return;
    };
    if let Some(previous) = spans.last_mut() {
        if previous.class == class && previous.range.end == start {
            previous.range.end = end;
            return;
        }
    }
    spans.push(HighlightSpan {
        range: start..end,
        class,
    });
}

/// All spans of `text` as `language`; `None` for an unsupported language.
pub fn highlight_spans(language: &str, text: &str) -> Option<Vec<HighlightSpan>> {
    Highlighted::new(language, text).map(|highlighted| highlighted.all_spans())
}

/// Moves `spans` across an edit that replaced `edit_start..old_end` with `edit_start..new_end`:
/// spans before it stay, spans after it shift by the length change, spans it touches are
/// dropped. Used to keep the old colours on screen while a background parse catches up.
pub fn shift_spans(
    spans: &[HighlightSpan],
    edit_start: usize,
    old_end: usize,
    new_end: usize,
) -> Vec<HighlightSpan> {
    let delta = new_end as i64 - old_end as i64;
    spans
        .iter()
        .filter_map(|span| {
            if span.range.end <= edit_start {
                Some(span.clone())
            } else if span.range.start >= old_end {
                let shift = |value: usize| (value as i64 + delta).max(0) as usize;
                Some(HighlightSpan {
                    range: shift(span.range.start)..shift(span.range.end),
                    class: span.class,
                })
            } else {
                None
            }
        })
        .collect()
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

/// State a background parse and the UI thread share.
type SharedHighlight = Arc<Mutex<Option<Highlighted>>>;

fn lock(shared: &SharedHighlight) -> std::sync::MutexGuard<'_, Option<Highlighted>> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Parses `text` in chunks on a background thread, publishing each chunk. It stops as soon as a
/// newer edit bumps `version`.
fn run_worker(
    shared: SharedHighlight,
    version: Arc<AtomicU64>,
    mine: u64,
    language: &'static str,
    text: String,
    first_line: usize,
) {
    {
        let mut guard = lock(&shared);
        if version.load(Ordering::Acquire) != mine {
            return;
        }
        match guard.as_mut() {
            Some(highlighted) => highlighted.begin(&text, first_line),
            None => {
                let Some(mut highlighted) = Highlighted::empty(language) else {
                    return;
                };
                highlighted.begin(&text, 0);
                *guard = Some(highlighted);
            }
        }
    }
    loop {
        let done = {
            let mut guard = lock(&shared);
            if version.load(Ordering::Acquire) != mine {
                return;
            }
            guard
                .as_mut()
                .map(|highlighted| highlighted.advance(&text, CHUNK_LINES))
                .unwrap_or(true)
        };
        if done {
            return;
        }
        std::thread::yield_now();
    }
}

/// The `gpui-base` highlighter. Parsing runs on a background thread; the UI thread only reads
/// what has been parsed (plus the previous colours, shifted across the edit, for the rest).
pub struct SyntectHighlighter {
    language: SharedString,
    name: &'static str,
    shared: SharedHighlight,
    version: Arc<AtomicU64>,
    /// Colours of the text as it was before the last edit, shifted across it. Used for the part
    /// the background parse has not reached yet.
    stale: Vec<HighlightSpan>,
}

impl SyntectHighlighter {
    /// A highlighter for `language`; `None` when it has no grammar.
    pub fn new(language: &str) -> Option<Self> {
        let (name, _) = LANGUAGES.iter().find(|(name, _)| *name == language)?;
        language_supported(name).then(|| Self {
            language: SharedString::from(*name),
            name,
            shared: Arc::new(Mutex::new(None)),
            version: Arc::new(AtomicU64::new(0)),
            stale: Vec::new(),
        })
    }

    /// What is on screen now: the parsed prefix plus the stale spans past it.
    fn displayed_spans(&self) -> Vec<HighlightSpan> {
        let guard = lock(&self.shared);
        let Some(highlighted) = guard.as_ref() else {
            return self.stale.clone();
        };
        let parsed_end = highlighted.parsed_end();
        let mut spans = highlighted.all_spans();
        spans.extend(
            self.stale
                .iter()
                .filter(|span| span.range.start >= parsed_end)
                .cloned(),
        );
        spans
    }
}

impl InputHighlighter for SyntectHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        let text = text.to_string();
        let mine = self.version.fetch_add(1, Ordering::AcqRel) + 1;
        if !highlight_enabled(text.len()) {
            *lock(&self.shared) = None;
            self.stale.clear();
            return;
        }
        let first_line = match (&edit, lock(&self.shared).as_ref()) {
            (Some(edit), Some(highlighted)) => highlighted.line_of(edit.start_byte),
            _ => 0,
        };
        self.stale = match edit {
            Some(edit) => shift_spans(
                &self.displayed_spans(),
                edit.start_byte,
                edit.old_end_byte,
                edit.new_end_byte,
            ),
            None => Vec::new(),
        };
        let shared = Arc::clone(&self.shared);
        let version = Arc::clone(&self.version);
        let name = self.name;
        cx.background_executor()
            .spawn(async move { run_worker(shared, version, mine, name, text, first_line) })
            .detach();
        // Ask for repaints while the background parse publishes chunks.
        let shared = Arc::clone(&self.shared);
        let version = Arc::clone(&self.version);
        cx.spawn(async move |editor, cx| loop {
            cx.background_executor().timer(REPAINT_EVERY).await;
            let superseded = version.load(Ordering::Acquire) != mine;
            let done = match shared.try_lock() {
                Ok(guard) => guard.as_ref().is_none_or(Highlighted::is_complete),
                Err(TryLockError::Poisoned(_)) => true,
                Err(TryLockError::WouldBlock) => false,
            };
            if editor.update(cx, |_, cx| cx.notify()).is_err() || superseded || done {
                break;
            }
        })
        .detach();
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut spans = Vec::new();
        match self.shared.try_lock() {
            Ok(guard) => {
                if let Some(highlighted) = guard.as_ref() {
                    let parsed_end = highlighted.parsed_end();
                    spans.extend(highlighted.spans_in(range));
                    spans.extend(
                        self.stale
                            .iter()
                            .filter(|span| {
                                span.range.start >= parsed_end
                                    && span.range.start < range.end
                                    && span.range.end > range.start
                            })
                            .cloned(),
                    );
                } else {
                    spans.extend(self.stale.iter().cloned());
                }
            }
            // The worker holds the lock for one chunk; show the previous colours meanwhile.
            Err(_) => spans.extend(
                self.stale
                    .iter()
                    .filter(|span| span.range.start < range.end && span.range.end > range.start)
                    .cloned(),
            ),
        }
        spans
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
        SyntectHighlighter::new(language)
            .map(|highlighter| Box::new(highlighter) as Box<dyn InputHighlighter>)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_keywords_get_a_style_and_plain_text_does_not() {
        let source = "fn main() { let x = 1; }\n";
        let spans = highlight_spans("rust", source).expect("rust is supported");
        assert!(
            spans.iter().any(
                |span| &source[span.range.clone()] == "fn" && span.class == TokenClass::Keyword
            ),
            "{spans:?}"
        );
        assert!(highlight_spans("text", "just words").is_none());
    }

    #[test]
    fn json_markdown_javascript_and_rust_are_supported_like_workspace_0_1() {
        for language in ["rust", "json", "markdown", "javascript", "typescript"] {
            assert!(language_supported(language), "{language}");
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
        let text_of = |class| {
            spans
                .iter()
                .filter(|span| span.class == class)
                .map(|span| &source[span.range.clone()])
                .collect::<String>()
        };
        assert!(text_of(TokenClass::Comment).contains("note"));
        assert!(text_of(TokenClass::String).contains("text"));
    }

    #[test]
    fn an_edit_re_highlights_from_its_own_line_and_matches_a_full_run() {
        let before = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let after = "fn a() {}\nfn bb() {}\nfn c() {}\n";
        let mut incremental = Highlighted::new("rust", before).expect("rust");
        let line = incremental.line_of(before.find("fn b").unwrap());
        incremental.rehighlight_from(after, line);
        let full = Highlighted::new("rust", after).expect("rust");
        assert_eq!(incremental.all_spans(), full.all_spans());
    }

    #[test]
    fn a_block_comment_opened_above_an_edit_still_colours_the_edited_line() {
        let before = "/* open\nmiddle\nclose */\nlet y = 1;\n";
        let after = "/* open\nmiddle!\nclose */\nlet y = 1;\n";
        let mut incremental = Highlighted::new("rust", before).expect("rust");
        incremental.rehighlight_from(after, 1);
        let full = Highlighted::new("rust", after).expect("rust");
        assert_eq!(incremental.all_spans(), full.all_spans());
    }

    #[test]
    fn spans_in_a_range_only_return_overlapping_spans() {
        let source = "fn a() {}\nlet b = 2;\n";
        let highlighted = Highlighted::new("rust", source).expect("rust");
        let second_line = source.find("let").unwrap()..source.len();
        let spans = highlighted.spans_in(&second_line);
        assert!(!spans.is_empty());
        assert!(spans.iter().all(|span| span.range.end > second_line.start));
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
        let full = started.elapsed();
        let edited = text.replacen("value_100 ", "value_100x ", 1);
        let started = std::time::Instant::now();
        highlighted.rehighlight_from(&edited, 100);
        let early = started.elapsed();
        let later = edited.replacen("value_29900 ", "value_29900x ", 1);
        let started = std::time::Instant::now();
        highlighted.rehighlight_from(&later, 29_900);
        let late = started.elapsed();
        println!(
            "30K lines: full {full:?}, edit at line 100 {early:?}, edit at line 29900 {late:?}"
        );
    }

    /// Splits the cost between syntect's regex parsing and this module's bookkeeping.
    #[test]
    #[ignore = "reports timings; run in release mode"]
    fn highlight_cost_breakdown_is_reported() {
        let mut text = String::new();
        for line in 0..3_000 {
            text.push_str(&format!(
                "    let value_{line} = compute(\"text {line}\", {line}); // note\n"
            ));
        }
        let syntax = syntax_for("rust").expect("rust");
        let started = std::time::Instant::now();
        let mut parse = ParseState::new(syntax);
        let mut ops_total = 0;
        for line in LinesWithEndings::from(&text) {
            ops_total += parse
                .parse_line(line, syntaxes())
                .map(|ops| ops.len())
                .unwrap_or(0);
        }
        let parse_only = started.elapsed();
        let started = std::time::Instant::now();
        let highlighted = Highlighted::new("rust", &text).expect("rust");
        let with_bookkeeping = started.elapsed();
        println!(
            "3K lines ({} bytes): syntect parse only {parse_only:?} ({ops_total} ops), full Highlighted {with_bookkeeping:?}, {} spans",
            text.len(),
            highlighted.all_spans().len()
        );
        let started = std::time::Instant::now();
        let _ = syntaxes();
        println!(
            "syntax set load (already loaded here): {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn parsing_in_chunks_publishes_a_prefix_and_ends_equal_to_a_full_run() {
        let text = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";
        let mut chunked = Highlighted::empty("rust").expect("rust");
        chunked.begin(text, 0);
        assert_eq!(chunked.parsed_lines(), 0);
        assert!(!chunked.advance(text, 1));
        assert_eq!(chunked.parsed_lines(), 1);
        assert_eq!(chunked.parsed_end(), "fn a() {}\n".len());
        assert!(chunked
            .all_spans()
            .iter()
            .all(|span| span.range.end <= chunked.parsed_end()));
        assert!(!chunked.advance(text, 2));
        assert!(chunked.advance(text, 10));
        assert_eq!(chunked.parsed_end(), text.len());
        let full = Highlighted::new("rust", text).expect("rust");
        assert_eq!(chunked.all_spans(), full.all_spans());
    }

    #[test]
    fn beginning_again_midway_keeps_the_earlier_lines_and_matches_a_full_run() {
        let before = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let after = "fn a() {}\nfn bb() {}\nfn c() {}\n";
        let mut highlighted = Highlighted::new("rust", before).expect("rust");
        highlighted.begin(after, 1);
        assert_eq!(highlighted.parsed_lines(), 1);
        assert!(highlighted.advance(after, 100));
        assert_eq!(
            highlighted.all_spans(),
            Highlighted::new("rust", after).expect("rust").all_spans()
        );
    }

    #[test]
    fn spans_shift_across_an_edit_and_the_touched_ones_are_dropped() {
        let span = |start: usize, end: usize| HighlightSpan {
            range: start..end,
            class: TokenClass::Keyword,
        };
        let spans = [span(0, 2), span(5, 9), span(12, 14)];
        // Bytes 4..10 were replaced by 4..16 (6 bytes became 12): +6.
        let shifted = shift_spans(&spans, 4, 10, 16);
        assert_eq!(shifted, vec![span(0, 2), span(18, 20)]);
        // A deletion shifts left.
        let shifted = shift_spans(&spans, 4, 10, 4);
        assert_eq!(shifted, vec![span(0, 2), span(6, 8)]);
    }
}
