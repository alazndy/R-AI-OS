//! The memory view's pure model: the heading outline of `memory.md` with checklist progress,
//! and a safe block rendering of one section. Raw HTML, links and images are never interpreted:
//! they stay inert text. Budgets bound the work for hostile or huge files.

/// Longest `memory.md` text the view renders, in characters.
pub const MAX_DOCUMENT_CHARS: usize = 64_000;
/// Most lines rendered for one section.
pub const MAX_SECTION_LINES: usize = 2_000;
/// Most table columns kept per row.
pub const MAX_TABLE_COLUMNS: usize = 32;
/// Most headings in the outline.
pub const MAX_HEADINGS: usize = 300;

/// One heading-delimited part of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySection {
    /// Stable identifier within one document (`section-<line>`, `intro` or `section-0`).
    pub id: String,
    /// Heading text.
    pub title: String,
    /// Heading level, 1 to 6.
    pub level: usize,
    /// The section's lines including its heading and nested sections.
    pub content: String,
    /// Completed checklist items.
    pub completed: usize,
    /// All checklist items.
    pub objectives: usize,
}

#[derive(Clone, Copy)]
struct Fence {
    marker: char,
    length: usize,
}

/// The fence a line opens or closes: the marker character and run length, when the line is a
/// fence line at all (up to three spaces of indentation, then three or more backticks or tildes).
fn fence_of(line: &str) -> Option<(Fence, bool)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = trimmed.chars().take_while(|c| *c == marker).count();
    if length < 3 {
        return None;
    }
    let rest = &trimmed[length..];
    Some((Fence { marker, length }, rest.trim().is_empty()))
}

/// Applies a fence line to the current fence state.
fn track_fence(state: &mut Option<Fence>, line: &str) -> bool {
    let Some((fence, bare)) = fence_of(line) else {
        return false;
    };
    match state {
        None => *state = Some(fence),
        Some(open) if open.marker == fence.marker && fence.length >= open.length && bare => {
            *state = None;
        }
        Some(_) => {}
    }
    true
}

fn heading_of(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = trimmed[level..].strip_prefix(char::is_whitespace)?;
    let title = rest.trim().trim_end_matches('#').trim();
    (!title.is_empty()).then(|| (level, title.to_owned()))
}

fn checklist_state(line: &str) -> Option<bool> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let rest = rest.strip_prefix('[')?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    if chars.next()? != ']' || !chars.next().is_some_and(char::is_whitespace) {
        return None;
    }
    match mark {
        ' ' => Some(false),
        'x' | 'X' => Some(true),
        _ => None,
    }
}

fn make_section(id: String, title: String, level: usize, lines: &[&str]) -> MemorySection {
    let mut completed = 0;
    let mut objectives = 0;
    let mut fence = None;
    for line in lines {
        if track_fence(&mut fence, line) || fence.is_some() {
            continue;
        }
        if let Some(done) = checklist_state(line) {
            objectives += 1;
            completed += usize::from(done);
        }
    }
    MemorySection {
        id,
        title,
        level,
        content: lines.join("\n"),
        completed,
        objectives,
    }
}

/// The outline of `content`: one section per heading (at most [`MAX_HEADINGS`]), each running to
/// the next heading of the same or a higher level. Text before the first heading is the
/// `intro` section; a document without headings is one `Genel` section.
pub fn sections(content: &str) -> Vec<MemorySection> {
    let text: String = content.chars().take(MAX_DOCUMENT_CHARS).collect();
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    let mut fence = None;
    for (index, line) in lines.iter().enumerate() {
        if track_fence(&mut fence, line) || fence.is_some() {
            continue;
        }
        if let Some((level, title)) = heading_of(line) {
            if headings.len() < MAX_HEADINGS {
                headings.push((index, level, title));
            }
        }
    }
    if headings.is_empty() {
        return vec![make_section("section-0".into(), "Genel".into(), 1, &lines)];
    }
    let mut out = Vec::new();
    if headings[0].0 > 0
        && lines[..headings[0].0]
            .iter()
            .any(|line| !line.trim().is_empty())
    {
        out.push(make_section(
            "intro".into(),
            "Giriş".into(),
            1,
            &lines[..headings[0].0],
        ));
    }
    for (position, (index, level, title)) in headings.iter().enumerate() {
        let end = headings[position + 1..]
            .iter()
            .find(|(_, other, _)| other <= level)
            .map_or(lines.len(), |(next, _, _)| *next);
        out.push(make_section(
            format!("section-{index}"),
            title.clone(),
            *level,
            &lines[*index..end],
        ));
    }
    out
}

/// One drawn block of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading.
    Heading {
        /// Level 1 to 6.
        level: usize,
        /// Plain text.
        text: String,
    },
    /// A paragraph line.
    Paragraph(String),
    /// A list item, possibly a checklist item.
    Item {
        /// Nesting depth (two spaces per level).
        depth: usize,
        /// `Some(done)` for `[ ]` / `[x]` items.
        checked: Option<bool>,
        /// Plain text.
        text: String,
    },
    /// A line of a fenced code block.
    Code(String),
    /// A table row.
    Row(Vec<String>),
    /// The section was cut at [`MAX_SECTION_LINES`] lines.
    Truncated,
}

