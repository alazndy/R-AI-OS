//! Input decisions of the terminal view that do not need a window.

use rwk_core::error::ErrorCode;
use rwk_core::terminal::{KeyChord, MAX_PASTE_BYTES};

/// What to do with text the user wants to paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteDecision {
    /// A single line: send it now.
    Send,
    /// Several lines: show a preview and wait for the user (spec section 6).
    Confirm {
        /// Number of lines the paste contains.
        lines: usize,
    },
    /// Larger than the 16 KiB cap: refuse with a visible message.
    TooLarge,
}

/// Classifies a paste.
pub fn decide_paste(text: &str) -> PasteDecision {
    if text.len() > MAX_PASTE_BYTES {
        return PasteDecision::TooLarge;
    }
    let lines = text.lines().count();
    if lines > 1 {
        PasteDecision::Confirm { lines }
    } else {
        PasteDecision::Send
    }
}

/// The message for a failed input send; the error code says which of the three reasons applies.
pub fn input_failure_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Conflict => "The program is not reading its input.",
        ErrorCode::NotFound => "This terminal session has ended.",
        _ => "The input was refused.",
    }
}

/// Builds the chord for a key event.
pub fn chord_from_keystroke(
    key: &str,
    text: Option<&str>,
    control: bool,
    alt: bool,
    shift: bool,
) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        text: text.map(str::to_owned),
        control,
        alt,
        shift,
    }
}

/// Chords the workbench keeps for itself. Plain Ctrl+C/V and every unmodified key go to the
/// program (spec section 7).
pub fn is_reserved(key: &str, control: bool, shift: bool) -> bool {
    control && shift && matches!(key, "c" | "v")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multiline_paste_waits_for_confirmation() {
        assert_eq!(decide_paste("one line"), PasteDecision::Send);
        assert_eq!(
            decide_paste("two\nlines"),
            PasteDecision::Confirm { lines: 2 }
        );
    }

    #[test]
    fn an_oversized_paste_is_refused() {
        let big = "x".repeat(MAX_PASTE_BYTES + 1);
        assert_eq!(decide_paste(&big), PasteDecision::TooLarge);
    }

    #[test]
    fn a_key_event_maps_to_a_chord() {
        let chord = chord_from_keystroke("c", Some("c"), true, false, false);
        assert_eq!(chord.key, "c");
        assert!(chord.control && !chord.alt && !chord.shift);
    }

    #[test]
    fn each_input_failure_has_its_own_message() {
        assert!(input_failure_message(ErrorCode::Conflict).contains("not reading"));
        assert!(input_failure_message(ErrorCode::NotFound).contains("ended"));
        assert!(input_failure_message(ErrorCode::InvalidInput).contains("refused"));
    }

    #[test]
    fn reserved_workbench_chords_are_not_forwarded() {
        assert!(is_reserved("c", true, true));
        assert!(is_reserved("v", true, true));
        assert!(!is_reserved("c", true, false));
    }
}
