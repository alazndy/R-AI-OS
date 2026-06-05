//! Output distillation engine for MCP tool responses.
//!
//! Reduces agent context consumption by compressing large tool outputs before
//! they reach the calling model. Compression is transparent: if the result
//! would be larger than the original the original is returned unchanged.
//!
//! # Integration
//!
//! Call [`apply`] after `handle_tools_call` resolves:
//! ```ignore
//! let mut response = self.dispatch_tool(name, args)?;
//! apply(name, &mut response);
//! ```

pub mod diff;
pub mod metrics;

use serde_json::Value;

/// Minimum byte length below which distillation is skipped (already compact).
const MIN_LEN_THRESHOLD: usize = 512;

/// Generic per-tool line cap for outputs with no dedicated distiller.
const GENERIC_LINE_CAP: usize = 300;

/// Apply distillation to the `content[0].text` field of an MCP tool response.
///
/// Modifies `response` in place. No-ops when:
/// - the text field is absent or below `MIN_LEN_THRESHOLD`
/// - the compressed output is not smaller than the original
pub fn apply(tool: &str, response: &mut Value) {
    let original = match response["content"][0]["text"].as_str() {
        Some(t) if t.len() >= MIN_LEN_THRESHOLD => t.to_string(),
        _ => return,
    };

    let compressed = distil(tool, &original);
    if compressed.len() >= original.len() {
        return;
    }

    let savings = metrics::savings_pct(&original, &compressed);
    let annotated = if savings >= 10.0 {
        format!("{compressed}\n[~{savings:.0}% context saved]")
    } else {
        compressed
    };

    response["content"][0]["text"] = Value::String(annotated);
}

/// Route to the appropriate distiller for the given tool name.
fn distil(tool: &str, text: &str) -> String {
    match tool {
        "git_diff" => diff::distil(text),
        _ => generic_cap(text),
    }
}

/// Fallback: cap any long output to `GENERIC_LINE_CAP` lines.
fn generic_cap(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= GENERIC_LINE_CAP {
        return text.to_string();
    }
    let mut out = lines[..GENERIC_LINE_CAP].join("\n");
    out.push_str(&format!(
        "\n  … {} more lines omitted",
        lines.len() - GENERIC_LINE_CAP
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_response(text: &str) -> Value {
        json!({ "content": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn short_text_is_not_modified() {
        let short = "3 files changed  +5  -2";
        let mut resp = make_response(short);
        apply("git_diff", &mut resp);
        assert_eq!(resp["content"][0]["text"].as_str().unwrap(), short);
    }

    #[test]
    fn large_generic_output_is_capped() {
        let text = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let mut resp = make_response(&text);
        apply("some_tool", &mut resp);
        let result = resp["content"][0]["text"].as_str().unwrap();
        let lines = result.lines().count();
        assert!(lines < 400, "should be capped, got {lines} lines");
        assert!(result.contains("omitted"), "should mention omission");
    }

    #[test]
    fn missing_content_field_is_noop() {
        let mut resp = json!({ "ok": true });
        apply("git_diff", &mut resp);
        assert_eq!(resp, json!({ "ok": true }));
    }
}
