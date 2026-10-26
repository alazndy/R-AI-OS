//! Text edits from a language server (formatting, code actions).

use lsp_types::{Position, TextEdit};

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
}
