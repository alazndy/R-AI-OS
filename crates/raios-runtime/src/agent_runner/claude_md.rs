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
- Wrapper PID: {}\n\
- Tracking: `raios sessions` | `raios sessions --agent claude`\n\
<!-- raios-session-end -->\n",
        &run_id[..8],
        agent_identity,
        started_at,
        std::process::id(),
    );
    if let Err(error) = std::fs::write(&path, format!("{}{}", stripped.trim_end(), block)) {
        eprintln!(
            "raios: could not write the session block to {}: {error}",
            path.display()
        );
    }
}

/// Remove the RAIOS session block from ~/.claude/CLAUDE.md - but only this process's own. When
/// another wrapper has injected its block since (the file holds one block, last writer wins),
/// leaving it alone keeps that wrapper's session visible until it exits itself.
pub(super) fn strip_session_from_claude_md() {
    strip_session_block_where(|pid| pid.is_none_or(|pid| pid == std::process::id()));
}

/// Remove a block left behind by a wrapper that no longer exists (killed with `kill -9`, power
/// loss). Called at the start of every run, so a stale block never outlives the next wrapper.
pub(super) fn sweep_stale_session_block() {
    strip_session_block_where(|pid| pid.is_some_and(|pid| !process_exists(pid)));
}

fn strip_session_block_where(should_strip: impl Fn(Option<u32>) -> bool) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    if !content.contains(SESSION_BEGIN) || !should_strip(session_block_pid(&content)) {
        return;
    }
    let stripped = strip_session_block(&content);
    if stripped != content {
        if let Err(error) = std::fs::write(&path, stripped.trim_end().to_string() + "\n") {
            eprintln!(
                "raios: could not remove the session block from {}: {error}",
                path.display()
            );
        }
    }
}

const SESSION_BEGIN: &str = "<!-- raios-session-begin -->";

/// The wrapper PID recorded in the session block, if the block has one (older blocks do not).
pub(super) fn session_block_pid(content: &str) -> Option<u32> {
    let start = content.find(SESSION_BEGIN)?;
    content[start..]
        .lines()
        .find_map(|line| line.strip_prefix("- Wrapper PID: "))
        .and_then(|pid| pid.trim().parse().ok())
}

/// Whether a process with this PID exists. Conservative: anything that is not a clear "no such
/// process" counts as alive, and without a way to ask (non-unix) so does everything.
fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return true;
        };
        // SAFETY: signal 0 only checks that the process exists and can be signalled.
        let rc = unsafe { libc::kill(pid, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

pub(super) fn strip_session_block(content: &str) -> String {
    const BEGIN: &str = SESSION_BEGIN;
    const END: &str = "<!-- raios-session-end -->";
    if let (Some(start), Some(end_off)) = (content.find(BEGIN), content.rfind(END)) {
        let before = content[..start].trim_end_matches('\n');
        let after = &content[end_off + END.len()..];
        format!("{}\n{}", before, after.trim_start_matches('\n'))
    } else {
        content.to_string()
    }
}
