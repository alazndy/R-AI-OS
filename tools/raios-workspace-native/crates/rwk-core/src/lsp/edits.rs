//! Text edits from a language server (formatting, code actions).

use lsp_types::{DocumentChanges, OneOf, Position, TextEdit, WorkspaceEdit};
use serde_json::Value;

use crate::error::{AppError, ErrorCode, Result};

/// Most edits one answer may carry; a bigger answer is more likely a bug than a refactoring.
pub const MAX_EDITS: usize = 10_000;

fn key(position: Position) -> (u32, u32) {
    (position.line, position.character)
}

/// Orders `edits` so they can be applied one after another on the original text: last
/// position first, so an applied edit never moves the ones still to come. Overlapping edits
/// are refused (the protocol forbids them).
pub fn in_apply_order(mut edits: Vec<TextEdit>) -> Result<Vec<TextEdit>> {
    if edits.len() > MAX_EDITS {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The language server sent too many edits.",
        ));
    }
    // Stable: edits that start at the same place keep the server's order.
    edits.sort_by_key(|edit| key(edit.range.start));
    for pair in edits.windows(2) {
        if key(pair[0].range.end) > key(pair[1].range.start) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "The language server sent overlapping edits.",
            ));
        }
    }
    edits.reverse();
    Ok(edits)
}

/// Most files one refactoring may touch.
pub const MAX_FILES: usize = 500;

/// The edits of one project file, ready to apply (last position first).
#[derive(Debug, Clone, PartialEq)]
pub struct FileEdits {
    /// Project-relative path.
    pub path: String,
    /// The edits, last position first.
    pub edits: Vec<TextEdit>,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message.to_owned())
}

/// Reads a server's `WorkspaceEdit` answer (a rename, say) into per-file edits. `relative`
/// maps a `file://` URI to a project path; an edit outside the project, and any file
/// creation, rename or deletion, refuse the whole answer, so a refactoring is applied
/// completely or not at all.
pub fn workspace_edit(
    answer: Value,
    relative: impl Fn(&str) -> Option<String>,
) -> Result<Vec<FileEdits>> {
    let edit: Option<WorkspaceEdit> = serde_json::from_value(answer)
        .map_err(|_| invalid("The language server's edit is not understood."))?;
    let Some(edit) = edit else {
        return Ok(Vec::new());
    };
    let mut by_uri: Vec<(String, Vec<TextEdit>)> = Vec::new();
    let mut add =
        |uri: &str, edits: Vec<TextEdit>| match by_uri.iter_mut().find(|(known, _)| known == uri) {
            Some((_, known)) => known.extend(edits),
            None => by_uri.push((uri.to_owned(), edits)),
        };
    for (uri, edits) in edit.changes.unwrap_or_default() {
        add(uri.as_str(), edits);
    }
    match edit.document_changes {
        None => {}
        Some(DocumentChanges::Edits(documents)) => {
            for document in documents {
                let edits = document
                    .edits
                    .into_iter()
                    .map(|edit| match edit {
                        OneOf::Left(edit) => edit,
                        OneOf::Right(annotated) => annotated.text_edit,
                    })
                    .collect();
                add(document.text_document.uri.as_str(), edits);
            }
        }
        Some(DocumentChanges::Operations(_)) => {
            return Err(invalid(
                "The language server wants to create, rename or delete files; that is not applied.",
            ));
        }
    }
    if by_uri.len() > MAX_FILES {
        return Err(invalid(
            "The language server's edit touches too many files.",
        ));
    }
    let mut files = Vec::new();
    for (uri, edits) in by_uri {
        let path = relative(&uri)
            .ok_or_else(|| invalid("The language server's edit reaches outside the project."))?;
        files.push(FileEdits {
            path,
            edits: in_apply_order(edits)?,
        });
    }
    Ok(files)
}

/// The byte offset of `position` in `text`; LSP columns count UTF-16 units. A column past the
/// end of its line stops at the line end.
fn offset_of(text: &str, position: Position) -> Option<usize> {
    let mut start = 0;
    for _ in 0..position.line {
        start += text[start..].find('\n')? + 1;
    }
    let line = &text[start..];
    let line = &line[..line.find('\n').unwrap_or(line.len())];
    let mut units = 0u32;
    for (index, character) in line.char_indices() {
        if units >= position.character {
            return Some(start + index);
        }
        units += u32::try_from(character.len_utf16()).ok()?;
    }
    Some(start + line.len())
}

