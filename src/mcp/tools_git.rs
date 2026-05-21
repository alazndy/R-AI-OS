use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn tool_git_status(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let s = crate::core::git::status(&path);
        let text = format!("Branch: {}  {}\nAhead: {}  Behind: {}\nStaged: {}  Modified: {}  Untracked: {}",
            s.branch.as_deref().unwrap_or("(detached)"), if s.dirty { "dirty" } else { "clean" },
            s.ahead, s.behind, s.staged.len(), s.unstaged.len(), s.untracked.len());
        Ok(json!({ "content": [{ "type": "text", "text": text }], "data": s }))
    }

    pub(super) fn tool_git_log(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let count = args["count"].as_u64().unwrap_or(10) as usize;
        let entries = crate::core::git::log(&path, count);
        let text = entries.iter().map(|e| format!("{} {} ({} {})", e.short_hash, e.message, e.author, e.date)).collect::<Vec<_>>().join("\n");
        Ok(json!({ "content": [{ "type": "text", "text": text }], "data": entries }))
    }

    pub(super) fn tool_git_diff(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let staged = args["staged"].as_bool().unwrap_or(false);
        let d = crate::core::git::diff(&path, staged);
        let summary = format!("{} files changed  +{}  -{}", d.files_changed, d.insertions, d.deletions);
        let text = if d.diff_text.is_empty() { summary } else { format!("{}\n\n{}", summary, d.diff_text) };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "data": { "files_changed": d.files_changed, "insertions": d.insertions, "deletions": d.deletions } }))
    }

    pub(super) fn tool_git_commit(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let message = args["message"].as_str().ok_or("missing message")?;
        let push = args["push"].as_bool().unwrap_or(false);
        let commit_result = crate::core::git::commit(&path, message, true);
        if !commit_result.ok {
            return Ok(json!({ "content": [{ "type": "text", "text": format!("Commit failed: {}", commit_result.message) }], "ok": false }));
        }
        let mut text = format!("✓ Committed: {}", commit_result.message);
        if push {
            let push_result = crate::core::git::push(&path);
            if push_result.ok { text.push_str("\n✓ Pushed to origin"); }
            else { text.push_str(&format!("\n✗ Push failed: {}", push_result.message)); }
        }
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ok": true }))
    }

    pub(super) fn tool_session_note(&self, args: &Value) -> Result<Value, String> {
        let note = args["note"].as_str().ok_or("missing note")?;
        let note_truncated = &note[..note.len().min(500)];
        let store = crate::session::SessionStore::new(crate::session::SessionStore::default_path());
        let session_id = args["session_id"].as_str().map(|s| s.to_string())
            .or_else(|| store.current_open().map(|s| s.id));
        match session_id {
            Some(id) => {
                store.record_event(&id, "note", note_truncated);
                Ok(json!({ "content": [{ "type": "text", "text": format!("Note recorded to session {}", id) }], "recorded": true, "session_id": id }))
            }
            None => Err("no active session".to_string()),
        }
    }
}
