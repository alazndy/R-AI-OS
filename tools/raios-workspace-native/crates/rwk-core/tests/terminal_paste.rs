//! How pasted text is encoded for a terminal.

use rwk_core::error::ErrorCode;
use rwk_core::terminal::{encode_paste, MAX_PASTE_BYTES};

#[test]
fn a_paste_without_bracketed_mode_turns_line_breaks_into_carriage_returns() {
    assert_eq!(encode_paste("a\nb\r\nc", false).unwrap(), b"a\rb\rc");
    assert_eq!(encode_paste("", false).unwrap(), b"");
}

#[test]
fn a_bracketed_paste_is_wrapped_and_keeps_its_line_breaks() {
    assert_eq!(
        encode_paste("a\nb", true).unwrap(),
        b"\x1b[200~a\nb\x1b[201~"
    );
}

#[test]
fn escape_and_nul_characters_never_survive_a_paste() {
    // A pasted end-of-paste marker would let the rest of the text run as typed input.
    for bracketed in [true, false] {
        let bytes = encode_paste("safe\x1b[201~rm -rf /\0", bracketed).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let inner = text
            .strip_prefix("\x1b[200~")
            .and_then(|rest| rest.strip_suffix("\x1b[201~"))
            .unwrap_or(&text);
        assert_eq!(inner, "safe[201~rm -rf /", "bracketed={bracketed}");
    }
}

#[test]
fn the_paste_limit_counts_bytes_not_characters() {
    assert!(encode_paste(&"x".repeat(MAX_PASTE_BYTES), true).is_ok());
    let over = encode_paste(&"x".repeat(MAX_PASTE_BYTES + 1), true).unwrap_err();
    assert_eq!(over.code, ErrorCode::InvalidInput);
    // 6,000 three-byte characters are 18,000 bytes.
    assert!(encode_paste(&"€".repeat(6_000), false).is_err());
}
