//! Answers about where a symbol is used and what a file contains: `textDocument/references`
//! and `textDocument/documentSymbol`, read into plain lines the Problems-style list can show.

use lsp_types::{DocumentSymbol, DocumentSymbolResponse, Location, SymbolKind};
use serde_json::Value;

use crate::error::{AppError, ErrorCode, Result};

/// Most lines one answer may produce.
pub const MAX_LINES: usize = 500;

/// One place in a project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Project-relative path.
    pub path: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column.
    pub column: usize,
    /// What to show: the source line for a reference, `kind name` (indented) for a symbol.
    pub label: String,
}

fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "The language server's answer is not understood.",
    )
}

/// The places of a references answer, in file and line order. Places outside the project are
/// dropped. `label` is left empty; the caller fills it with the source line.
pub fn references(answer: Value, relative: impl Fn(&str) -> Option<String>) -> Result<Vec<Place>> {
    let locations: Option<Vec<Location>> = serde_json::from_value(answer).map_err(|_| invalid())?;
    let mut places: Vec<Place> = locations
        .unwrap_or_default()
        .into_iter()
        .filter_map(|location| {
            Some(Place {
                path: relative(location.uri.as_str())?,
                line: location.range.start.line as usize + 1,
                column: location.range.start.character as usize + 1,
                label: String::new(),
            })
        })
        .collect();
    places.sort_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
    places.dedup_by(|a, b| (&a.path, a.line, a.column) == (&b.path, b.line, b.column));
    places.truncate(MAX_LINES);
    Ok(places)
}

fn kind_word(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE | SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => {
            "mod"
        }
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY | SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "new",
        SymbolKind::ENUM | SymbolKind::ENUM_MEMBER => "enum",
        SymbolKind::INTERFACE => "trait",
        SymbolKind::FUNCTION => "fn",
        SymbolKind::VARIABLE => "var",
        SymbolKind::CONSTANT => "const",
        SymbolKind::STRUCT => "struct",
        SymbolKind::TYPE_PARAMETER => "type",
        _ => "sym",
    }
}

fn walk(symbols: Vec<DocumentSymbol>, depth: usize, path: &str, out: &mut Vec<Place>) {
    for symbol in symbols {
        if out.len() >= MAX_LINES {
            return;
        }
        out.push(Place {
            path: path.to_owned(),
            line: symbol.selection_range.start.line as usize + 1,
            column: symbol.selection_range.start.character as usize + 1,
            label: format!(
                "{}{} {}",
                "  ".repeat(depth.min(8)),
                kind_word(symbol.kind),
                symbol.name
            ),
        });
        walk(symbol.children.unwrap_or_default(), depth + 1, path, out);
    }
}

/// The symbols of `path` in file order, nested ones indented under their parent.
pub fn outline(answer: Value, path: &str) -> Result<Vec<Place>> {
    let response: Option<DocumentSymbolResponse> =
        serde_json::from_value(answer).map_err(|_| invalid())?;
    let mut out = Vec::new();
    match response {
        None => {}
        Some(DocumentSymbolResponse::Nested(symbols)) => walk(symbols, 0, path, &mut out),
        Some(DocumentSymbolResponse::Flat(symbols)) => {
            for symbol in symbols.into_iter().take(MAX_LINES) {
                out.push(Place {
                    path: path.to_owned(),
                    line: symbol.location.range.start.line as usize + 1,
                    column: symbol.location.range.start.character as usize + 1,
                    label: format!("{} {}", kind_word(symbol.kind), symbol.name),
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn range(line: u32, column: u32) -> Value {
        json!({"start": {"line": line, "character": column}, "end": {"line": line, "character": column + 1}})
    }

    #[test]
    fn references_are_sorted_deduplicated_and_kept_inside_the_project() {
        let rel = |uri: &str| uri.strip_prefix("file:///p/").map(str::to_owned);
        let answer = json!([
            {"uri": "file:///p/b.rs", "range": range(4, 2)},
            {"uri": "file:///p/a.rs", "range": range(9, 0)},
            {"uri": "file:///p/a.rs", "range": range(9, 0)},
            {"uri": "file:///elsewhere/x.rs", "range": range(1, 1)},
            {"uri": "file:///p/a.rs", "range": range(2, 7)},
        ]);
        let places = references(answer, rel).unwrap();
        let seen: Vec<_> = places
            .iter()
            .map(|p| (p.path.as_str(), p.line, p.column))
            .collect();
        assert_eq!(seen, [("a.rs", 3, 8), ("a.rs", 10, 1), ("b.rs", 5, 3)]);
        assert!(references(Value::Null, rel).unwrap().is_empty());
        assert!(references(json!("nope"), rel).is_err());
    }

    #[test]
    fn a_nested_outline_is_indented_and_a_flat_one_is_listed() {
        let nested = json!([
            {"name": "Shape", "kind": 23, "range": range(0, 0), "selectionRange": range(0, 7),
             "children": [{"name": "area", "kind": 6, "range": range(2, 4), "selectionRange": range(2, 7)}]},
            {"name": "main", "kind": 12, "range": range(8, 0), "selectionRange": range(8, 3)}
        ]);
        let labels: Vec<_> = outline(nested, "a.rs")
            .unwrap()
            .into_iter()
            .map(|p| (p.line, p.label))
            .collect();
        assert_eq!(
            labels,
            [
                (1, "struct Shape".to_owned()),
                (3, "  method area".to_owned()),
                (9, "fn main".to_owned())
            ]
        );
        let flat = json!([{"name": "go", "kind": 12,
            "location": {"uri": "file:///p/a.rs", "range": range(3, 0)}}]);
        assert_eq!(outline(flat, "a.rs").unwrap()[0].label, "fn go");
        assert!(outline(Value::Null, "a.rs").unwrap().is_empty());
    }
}
