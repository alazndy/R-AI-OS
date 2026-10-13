//! The operations that change daemon state and therefore go through the consent gate: launching
//! an agent, queueing a handoff and registering a project. One value says what is about to be
//! sent, in words derived from the payload itself, and can run itself against a [`ControlPlane`].

use std::path::{Path, PathBuf};

use crate::control::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, QueuedHandoff,
};
use crate::error::Result;
use crate::handoff_input::handoff_operation_text;

/// A mutating command waiting for policy and consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guarded {
    /// Validate and audit starting an agent in the project.
    LaunchAgent(LaunchAgentRequest),
    /// Queue a `BLOCKER` handoff.
    CreateHandoff(CreateHandoffRequest),
    /// Register a project directory.
    RegisterProject {
        /// Directory to register.
        root: PathBuf,
        /// Retry key.
        key: IdempotencyKey,
    },
}

/// What a guarded operation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedOutcome {
    /// The daemon accepted the agent launch; the workspace now starts the process itself.
    AgentLaunchValidated,
    /// The daemon queued the handoff.
    HandoffQueued(QueuedHandoff),
    /// The project is registered (or already was).
    ProjectRegistered {
        /// True when nothing changed because it was registered before.
        already_registered: bool,
    },
}

impl Guarded {
    /// Snake-case command name the daemon's policy is keyed by.
    pub fn command_name(&self) -> &'static str {
        match self {
            Self::LaunchAgent(_) => "launch_agent",
            Self::CreateHandoff(_) => "create_handoff",
            Self::RegisterProject { .. } => "register_project",
        }
    }

    /// The project the operation targets.
    pub fn project_root(&self) -> &Path {
        match self {
            Self::LaunchAgent(request) => &request.project_root,
            Self::CreateHandoff(request) => &request.project_root,
            Self::RegisterProject { root, .. } => root,
        }
    }

    /// The sentence the consent dialog shows, derived from the exact payload.
    pub fn summary(&self) -> String {
        match self {
            Self::LaunchAgent(request) => format!(
                "{} ajanı {} projesinde başlatılacak{}.",
                request.agent.identity().unwrap_or("shell"),
                request.project_root.display(),
                match &request.prompt {
                    Some(prompt) => format!(" (ilk istem {} karakter)", prompt.chars().count()),
                    None => String::new(),
                }
            ),
            Self::CreateHandoff(request) => handoff_operation_text(request),
            Self::RegisterProject { root, .. } => {
                format!(
                    "{} dizini Raios'a proje olarak kaydedilecek.",
                    root.display()
                )
            }
        }
    }

    /// Sends the command. Blocking: call from a background executor, and only after policy
    /// and consent allowed it.
    pub fn execute(&self, plane: &dyn ControlPlane) -> Result<GuardedOutcome> {
        match self {
            Self::LaunchAgent(request) => {
                plane.launch_agent(request)?;
                Ok(GuardedOutcome::AgentLaunchValidated)
            }
            Self::CreateHandoff(request) => plane
                .create_handoff(request)
                .map(GuardedOutcome::HandoffQueued),
            Self::RegisterProject { root, key } => {
                let outcome = plane.register_project(root, key)?;
                Ok(GuardedOutcome::ProjectRegistered {
                    already_registered: outcome.already_registered,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::fake::{FakeControlPlane, RecordedCall};
    use crate::control::RegistrationOutcome;
    use crate::sessions::SessionKind;

    fn launch() -> Guarded {
        Guarded::LaunchAgent(LaunchAgentRequest {
            project_root: PathBuf::from("/work/app"),
            agent: SessionKind::Codex,
            prompt: Some("fix the build".to_owned()),
            idempotency_key: IdempotencyKey::new("k1"),
        })
    }

    fn handoff() -> Guarded {
        Guarded::CreateHandoff(CreateHandoffRequest {
            project_root: PathBuf::from("/work/app"),
            from: SessionKind::Claude,
            to: SessionKind::Codex,
            summary: "devam et".to_owned(),
            idempotency_key: IdempotencyKey::new("k2"),
        })
    }

    #[test]
    fn command_names_match_the_daemons_policy_keys() {
        assert_eq!(launch().command_name(), "launch_agent");
        assert_eq!(handoff().command_name(), "create_handoff");
        let register = Guarded::RegisterProject {
            root: PathBuf::from("/a"),
            key: IdempotencyKey::new("k"),
        };
        assert_eq!(register.command_name(), "register_project");
    }

    #[test]
    fn the_summary_comes_from_the_payload() {
        let text = launch().summary();

        assert!(
            text.contains("codex_kaira") && text.contains("/work/app"),
            "{text}"
        );
        assert!(text.contains("13 karakter"), "{text}");
        assert_eq!(launch().project_root(), Path::new("/work/app"));
    }

    #[test]
    fn executing_a_launch_sends_exactly_that_request() {
        let plane = FakeControlPlane::new();
        plane.push_launch(Ok(()));

        let outcome = launch().execute(&plane).unwrap();

        assert_eq!(outcome, GuardedOutcome::AgentLaunchValidated);
        assert!(
            matches!(&plane.calls()[..], [RecordedCall::LaunchAgent(request)]
            if request.agent == SessionKind::Codex && request.prompt.as_deref() == Some("fix the build"))
        );
    }

    #[test]
    fn executing_a_handoff_returns_the_queue_receipt() {
        let plane = FakeControlPlane::new();
        plane.push_handoff(Ok(QueuedHandoff {
            approval_id: "a-9".into(),
            project_path: PathBuf::from("/work/app"),
        }));

        let outcome = handoff().execute(&plane).unwrap();

        assert!(
            matches!(outcome, GuardedOutcome::HandoffQueued(queued) if queued.approval_id == "a-9")
        );
    }

    #[test]
    fn registering_reports_whether_it_was_already_known() {
        let plane = FakeControlPlane::new();
        plane.push_registration(Ok(RegistrationOutcome {
            project_path: PathBuf::from("/a"),
            already_registered: true,
        }));
        let register = Guarded::RegisterProject {
            root: PathBuf::from("/a"),
            key: IdempotencyKey::new("k"),
        };

        assert_eq!(
            register.execute(&plane).unwrap(),
            GuardedOutcome::ProjectRegistered {
                already_registered: true
            }
        );
    }

    #[test]
    fn a_daemon_failure_is_returned_not_swallowed() {
        let plane = FakeControlPlane::new();

        assert!(launch().execute(&plane).is_err());
    }
}
