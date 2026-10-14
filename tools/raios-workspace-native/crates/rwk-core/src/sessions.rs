//! Terminal and agent sessions owned by the workspace.
//!
//! Sessions are app-owned PTY children. Only an allowlisted [`SessionKind`] can be started,
//! never command text. The PTY engine itself sits behind [`TerminalHost`] so hosting can move
//! to the daemon later.

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::{AppError, ErrorCode, Result};
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

#[derive(Debug, Default)]
struct RegistryState {
    sessions: Vec<SessionInfo>,
    closed: bool,
}

/// Owns the set of sessions, enforces [`MAX_SESSIONS`], and stops everything on shutdown.
///
/// The registry lock is held while the host spawns a child, so two callers racing for the last
/// slot cannot both succeed. A poisoned lock is recovered rather than propagated: the state is
/// a plain list that stays consistent after a panic elsewhere.
pub struct SessionRegistry {
    host: Box<dyn TerminalHost>,
    state: Mutex<RegistryState>,
}

impl SessionRegistry {
    /// Creates an empty registry on top of a terminal host.
    pub fn new(host: Box<dyn TerminalHost>) -> Self {
        Self {
            host,
            state: Mutex::new(RegistryState::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, RegistryState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Updates every session's `running` flag from the host; a stopped session never revives.
    fn refresh(&self, state: &mut RegistryState) {
        for session in &mut state.sessions {
            session.running = session.running && self.host.is_running(&session.id);
        }
    }

    /// Starts a session, refusing when [`MAX_SESSIONS`] are already running or after shutdown.
    /// Sessions that have exited are dropped from the registry before the limit is checked.
    pub fn start(
        &self,
        project: &ProjectRoot,
        kind: SessionKind,
        prompt: Option<&str>,
        size: TerminalSize,
    ) -> Result<SessionInfo> {
        let mut state = self.lock();
        if state.closed {
            return Err(AppError::new(
                ErrorCode::WorkspaceClosed,
                "Workspace is closing; no new session can start.",
            ));
        }
        self.refresh(&mut state);
        state.sessions.retain(|session| session.running);
        if state.sessions.len() >= MAX_SESSIONS {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!("At most {MAX_SESSIONS} terminal sessions can run at once."),
            ));
        }
        let id = self.host.spawn(project, kind, prompt, size)?;
        let info = SessionInfo {
            id,
            project_id: project.info().id.clone(),
            kind,
            running: true,
        };
        state.sessions.push(info.clone());
        Ok(info)
    }

    /// Current sessions in start order, including ones that have exited since the last start.
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut state = self.lock();
        self.refresh(&mut state);
        state.sessions.clone()
    }

    /// Stops one app-owned session.
    pub fn stop(&self, id: &SessionId) -> Result<()> {
        let mut state = self.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| &session.id == id)
            .ok_or_else(|| {
                AppError::new(ErrorCode::NotFound, "No owned session matches this ID.")
            })?;
        self.host.stop(id)?;
        session.running = false;
        Ok(())
    }

    /// Stops every session and refuses new ones. Called when the window closes. A session that
    /// fails to stop does not prevent the others from being stopped.
    pub fn stop_all(&self) {
        let mut state = self.lock();
        state.closed = true;
        for session in &mut state.sessions {
            let _ = self.host.stop(&session.id);
            session.running = false;
        }
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
