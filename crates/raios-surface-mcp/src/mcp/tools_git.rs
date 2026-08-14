use serde_json::{json, Value};

use super::McpServer;

const MAX_NOTE_BYTES: usize = 500;

/// Truncates `note` to at most `max_bytes` bytes, snapping down to the
/// nearest UTF-8 char boundary. Plain byte-slicing (`&note[..max_bytes]`)
/// panics whenever `max_bytes` lands inside a multi-byte character — a real
/// risk here since `note` is caller-supplied MCP tool input of arbitrary
/// content and length.
fn truncate_note_bytes(note: &str, max_bytes: usize) -> &str {
    if note.len() <= max_bytes {
        return note;
    }
    let mut end = max_bytes;
    while end > 0 && !note.is_char_boundary(end) {
        end -= 1;
    }
    &note[..end]
}

impl McpServer {
    pub(super) fn tool_git_status(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let s = raios_core::core::git::status(&path);
        let text = format!(
            "Branch: {}  {}\nAhead: {}  Behind: {}\nStaged: {}  Modified: {}  Untracked: {}",
            s.branch.as_deref().unwrap_or("(detached)"),
            if s.dirty { "dirty" } else { "clean" },
            s.ahead,
            s.behind,
            s.staged.len(),
            s.unstaged.len(),
            s.untracked.len()
        );
        Ok(json!({ "content": [{ "type": "text", "text": text }], "data": s }))
    }

    pub(super) fn tool_git_log(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let count = args["count"].as_u64().unwrap_or(10) as usize;
        let entries = raios_core::core::git::log(&path, count);
        let text = entries
            .iter()
            .map(|e| format!("{} {} ({} {})", e.short_hash, e.message, e.author, e.date))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(json!({ "content": [{ "type": "text", "text": text }], "data": entries }))
    }

    pub(super) fn tool_git_diff(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let staged = args["staged"].as_bool().unwrap_or(false);
        let d = raios_core::core::git::diff(&path, staged);
        let summary = format!(
            "{} files changed  +{}  -{}",
            d.files_changed, d.insertions, d.deletions
        );
        let text = if d.diff_text.is_empty() {
            summary
        } else {
            format!("{}\n\n{}", summary, d.diff_text)
        };
        Ok(
            json!({ "content": [{ "type": "text", "text": text }], "data": { "files_changed": d.files_changed, "insertions": d.insertions, "deletions": d.deletions } }),
        )
    }

    pub(super) fn tool_git_commit(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let message = args["message"].as_str().ok_or("missing message")?;
        let push = args["push"].as_bool().unwrap_or(false);
        let commit_result = raios_core::core::git::commit(&path, message, true);
        if !commit_result.ok {
            return Ok(
                json!({ "content": [{ "type": "text", "text": format!("Commit failed: {}", commit_result.message) }], "ok": false }),
            );
        }
        let mut text = format!("✓ Committed: {}", commit_result.message);
        if push {
            let push_result = raios_core::core::git::push(&path);
            if push_result.ok {
                text.push_str("\n✓ Pushed to origin");
            } else {
                text.push_str(&format!("\n✗ Push failed: {}", push_result.message));
            }
        }
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ok": true }))
    }

    pub(super) fn tool_session_note(&self, args: &Value) -> Result<Value, String> {
        let note = args["note"].as_str().ok_or("missing note")?;
        let note_truncated = truncate_note_bytes(note, MAX_NOTE_BYTES);
        let store = raios_runtime::session::SessionStore::new(
            raios_runtime::session::SessionStore::default_path(),
        );
        let session_id = args["session_id"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| store.current_open().map(|s| s.id));
        match session_id {
            Some(id) => {
                store.record_event(&id, "note", note_truncated);
                Ok(
                    json!({ "content": [{ "type": "text", "text": format!("Note recorded to session {}", id) }], "recorded": true, "session_id": id }),
                )
            }
            None => Err("no active session".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_note_bytes;

    #[test]
    fn leaves_notes_shorter_than_the_limit_untouched() {
        assert_eq!(truncate_note_bytes("short note", 500), "short note");
    }

    #[test]
    fn leaves_notes_exactly_at_the_limit_untouched() {
        let note = "a".repeat(500);
        assert_eq!(truncate_note_bytes(&note, 500), note);
    }

    #[test]
    fn truncates_long_ascii_notes_to_the_byte_limit() {
        let note = "a".repeat(600);
        let truncated = truncate_note_bytes(&note, 500);
        assert_eq!(truncated.len(), 500);
    }

    /// Regression test: the previous implementation byte-sliced with
    /// `&note[..note.len().min(500)]`, which panics with "byte index 500 is
    /// not a char boundary" whenever a multi-byte UTF-8 character straddles
    /// the cut point — a real risk since `note` is arbitrary MCP caller input.
    #[test]
    fn does_not_panic_on_multi_byte_utf8_at_the_cut_boundary() {
        // Each '→' is 3 bytes (U+2192); 166 of them = 498 bytes, then two
        // ASCII chars land the 500-byte cut exactly inside the 167th arrow.
        let note = format!("{}rest of the note", "→".repeat(167));
        let truncated = truncate_note_bytes(&note, 500);
        assert!(truncated.len() <= 500);
        assert!(note.starts_with(truncated));
    }

    #[test]
    fn handles_notes_entirely_made_of_multi_byte_chars() {
        let note = "→".repeat(300); // 900 bytes
        let truncated = truncate_note_bytes(&note, 500);
        assert!(truncated.len() <= 500);
        assert!(note.starts_with(truncated));
    }

    #[test]
    fn empty_note_stays_empty() {
        assert_eq!(truncate_note_bytes("", 500), "");
    }
}
