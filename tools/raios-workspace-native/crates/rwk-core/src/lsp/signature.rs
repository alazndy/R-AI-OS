//! Reads a `textDocument/signatureHelp` answer into one line of text.

use serde_json::Value;

/// The active signature as one line, with the active parameter between `«` and `»`. `None` when
/// the server has no signature for the position.
pub fn signature_line(answer: &Value) -> Option<String> {
    let signatures = answer.get("signatures")?.as_array()?;
    let active = answer
        .get("activeSignature")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0);
    let signature = signatures.get(active).or_else(|| signatures.first())?;
    let label = signature.get("label")?.as_str()?;
    let index = signature
        .get("activeParameter")
        .or_else(|| answer.get("activeParameter"))
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok());
    let parameter = index
        .and_then(|index| signature.get("parameters")?.as_array()?.get(index))
        .and_then(|parameter| parameter.get("label"));
    let range = match parameter {
        Some(Value::String(text)) => label.find(text.as_str()).map(|at| at..at + text.len()),
        Some(Value::Array(bounds)) => {
            let start = utf16_to_byte(label, bounds.first()?.as_u64()?)?;
            let end = utf16_to_byte(label, bounds.get(1)?.as_u64()?)?;
            (start <= end).then_some(start..end)
        }
        _ => None,
    };
    Some(match range {
        Some(range) => format!(
            "{}«{}»{}",
            &label[..range.start],
            &label[range.start..range.end],
            &label[range.end..]
        ),
        None => label.to_owned(),
    })
}

/// Byte offset of the UTF-16 offset `units` in `text`, when it lies on a character boundary.
fn utf16_to_byte(text: &str, units: u64) -> Option<usize> {
    let mut seen = 0u64;
    for (byte, ch) in text.char_indices() {
        if seen == units {
            return Some(byte);
        }
        seen += ch.len_utf16() as u64;
    }
    (seen == units).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_active_parameter_is_marked_by_text_or_by_offsets() {
        let by_text = json!({
            "signatures": [{"label": "fn f(a: i32, b: u8)", "parameters": [{"label": "a: i32"}, {"label": "b: u8"}]}],
            "activeParameter": 1
        });
        assert_eq!(signature_line(&by_text).unwrap(), "fn f(a: i32, «b: u8»)");
        let by_offsets = json!({
            "signatures": [{"label": "f(é, b)", "parameters": [{"label": [2, 3]}, {"label": [5, 6]}], "activeParameter": 1}]
        });
        assert_eq!(signature_line(&by_offsets).unwrap(), "f(é, «b»)");
    }

    #[test]
    fn nothing_to_show_is_none_and_a_missing_parameter_keeps_the_label() {
        assert_eq!(signature_line(&json!({"signatures": []})), None);
        let plain = json!({"signatures": [{"label": "f()"}]});
        assert_eq!(signature_line(&plain).unwrap(), "f()");
    }
}
