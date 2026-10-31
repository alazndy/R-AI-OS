//! Injects/strips the RAIOS session block in `~/.claude/CLAUDE.md`.

use super::*;

/// Inject a RAIOS session block into ~/.claude/CLAUDE.md so the session ID
/// and tracking info appear inside Claude Code's own UI (visible via /status
/// and referenced in the system context). Strips any stale block first.
pub(super) fn inject_session_to_claude_md(run_id: &str, agent_identity: &str, started_at: &str) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let stripped = strip_session_block(&content);
    let block = format!(
        "\n<!-- raios-session-begin -->\n\
# RAIOS WRAPPER SESSION\n\
- Session ID: `{}`\n\
- Agent: {}\n\
- Started: {}\n\
- Tracking: `raios sessions` | `raios sessions --agent claude`\n\
<!-- raios-session-end -->\n",
        &run_id[..8],
        agent_identity,
        started_at,
    );
    let _ = std::fs::write(&path, format!("{}{}", stripped.trim_end(), block));
}

/// Remove the RAIOS session block from ~/.claude/CLAUDE.md.
pub(super) fn strip_session_from_claude_md() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let stripped = strip_session_block(&content);
    if stripped != content {
        let _ = std::fs::write(&path, stripped.trim_end().to_string() + "\n");
    }
}

pub(super) fn strip_session_block(content: &str) -> String {
    const BEGIN: &str = "<!-- raios-session-begin -->";
    const END: &str = "<!-- raios-session-end -->";
    if let (Some(start), Some(end_off)) = (content.find(BEGIN), content.rfind(END)) {
        let before = content[..start].trim_end_matches('\n');
        let after = &content[end_off + END.len()..];
        format!("{}\n{}", before, after.trim_start_matches('\n'))
    } else {
        content.to_string()
    }
}
