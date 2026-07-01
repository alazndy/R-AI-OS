//! Token estimation helpers.

/// Approximate token count using the ~4 chars/token heuristic.
pub fn estimate_tokens(text: &str) -> usize {
    ((text.len() as f64) / 4.0).ceil() as usize
}

/// Percentage of tokens saved: `(orig - compressed) / orig * 100`.
pub fn savings_pct(original: &str, compressed: &str) -> f64 {
    let orig = estimate_tokens(original);
    let comp = estimate_tokens(compressed);
    if orig == 0 {
        return 0.0;
    }
    (orig.saturating_sub(comp) as f64 / orig as f64) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn four_chars_is_one_token() {
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn savings_halved_text() {
        let orig = "a".repeat(400);
        let comp = "a".repeat(200);
        let pct = savings_pct(&orig, &comp);
        assert!((pct - 50.0).abs() < 1.0, "expected ~50%, got {:.1}%", pct);
    }

    #[test]
    fn savings_zero_when_equal() {
        assert_eq!(savings_pct("hello", "hello"), 0.0);
    }
}
