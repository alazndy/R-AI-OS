//! Terminal and agent sessions owned by the workspace.
//!
//! Sessions are app-owned PTY children. Only an allowlisted [`SessionKind`] can be started,
//! never command text. The PTY engine itself sits behind [`TerminalHost`] so hosting can move
//! to the daemon later.

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::{AppError, ErrorCode, Result};
use crate::project::{ProjectId, ProjectRoot};

/// Maximum number of concurrent sessions, in the UI and in the backend.
pub const MAX_SESSIONS: usize = 12;

/// What a session runs. Deserialized from an enum, never from free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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

    /// The agent name `raios resume` uses in its catalog; `None` when the agent has no verified
    /// native continuation (a plain shell, or Antigravity).
    pub fn native_agent(self) -> Option<&'static str> {
        match self {
            Self::Shell | Self::Agy => None,
            Self::Claude => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Opencode => Some("opencode"),
        }
    }

    /// The reverse of [`native_agent`](Self::native_agent).
    pub fn from_native_agent(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            _ => None,
        }
    }
}

/// Longest native conversation id a [`ResumeKey`] accepts, in bytes.
const MAX_RESUME_ID: usize = 128;

/// A saved conversation key, in the `agent:native-id` shape `raios resume --list` prints.
///
/// A validated key is the only thing a resume pane can be started with. It reaches the child
/// as one argument, never through a shell, and the allowlist here keeps it one plain token:
/// ids start with an alphanumeric (or `_`) and continue with alphanumerics, `-` and `_`, so a
/// `-`-leading, empty, oversized or punctuated value never reaches the command line. Only
/// agents with a verified native continuation can parse, so Antigravity and shells are
/// refused with the rest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResumeKey {
    agent: SessionKind,
    native_id: String,
}

impl ResumeKey {
    /// Validates one full `agent:native-id` key from the recovery catalog.
    pub fn parse(key: &str) -> Option<Self> {
        let (agent, native_id) = key.split_once(':')?;
        let agent = SessionKind::from_native_agent(agent)?;
        let valid = !native_id.is_empty()
            && native_id.len() <= MAX_RESUME_ID
            && native_id.starts_with(|start: char| start.is_ascii_alphanumeric() || start == '_')
            && native_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        valid.then(|| Self {
            agent,
            native_id: native_id.to_owned(),
        })
    }

    /// The agent the conversation belongs to.
    pub fn agent(&self) -> SessionKind {
        self.agent
    }

    /// The agent's own conversation id, without the agent prefix.
    pub fn native_id(&self) -> &str {
        &self.native_id
    }
}

impl std::fmt::Display for ResumeKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The private constructor only accepts native agents, whose name is always there.
        write!(
            formatter,
            "{}:{}",
            self.agent
                .native_agent()
                .expect("resume keys only carry native agents"),
            self.native_id
        )
    }
}

/// How one pane's child starts. Either way the command is assembled from the allowlist in
/// `terminal::command`; nothing here is command text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLaunch {
    /// A fresh run: `raios run <agent> --project <root> [prompt]`, or the user's shell.
    Fresh {
        /// Optional first prompt for an agent run.
        prompt: Option<String>,
    },
    /// A saved conversation continued in place: `raios resume <key>`.
    Resume(ResumeKey),
}

impl Default for SessionLaunch {
    fn default() -> Self {
        Self::Fresh { prompt: None }
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
        launch: &SessionLaunch,
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
        launch: &SessionLaunch,
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
        let id = self.host.spawn(project, kind, launch, size)?;
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
    fn at_most_twelve_sessions_may_run_at_once() {
        assert_eq!(MAX_SESSIONS, 12);
    }

    #[test]
    fn native_agents_round_trip_and_cover_only_resumable_kinds() {
        for kind in [
            SessionKind::Claude,
            SessionKind::Codex,
            SessionKind::Opencode,
        ] {
            let name = kind.native_agent().expect("native agent");
            assert_eq!(SessionKind::from_native_agent(name), Some(kind));
        }
        assert_eq!(SessionKind::Shell.native_agent(), None);
        assert_eq!(SessionKind::Agy.native_agent(), None);
        assert_eq!(SessionKind::from_native_agent("agy"), None);
        assert_eq!(SessionKind::from_native_agent("antigravity"), None);
        assert_eq!(SessionKind::from_native_agent(""), None);
    }

    #[test]
    fn resume_keys_accept_only_native_agents_and_plain_ids() {
        let key = ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("key");
        assert_eq!(key.agent(), SessionKind::Claude);
        assert_eq!(key.native_id(), "3294c005-aee0-4e95-860f-5800e045b624");
        assert_eq!(
            key.to_string(),
            "claude:3294c005-aee0-4e95-860f-5800e045b624"
        );
        assert_eq!(
            ResumeKey::parse("opencode:ses_abc123")
                .expect("key")
                .native_id(),
            "ses_abc123"
        );
    }

    #[test]
    fn resume_keys_refuse_flags_other_agents_and_oversized_ids() {
        for bad in [
            "",
            "claude",
            ":abc",
            "claude:",
            "agy:1234",
            "shell:1234",
            "Claude:1234",
            "claude:-rf",
            "claude:--fork-session",
            "claude:a/b",
            "claude:a b",
            "claude:a:b",
            "claude:a\0b",
        ] {
            assert!(ResumeKey::parse(bad).is_none(), "{bad:?} must be refused");
        }
        let longest = format!("claude:{}", "a".repeat(128));
        assert!(ResumeKey::parse(&longest).is_some());
        let longer = format!("claude:{}", "a".repeat(129));
        assert!(ResumeKey::parse(&longer).is_none());
    }
}
