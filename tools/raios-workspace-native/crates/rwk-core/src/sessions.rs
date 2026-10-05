//! Terminal and agent sessions owned by the workspace.
//!
//! Sessions are app-owned PTY children. Only an allowlisted [`SessionKind`] can be started,
//! never command text. The PTY engine itself sits behind [`TerminalHost`] so hosting can move
//! to the daemon later.

use std::sync::Mutex;

use crate::error::Result;
use crate::project::{ProjectId, ProjectRoot};

/// Maximum number of concurrent sessions, in the UI and in the backend.
pub const MAX_SESSIONS: usize = 4;

/// What a session runs. Deserialized from an enum, never from free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionKind {
    /// The user's login shell.
    Shell,
    /// Claude Code through `raios run claude`.
    Claude,
    /// Codex CLI through `raios run codex`.
    Codex,
    /// OpenCode through `raios run opencode`.
    Opencode,
    /// Antigravity through `raios run agy`.
    Agy,
}

impl SessionKind {
    /// Argument for `raios run <agent>`; `None` for a plain shell.
    pub fn agent(self) -> Option<&'static str> {
        match self {
            Self::Shell => None,
            Self::Claude => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Opencode => Some("opencode"),
            Self::Agy => Some("agy"),
        }
    }

    /// Control-plane agent identity used in handoffs; `None` for a plain shell.
    pub fn identity(self) -> Option<&'static str> {
        match self {
            Self::Shell => None,
            Self::Claude => Some("claude_kaira"),
            Self::Codex => Some("codex_kaira"),
            Self::Opencode => Some("opencode_kaira"),
            Self::Agy => Some("antigravity_kaira"),
        }
    }
}

/// Opaque identifier of a running or finished session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Wraps a generated identifier.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the UI knows about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// Session identifier.
    pub id: SessionId,
    /// Project the session runs in.
    pub project_id: ProjectId,
    /// What the session runs.
    pub kind: SessionKind,
    /// False once the child has exited.
    pub running: bool,
}

/// Terminal size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

/// The PTY engine. Implemented by the terminal engine in P2; faked in tests.
pub trait TerminalHost: Send + Sync {
    /// Starts a child in `project`'s root and returns its session identifier.
    fn spawn(
        &self,
        project: &ProjectRoot,
        kind: SessionKind,
        prompt: Option<&str>,
        size: TerminalSize,
    ) -> Result<SessionId>;

    /// Sends user input bytes to one session.
    fn write(&self, id: &SessionId, data: &[u8]) -> Result<()>;

    /// Resizes one session's PTY.
    fn resize(&self, id: &SessionId, size: TerminalSize) -> Result<()>;

    /// Stops one session and its descendants.
    fn stop(&self, id: &SessionId) -> Result<()>;

    /// True while the session's child is alive.
    fn is_running(&self, id: &SessionId) -> bool;
}

/// Owns the set of sessions, enforces [`MAX_SESSIONS`], and stops everything on shutdown.
pub struct SessionRegistry {
    // Read by the P1b implementation.
    #[allow(dead_code)]
    host: Box<dyn TerminalHost>,
    #[allow(dead_code)]
    sessions: Mutex<Vec<SessionInfo>>,
}

impl SessionRegistry {
    /// Creates an empty registry on top of a terminal host.
    pub fn new(host: Box<dyn TerminalHost>) -> Self {
        Self {
            host,
            sessions: Mutex::new(Vec::new()),
        }
    }

    /// Starts a session, refusing when [`MAX_SESSIONS`] are already running or after shutdown.
    pub fn start(
        &self,
        _project: &ProjectRoot,
        _kind: SessionKind,
        _prompt: Option<&str>,
        _size: TerminalSize,
    ) -> Result<SessionInfo> {
        unimplemented!("P1b")
    }

    /// Current sessions in start order.
    pub fn list(&self) -> Vec<SessionInfo> {
        unimplemented!("P1b")
    }

    /// Stops one app-owned session.
    pub fn stop(&self, _id: &SessionId) -> Result<()> {
        unimplemented!("P1b")
    }

    /// Stops every session and refuses new ones. Called when the window closes.
    pub fn stop_all(&self) {
        unimplemented!("P1b")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_has_no_agent_or_identity() {
        assert_eq!(SessionKind::Shell.agent(), None);
        assert_eq!(SessionKind::Shell.identity(), None);
    }

    #[test]
    fn agent_identities_match_the_daemon_handoff_allowlist() {
        // Mirrors `HANDOFF_IDENTITIES` in raios-runtime's control_plane/handoff.rs.
        let daemon_allowlist = [
            "claude_kaira",
            "codex_kaira",
            "opencode_kaira",
            "antigravity_kaira",
        ];
        let kinds = [
            SessionKind::Claude,
            SessionKind::Codex,
            SessionKind::Opencode,
            SessionKind::Agy,
        ];
        let identities: Vec<&str> = kinds.iter().filter_map(|k| k.identity()).collect();
        assert_eq!(identities, daemon_allowlist);
    }

    #[test]
    fn agent_arguments_match_raios_run() {
        let arguments: Vec<Option<&str>> = [
            SessionKind::Claude,
            SessionKind::Codex,
            SessionKind::Opencode,
            SessionKind::Agy,
        ]
        .iter()
        .map(|k| k.agent())
        .collect();
        assert_eq!(
            arguments,
            [Some("claude"), Some("codex"), Some("opencode"), Some("agy")]
        );
    }

    #[test]
    fn at_most_four_sessions_may_run_at_once() {
        assert_eq!(MAX_SESSIONS, 4);
    }
}
