//! Injects/strips the RAIOS session block in `~/.claude/CLAUDE.md`.

use super::*;

/// Register this wrapper in the RAIOS session block of ~/.claude/CLAUDE.md so the session ID
/// and tracking info appear inside Claude Code's own UI (visible via /status and referenced in
/// the system context). The file holds ONE block that lists every live wrapper, so concurrent
/// runs do not overwrite each other. Entries of wrappers that no longer exist are dropped.
pub(super) fn inject_session_to_claude_md(run_id: &str, agent_identity: &str, started_at: &str) {
    let me = std::process::id();
    let entry = SessionEntry {
        id: run_id[..8].to_string(),
        agent: agent_identity.to_string(),
        started: started_at.to_string(),
        pid: Some(me),
    };
    update_session_block(true, |mut entries| {
        // Drop our own old entry, wrappers that are gone, and legacy entries nobody can vouch for.
        entries.retain(|e| e.pid.is_some_and(|pid| pid != me && process_exists(pid)));
        entries.push(entry);
        entries
    });
}

/// Remove this wrapper from the session block (the block goes away with its last entry). Other
/// wrappers' entries stay, so their sessions remain visible until they exit themselves. A legacy
/// entry without a PID is removed too.
pub(super) fn strip_session_from_claude_md() {
    let me = std::process::id();
    update_session_block(false, |mut entries| {
        entries.retain(|e| e.pid.is_some_and(|pid| pid != me));
        entries
    });
}

/// Remove entries of wrappers that no longer exist (killed with `kill -9`, power loss). Called
/// at the start of every run, so a stale entry never outlives the next wrapper. Entries without
/// a PID cannot be proven stale and stay.
pub(super) fn sweep_stale_session_block() {
    update_session_block(false, |mut entries| {
        entries.retain(|e| e.pid.is_none_or(process_exists));
        entries
    });
}

const SESSION_BEGIN: &str = "<!-- raios-session-begin -->";
const SESSION_END: &str = "<!-- raios-session-end -->";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionEntry {
    id: String,
    agent: String,
    started: String,
    pid: Option<u32>,
}

impl SessionEntry {
    fn render(&self) -> String {
        let pid = self
            .pid
            .map(|pid| format!(" · Wrapper PID: {pid}"))
            .unwrap_or_default();
        format!(
            "- Session `{}` · Agent: {} · Started: {}{pid}\n",
            self.id, self.agent, self.started
        )
    }

    /// One line of the current format: ``- Session `id` · Agent: a · Started: t · Wrapper PID: n``.
    fn parse(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("- Session `")?;
        let (id, rest) = rest.split_once('`')?;
        let mut entry = SessionEntry {
            id: id.to_string(),
            agent: String::new(),
            started: String::new(),
            pid: None,
        };
        for part in rest.split(" · ") {
            if let Some(agent) = part.strip_prefix("Agent: ") {
                entry.agent = agent.to_string();
            } else if let Some(started) = part.strip_prefix("Started: ") {
                entry.started = started.to_string();
            } else if let Some(pid) = part.strip_prefix("Wrapper PID: ") {
                entry.pid = pid.trim().parse().ok();
            }
        }
        Some(entry)
    }
}

/// The entries of the session block in `content`. Reads the legacy single-session layout
/// (`- Session ID:`, `- Agent:`, `- Started:`, `- Wrapper PID:` on separate lines) as one entry.
fn session_entries(content: &str) -> Vec<SessionEntry> {
    let Some(start) = content.find(SESSION_BEGIN) else {
        return Vec::new();
    };
    let end = content.rfind(SESSION_END).unwrap_or(content.len());
    let block = content.get(start..end).unwrap_or_default();
    let mut entries: Vec<SessionEntry> = block.lines().filter_map(SessionEntry::parse).collect();
    if entries.is_empty() {
        let field = |name: &str| {
            block
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .map(|value| value.trim().trim_matches('`').to_string())
        };
        if let Some(id) = field("- Session ID: ") {
            entries.push(SessionEntry {
                id,
                agent: field("- Agent: ").unwrap_or_default(),
                started: field("- Started: ").unwrap_or_default(),
                pid: field("- Wrapper PID: ").and_then(|pid| pid.parse().ok()),
            });
        }
    }
    entries
}

fn render_session_block(entries: &[SessionEntry]) -> String {
    let mut block = format!(
        "\n{SESSION_BEGIN}\n# RAIOS WRAPPER SESSIONS\n\
         - Tracking: `raios sessions` | `raios sessions --agent claude`\n"
    );
    for entry in entries {
        block.push_str(&entry.render());
    }
    block.push_str(SESSION_END);
    block.push('\n');
    block
}

/// Read ~/.claude/CLAUDE.md, apply `change` to the session entries and write the file back with
/// the block re-rendered (or removed when no entry is left). Nothing is written when nothing
/// changed. `create` makes a block appear in a file that has none yet.
fn update_session_block(create: bool, change: impl FnOnce(Vec<SessionEntry>) -> Vec<SessionEntry>) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    // Held until the function returns, so two wrappers cannot interleave read and write.
    let _lock = SessionFileLock::acquire(&path.with_file_name(".raios-session.lock"));
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let has_block = content.contains(SESSION_BEGIN);
    if !has_block && !create {
        return;
    }
    let old = session_entries(&content);
    let new = change(old.clone());
    if has_block && new == old && content.contains("# RAIOS WRAPPER SESSIONS") {
        return;
    }
    let stripped = strip_session_block(&content);
    let updated = if new.is_empty() {
        stripped.trim_end().to_string() + "\n"
    } else {
        format!("{}{}", stripped.trim_end(), render_session_block(&new))
    };
    if updated == content {
        return;
    }
    if let Err(error) = std::fs::write(&path, updated) {
        eprintln!(
            "raios: could not update the session block in {}: {error}",
            path.display()
        );
    }
}

/// An exclusive advisory lock on a sidecar file (never on CLAUDE.md itself, which Claude Code
/// and editors replace). Best effort: when the lock cannot be taken the update still runs, as it
/// did before locking existed. Released when dropped (closing the file drops the `flock`).
struct SessionFileLock {
    _file: Option<std::fs::File>,
}

impl SessionFileLock {
    fn acquire(path: &Path) -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .ok();
        #[cfg(unix)]
        if let Some(file) = &file {
            use std::os::fd::AsRawFd;
            // SAFETY: the fd is valid for the lifetime of `file`; LOCK_EX blocks until granted.
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        }
        SessionFileLock { _file: file }
    }
}

/// The wrapper PIDs recorded in the session block (legacy blocks without one yield none).
#[cfg(test)]
pub(super) fn session_block_pids(content: &str) -> Vec<u32> {
    session_entries(content)
        .into_iter()
        .filter_map(|entry| entry.pid)
        .collect()
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
    const END: &str = SESSION_END;
    if let (Some(start), Some(end_off)) = (content.find(BEGIN), content.rfind(END)) {
        let before = content[..start].trim_end_matches('\n');
        let after = &content[end_off + END.len()..];
        format!("{}\n{}", before, after.trim_start_matches('\n'))
    } else {
        content.to_string()
    }
}
