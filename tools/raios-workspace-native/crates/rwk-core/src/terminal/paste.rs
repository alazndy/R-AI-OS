//! Encoding of pasted text for a terminal.

use crate::error::{AppError, ErrorCode, Result};

/// Largest paste accepted, in bytes.
pub const MAX_PASTE_BYTES: usize = 16 * 1024;

const BRACKET_START: &[u8] = b"\x1b[200~";
const BRACKET_END: &[u8] = b"\x1b[201~";

/// Turns pasted text into the bytes to write to the PTY.
///
/// Escape and NUL characters are dropped first. Without that, pasted text could contain the
/// end-of-paste marker `ESC [ 201 ~` and make the shell run what follows it as typed commands.
/// With bracketed paste on, the text is wrapped in the start and end markers and keeps its line
/// breaks. Otherwise line breaks become carriage returns, which is what the Enter key sends.
pub fn encode_paste(text: &str, bracketed: bool) -> Result<Vec<u8>> {
    if text.len() > MAX_PASTE_BYTES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("A paste is limited to {MAX_PASTE_BYTES} bytes."),
        ));
    }
    let cleaned: String = text
        .chars()
        .filter(|c| *c != '\x1b' && *c != '\0')
        .collect();
    if bracketed {
        let mut bytes = Vec::with_capacity(cleaned.len() + BRACKET_START.len() + BRACKET_END.len());
        bytes.extend_from_slice(BRACKET_START);
        bytes.extend_from_slice(cleaned.as_bytes());
        bytes.extend_from_slice(BRACKET_END);
        return Ok(bytes);
    }
    Ok(cleaned
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .into_bytes())
}