/// Removes inline markup that could make text look interactive: `[text](url)` becomes `text`,
/// images become their alt text, emphasis and code markers are dropped. Nothing is fetched or
/// opened; HTML tags stay as visible text.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            i += 1;
            continue;
        }
        if c == '[' {
            if let Some(close) = chars[i..].iter().position(|ch| *ch == ']') {
                let close = i + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|ch| *ch == ')') {
                        out.extend(&chars[i + 1..close]);
                        i = close + end + 1;
                        continue;
                    }
                }
            }
        }
        if matches!(c, '`' | '*' | '_') && chars.get(i + 1) == Some(&c) {
            i += 2;
            continue;
        }
        if c == '`' || c == '*' {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() < 3 {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.chars().all(|c| matches!(c, '-' | ':' | '|' | ' ')) {
        return Some(Vec::new());
    }
    Some(
        inner
            .split('|')
            .take(MAX_TABLE_COLUMNS)
            .map(|cell| plain(cell.trim()))
            .collect(),
    )
}

/// The blocks of one section's content, at most [`MAX_SECTION_LINES`] lines.
pub fn blocks(content: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut fence = None;
    for (count, line) in content.split('\n').enumerate() {
        if count >= MAX_SECTION_LINES {
            out.push(Block::Truncated);
            break;
        }
        if track_fence(&mut fence, line) {
            continue;
        }
        if fence.is_some() {
            out.push(Block::Code(line.to_owned()));
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some((level, text)) = heading_of(line) {
            out.push(Block::Heading {
                level,
                text: plain(&text),
            });
        } else if let Some(cells) = table_cells(line) {
            if !cells.is_empty() {
                out.push(Block::Row(cells));
            }
        } else if let Some(item) = item_of(line) {
            out.push(item);
        } else {
            out.push(Block::Paragraph(plain(line.trim())));
        }
    }
    out
}

fn item_of(line: &str) -> Option<Block> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let checked = checklist_state(line);
    let text = if checked.is_some() {
        body.get(4..).unwrap_or("")
    } else {
        body
    };
    Some(Block::Item {
        depth: indent / 2,
        checked,
        text: plain(text.trim()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "intro line\n\n# Proje\n\n## Hedefler\n- [x] bir\n- [ ] iki\n- not a task\n\n## Notlar\n```\n# not a heading\n- [ ] not counted\n```\ntext\n";

    #[test]
    fn the_outline_nests_sections_and_keeps_an_intro() {
        let found = sections(DOC);

        let titles: Vec<_> = found.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Giriş", "Proje", "Hedefler", "Notlar"]);
        let project = &found[1];
        assert_eq!(project.level, 1);
        assert!(
            project.content.contains("## Notlar"),
            "a parent keeps its children"
        );
        assert!(!found[2].content.contains("## Notlar"));
    }

    #[test]
    fn checklist_progress_ignores_fenced_code() {
        let found = sections(DOC);

        assert_eq!((found[2].completed, found[2].objectives), (1, 2));
        assert_eq!((found[3].completed, found[3].objectives), (0, 0));
        assert!(found.iter().all(|s| s.title != "not a heading"));
    }

    #[test]
    fn a_document_without_headings_is_one_general_section() {
        let found = sections("just text\n- [ ] a");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Genel");
        assert_eq!(found[0].objectives, 1);
    }

    #[test]
    fn at_most_300_headings_are_listed() {
        let many = (0..400).map(|i| format!("# h{i}\n")).collect::<String>();

        assert_eq!(sections(&many).len(), MAX_HEADINGS);
    }

    #[test]
    fn crlf_and_tilde_fences_work() {
        let found = sections("# A\r\n~~~\r\n# fake\r\n~~~\r\n# B\r\n");

        let titles: Vec<_> = found.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"]);
    }

    #[test]
    fn links_images_and_markup_become_inert_plain_text() {
        assert_eq!(
            plain("see [docs](https://evil.example/x) now"),
            "see docs now"
        );
        assert_eq!(plain("![alt](http://x/y.png)"), "alt");
        assert_eq!(
            plain("**bold** and `code` and __under__"),
            "bold and code and under"
        );
        assert_eq!(
            plain("<script>alert(1)</script>"),
            "<script>alert(1)</script>"
        );
    }

    #[test]
    fn blocks_cover_headings_lists_code_and_tables() {
        let found = blocks("## T\n- [x] done\n  - nested\n```\ncode line\n```\n| a | b |\n|---|---|\n| 1 | 2 |\npara **x**");

        assert_eq!(
            found,
            vec![
                Block::Heading {
                    level: 2,
                    text: "T".into()
                },
                Block::Item {
                    depth: 0,
                    checked: Some(true),
                    text: "done".into()
                },
                Block::Item {
                    depth: 1,
                    checked: None,
                    text: "nested".into()
                },
                Block::Code("code line".into()),
                Block::Row(vec!["a".into(), "b".into()]),
                Block::Row(vec!["1".into(), "2".into()]),
                Block::Paragraph("para x".into()),
            ]
        );
    }

    #[test]
    fn a_wide_table_keeps_32_columns_and_a_long_section_is_cut() {
        let wide = format!(
            "|{}|",
            (0..50).map(|i| i.to_string()).collect::<Vec<_>>().join("|")
        );
        let found = blocks(&wide);
        assert!(matches!(&found[0], Block::Row(cells) if cells.len() == MAX_TABLE_COLUMNS));

        let long = "line\n".repeat(MAX_SECTION_LINES + 10);
        let found = blocks(&long);
        assert_eq!(found.last(), Some(&Block::Truncated));
        assert_eq!(found.len(), MAX_SECTION_LINES + 1);
    }

    #[test]
    fn text_past_the_document_budget_is_not_parsed() {
        let huge = format!("# A\n{}\n# Z\n", "x".repeat(MAX_DOCUMENT_CHARS));

        let found = sections(&huge);

        assert_eq!(found.len(), 1, "the heading after the budget is never seen");
    }
}
