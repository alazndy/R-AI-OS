//! Small text helpers shared across the workspace.

/// Longest prefix of `s` that is at most `max_bytes` long and ends on a UTF-8
/// character boundary.
///
/// Plain byte slicing (`&s[..max_bytes]`) panics whenever `max_bytes` lands
/// inside a multi-byte character. That is routine for non-ASCII text — Turkish
/// `ş ı ğ ü ö ç`, accents, CJK, emoji — and has crashed this codebase before.
/// Use this wherever text of unknown content is cut to a byte budget.
pub fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_returned_whole() {
        assert_eq!(truncate_at_char_boundary("merhaba", 100), "merhaba");
        assert_eq!(truncate_at_char_boundary("merhaba", 7), "merhaba");
    }

    #[test]
    fn ascii_is_cut_exactly_at_the_limit() {
        assert_eq!(truncate_at_char_boundary("abcdef", 3), "abc");
    }

    #[test]
    fn a_cut_inside_a_multibyte_char_backs_up_to_the_previous_boundary() {
        // "ş" is two bytes (0xC5 0x9F): a limit of 1 lands inside it.
        assert_eq!(truncate_at_char_boundary("şa", 1), "");
        assert_eq!(truncate_at_char_boundary("aşb", 2), "a");
        assert_eq!(truncate_at_char_boundary("aşb", 3), "aş");
        // 4-byte emoji.
        assert_eq!(truncate_at_char_boundary("a😀", 4), "a");
        assert_eq!(truncate_at_char_boundary("a😀", 5), "a😀");
    }

    #[test]
    fn zero_budget_yields_empty() {
        assert_eq!(truncate_at_char_boundary("anything", 0), "");
        assert_eq!(truncate_at_char_boundary("", 0), "");
    }

    #[test]
    fn result_is_always_valid_and_within_budget() {
        let text = "Türkçe: şığüöç 😀 日本語 — end";
        for max in 0..=text.len() + 2 {
            let cut = truncate_at_char_boundary(text, max);
            assert!(cut.len() <= max, "max={max} cut={cut:?}");
            assert!(text.starts_with(cut));
        }
    }
}