/// The identifier under (or just before) the UTF-16 column of `line`.
pub fn word_at(line: &str, column: u32) -> Option<String> {
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut units = 0u32;
    let mut characters = Vec::new();
    for character in line.chars() {
        characters.push((units, character));
        units += u32::try_from(character.len_utf16()).ok()?;
    }
    // The cursor sits between characters: prefer the one it is in front of, else the one before.
    let at = characters
        .iter()
        .position(|(start, character)| *start >= column && is_word(*character))
        .filter(|&index| characters[index].0 == column)
        .or_else(|| {
            characters.iter().rposition(|(start, character)| {
                is_word(*character) && *start + character.len_utf16() as u32 == column
            })
        })?;
    let mut from = at;
    while from > 0 && is_word(characters[from - 1].1) {
        from -= 1;
    }
    let mut to = at;
    while to + 1 < characters.len() && is_word(characters[to + 1].1) {
        to += 1;
    }
    Some(characters[from..=to].iter().map(|(_, c)| *c).collect())
}

/// Applies `edits` (last position first, as [`in_apply_order`] returns them) to `text`.
pub fn apply(text: &str, edits: &[TextEdit]) -> Result<String> {
    let mut result = text.to_owned();
    for edit in edits {
        let (Some(from), Some(to)) = (
            offset_of(&result, edit.range.start),
            offset_of(&result, edit.range.end),
        ) else {
            return Err(invalid("An edit points outside the file."));
        };
        if from > to {
            return Err(invalid("An edit has a reversed range."));
        }
        result.replace_range(from..to, &edit.new_text);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Range;

    fn edit(line: u32, from: u32, to: u32, text: &str) -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(line, from), Position::new(line, to)),
            new_text: text.to_owned(),
        }
    }

    #[test]
    fn edits_come_back_last_first_and_overlaps_are_refused() {
        let ordered = in_apply_order(vec![
            edit(0, 0, 1, "a"),
            edit(2, 0, 0, "c"),
            edit(1, 0, 1, "b"),
        ])
        .unwrap();
        let lines: Vec<_> = ordered.iter().map(|e| e.range.start.line).collect();
        assert_eq!(lines, [2, 1, 0]);

        // Touching edits are fine; overlapping ones are not.
        assert!(in_apply_order(vec![edit(0, 0, 2, "x"), edit(0, 2, 3, "y")]).is_ok());
        assert!(in_apply_order(vec![edit(0, 0, 3, "x"), edit(0, 2, 4, "y")]).is_err());
    }

    #[test]
    fn edits_apply_with_utf16_columns_and_line_ends() {
        // "é" is one UTF-16 unit, "😀" two.
        let ordered = in_apply_order(vec![edit(0, 3, 4, "Y"), edit(1, 0, 99, "z")]).unwrap();
        assert_eq!(apply("é😀x\nabc", &ordered).unwrap(), "é😀Y\nz");
        assert!(apply("a", &[edit(5, 0, 0, "x")]).is_err());
    }

    #[test]
    fn a_workspace_edit_becomes_project_paths_or_is_refused_whole() {
        let uri = |name: &str| format!("file:///p/{name}");
        let rel = |uri: &str| uri.strip_prefix("file:///p/").map(str::to_owned);
        let answer = serde_json::json!({"changes": {
            uri("a.rs"): [{"range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":1}}, "newText": "x"}],
            uri("b.rs"): [{"range": {"start": {"line":2,"character":0}, "end": {"line":2,"character":1}}, "newText": "y"}],
        }});
        let files = workspace_edit(answer, rel).unwrap();
        let mut paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["a.rs", "b.rs"]);

        let outside = serde_json::json!({"changes": {"file:///etc/x": []}});
        assert!(workspace_edit(outside, rel).is_err());
        let rename = serde_json::json!({"documentChanges": [
            {"kind": "rename", "oldUri": "file:///p/a", "newUri": "file:///p/b"}]});
        assert!(workspace_edit(rename, rel).is_err());
        assert!(workspace_edit(serde_json::Value::Null, rel)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_word_under_the_cursor_is_found_at_its_edges() {
        assert_eq!(word_at("let foo_bar = 1;", 6).as_deref(), Some("foo_bar"));
        assert_eq!(word_at("let foo_bar = 1;", 4).as_deref(), Some("foo_bar"));
        assert_eq!(word_at("let foo_bar = 1;", 11).as_deref(), Some("foo_bar"));
        assert_eq!(word_at("a + b", 1), Some("a".to_owned()));
        assert_eq!(word_at("a + b", 2), None);
        assert_eq!(word_at("   ", 1), None);
        assert_eq!(word_at("😀 ab", 3).as_deref(), Some("ab"));
    }
}
