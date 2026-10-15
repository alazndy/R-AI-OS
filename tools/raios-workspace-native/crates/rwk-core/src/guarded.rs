//! The operations that change daemon state and therefore go through the consent gate: launching
//! an agent, queueing a handoff and registering a project. One value says what is about to be
//! sent, in words derived from the payload itself, and can run itself against a [`ControlPlane`].

use std::path::{Path, PathBuf};

use crate::control::{
    ControlPlane, CreateHandoffRequest, HandoffDecisionRequest, IdempotencyKey, LaunchAgentRequest,
    QueuedHandoff,
};
use crate::error::Result;
use crate::handoff_input::handoff_operation_text;
use crate::sessions::ResumeKey;

/// A mutating command waiting for policy and consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guarded {
    /// Validate and audit starting an agent in the project.
    LaunchAgent(LaunchAgentRequest),
    /// Continue a saved conversation. Policy and audit are the launch's: the daemon records the
    /// pane as an agent launch in the project all the same.
    ResumeAgent {
        /// Project root the conversation continues in.
        root: PathBuf,
        /// The saved conversation, validated.
        key: ResumeKey,
        /// Retry key.
        idempotency_key: IdempotencyKey,
    },
    /// Queue a `BLOCKER` handoff.
    CreateHandoff(CreateHandoffRequest),
    /// Approve a pending handoff approval: owner consent, not delivery.
    ApproveHandoff(HandoffDecisionRequest),
    /// Reject a pending handoff approval with a reason.
    RejectHandoff(HandoffDecisionRequest),
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
    /// The daemon recorded the owner's approval; delivery stays with the addressed agent.
    HandoffApproved,
    /// The daemon recorded the rejection.
    HandoffRejected,
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
            Self::ResumeAgent { .. } => "launch_agent",
            Self::CreateHandoff(_) => "create_handoff",
            Self::ApproveHandoff(_) => "approve_handoff",
            Self::RejectHandoff(_) => "reject_handoff",
            Self::RegisterProject { .. } => "register_project",
        }
    }

    /// The project the operation targets.
    pub fn project_root(&self) -> &Path {
        match self {
            Self::LaunchAgent(request) => &request.project_root,
            Self::ResumeAgent { root, .. } => root,
            Self::CreateHandoff(request) => &request.project_root,
            Self::ApproveHandoff(request) => &request.project_root,
            Self::RejectHandoff(request) => &request.project_root,
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
            Self::ResumeAgent { root, key, .. } => format!(
                "{} oturumu {} projesinde sürdürülecek ({}).",
                key.agent().identity().unwrap_or("ajan"),
                root.display(),
                key
            ),
            Self::ApproveHandoff(request) => format!(
                "{} numaralı devir onayı onaylanacak ({}). Onay teslim değildir: notu adreslenen ajan kendi çalıştırmasında teslim alacak.",
                request.approval_id,
                request.project_root.display()
            ),
            Self::RejectHandoff(request) => format!(
                "{} numaralı devir onayı reddedilecek ({}). Gerekçe: {}",
                request.approval_id,
                request.project_root.display(),
                request.reason.as_deref().unwrap_or("(yok)")
            ),
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
            Self::ResumeAgent {
                root,
                key,
                idempotency_key,
            } => {
                plane.launch_agent(&LaunchAgentRequest {
                    project_root: root.clone(),
                    agent: key.agent(),
                    prompt: None,
                    idempotency_key: idempotency_key.clone(),
                })?;
                Ok(GuardedOutcome::AgentLaunchValidated)
            }
            Self::CreateHandoff(request) => plane
                .create_handoff(request)
                .map(GuardedOutcome::HandoffQueued),
            Self::ApproveHandoff(request) => {
                plane.approve_handoff(request)?;
                Ok(GuardedOutcome::HandoffApproved)
            }
            Self::RejectHandoff(request) => {
                plane.reject_handoff(request)?;
                Ok(GuardedOutcome::HandoffRejected)
            }
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

    fn resume() -> Guarded {
        Guarded::ResumeAgent {
            root: PathBuf::from("/work/app"),
            key: ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("key"),
            idempotency_key: IdempotencyKey::new("r1"),
        }
    }

    #[test]
    fn a_resume_is_audited_as_a_launch_of_its_agent() {
        let plane = FakeControlPlane::new();
        plane.push_launch(Ok(()));

        let outcome = resume().execute(&plane).expect("executed");

        assert_eq!(outcome, GuardedOutcome::AgentLaunchValidated);
        assert!(
            matches!(
                &plane.calls()[..],
                [RecordedCall::LaunchAgent(request)]
                if request.agent == SessionKind::Claude
                    && request.prompt.is_none()
                    && request.project_root == Path::new("/work/app")
            ),
            "a resume reaches the daemon as the agent's launch"
        );
        let summary = resume().summary();
        assert!(
            summary.contains("claude_kaira") && summary.contains("sürdürülecek"),
            "{summary}"
        );
        assert_eq!(resume().project_root(), Path::new("/work/app"));
    }

    #[test]
    fn command_names_match_the_daemons_policy_keys() {
        assert_eq!(launch().command_name(), "launch_agent");
        assert_eq!(resume().command_name(), "launch_agent", "same policy key");
        assert_eq!(handoff().command_name(), "create_handoff");
        assert_eq!(
            Guarded::ApproveHandoff(decision(None)).command_name(),
            "approve_handoff"
        );
        assert_eq!(
            Guarded::RejectHandoff(decision(Some("kapsam dışı"))).command_name(),
            "reject_handoff"
        );
        let register = Guarded::RegisterProject {
            root: PathBuf::from("/a"),
            key: IdempotencyKey::new("k"),
        };
        assert_eq!(register.command_name(), "register_project");
    }

    fn decision(reason: Option<&str>) -> HandoffDecisionRequest {
        HandoffDecisionRequest {
            project_root: PathBuf::from("/work/app"),
            approval_id: "a-7".to_owned(),
            expected_revision: 4,
            reason: reason.map(str::to_owned),
            idempotency_key: IdempotencyKey::new("d-1"),
        }
    }

    #[test]
    fn the_approval_text_says_consent_is_not_delivery() {
        let text = Guarded::ApproveHandoff(decision(None)).summary();

        assert!(text.contains("a-7") && text.contains("/work/app"), "{text}");
        assert!(text.contains("teslim değildir"), "{text}");

        let rejection = Guarded::RejectHandoff(decision(Some("kapsam dışı"))).summary();
        assert!(
            rejection.contains("a-7") && rejection.contains("kapsam dışı"),
            "{rejection}"
        );
        assert_eq!(
            Guarded::ApproveHandoff(decision(None)).project_root(),
            Path::new("/work/app")
        );
    }

    #[test]
    fn approving_sends_the_revision_the_user_saw() {
        let plane = FakeControlPlane::new();
        plane.push_approval(Ok(()));

        let outcome = Guarded::ApproveHandoff(decision(None))
            .execute(&plane)
            .unwrap();

        assert_eq!(outcome, GuardedOutcome::HandoffApproved);
        assert!(
            matches!(&plane.calls()[..], [RecordedCall::ApproveHandoff(request)]
            if request.approval_id == "a-7" && request.expected_revision == 4 && request.reason.is_none())
        );
    }

    #[test]
    fn rejecting_sends_the_reason_and_the_revision() {
        let plane = FakeControlPlane::new();
        plane.push_rejection(Ok(()));

        let outcome = Guarded::RejectHandoff(decision(Some("kapsam dışı")))
            .execute(&plane)
            .unwrap();

        assert_eq!(outcome, GuardedOutcome::HandoffRejected);
        assert!(
            matches!(&plane.calls()[..], [RecordedCall::RejectHandoff(request)]
            if request.reason.as_deref() == Some("kapsam dışı") && request.expected_revision == 4)
        );
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
