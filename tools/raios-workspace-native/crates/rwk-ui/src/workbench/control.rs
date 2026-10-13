//! The workbench's daemon-facing operations: starting agents, queueing handoffs and registering
//! the project. Every one goes through the policy read and, when the policy says `confirm`, a
//! consent dialog that shows the exact payload; nothing is sent without an explicit yes.

use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Context, Window};
use rwk_core::consent::ConsentRequest;
use rwk_core::control::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
};
use rwk_core::error::AppError;
use rwk_core::guarded::{Guarded, GuardedOutcome};
use rwk_core::handoff_flow::{observe_receipt, TrackedHandoff};
use rwk_core::handoff_state::Observation;
use rwk_core::sessions::SessionKind;

use super::{PendingAction, Workbench};
use crate::handoff::{ComposerEvent, HandoffComposer};

/// How often open handoffs are read again.
const RECEIPT_POLL: Duration = Duration::from_secs(3);

impl Workbench {
    /// Connects the daemon: the inspector reads it, guarded commands send through it.
    pub fn set_control_plane(
        &mut self,
        plane: Option<Arc<dyn ControlPlane>>,
        cx: &mut Context<Self>,
    ) {
        self.plane = plane.clone();
        self.inspector
            .update(cx, |inspector, cx| inspector.set_control_plane(plane, cx));
        cx.notify();
    }

    /// True while a daemon connection is configured.
    pub fn has_control_plane(&self) -> bool {
        self.plane.is_some()
    }

    /// The handoffs this workspace queued, newest last.
    pub fn handoffs(&self) -> &[TrackedHandoff] {
        &self.handoffs
    }

    /// Asks which agent to start.
    pub(super) fn open_agent_chooser(&mut self, cx: &mut Context<Self>) {
        self.pending = Some(PendingAction::ChooseAgent);
        cx.notify();
    }

    /// Starts `kind` in the open project: policy, consent, daemon validation, then the terminal.
    pub fn launch_agent(&mut self, kind: SessionKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let request = LaunchAgentRequest {
            project_root: project.info().root.clone(),
            agent: kind,
            prompt: None,
            idempotency_key: IdempotencyKey::fresh(),
        };
        self.start_guarded(Guarded::LaunchAgent(request), window, cx);
    }

    /// Registers the open project with Raios, through the same guard.
    pub(super) fn register_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let op = Guarded::RegisterProject {
            root: project.info().root.clone(),
            key: IdempotencyKey::fresh(),
        };
        self.start_guarded(op, window, cx);
    }

    /// Reads the policy for `op` and goes on: send, refuse, or ask.
    pub fn start_guarded(&mut self, op: Guarded, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plane) = self.plane.clone() else {
            self.say("Daemon bağlantısı yapılandırılmadı", cx);
            return;
        };
        let name = op.command_name();
        cx.spawn_in(window, async move |this, cx| {
            let reader = Arc::clone(&plane);
            let policy = cx
                .background_executor()
                .spawn(async move { reader.command_policy(name) })
                .await;
            let _ = this.update_in(cx, |workbench, window, cx| {
                workbench.after_policy(op, policy, window, cx);
            });
        })
        .detach();
    }

    fn after_policy(
        &mut self,
        op: Guarded,
        policy: Result<PolicyAction, AppError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match policy {
            Ok(PolicyAction::Allow) => self.run_guarded(op, window, cx),
            Ok(PolicyAction::Deny) => {
                self.receiver_launch_failed(cx);
                self.say(
                    format!("Politika '{}' komutunu reddediyor", op.command_name()),
                    cx,
                );
            }
            Ok(PolicyAction::Confirm) => {
                match self
                    .consent
                    .begin(op.command_name(), op.project_root(), &op.summary())
                {
                    Ok((request, receiver)) => {
                        // The registry sends the answer into this channel, so it must stay open
                        // until the dialog is answered.
                        self.consent_wait = Some(receiver);
                        self.pending = Some(PendingAction::Consent(request, Box::new(op)));
                        cx.notify();
                    }
                    Err(error) => {
                        self.receiver_launch_failed(cx);
                        self.say(error.message, cx);
                    }
                }
            }
            Err(error) => {
                self.receiver_launch_failed(cx);
                self.say(error.message, cx);
            }
        }
    }

    /// Applies the answer to a consent dialog.
    pub(super) fn answer_consent(
        &mut self,
        request: &ConsentRequest,
        op: Guarded,
        approved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let decision = if approved {
            rwk_core::consent::ConsentDecision::Approved
        } else {
            rwk_core::consent::ConsentDecision::Denied
        };
        let result = self.consent.decide(&request.id, decision);
        self.consent_wait = None;
        match result {
            Ok(()) if approved => self.run_guarded(op, window, cx),
            Ok(()) => {
                self.receiver_launch_failed(cx);
                self.say("Vazgeçildi; hiçbir şey gönderilmedi", cx);
            }
            Err(error) => {
                self.receiver_launch_failed(cx);
                self.say(error.message, cx);
            }
        }
    }

    fn run_guarded(&mut self, op: Guarded, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plane) = self.plane.clone() else {
            return;
        };
        let sent = op.clone();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { sent.execute(plane.as_ref()) })
                .await;
            let _ = this.update_in(cx, |workbench, window, cx| {
                workbench.after_guarded(op, outcome, window, cx);
            });
        })
        .detach();
    }

    fn after_guarded(
        &mut self,
        op: Guarded,
        outcome: Result<GuardedOutcome, AppError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (op, outcome) {
            (Guarded::LaunchAgent(request), Ok(GuardedOutcome::AgentLaunchValidated)) => {
                self.new_terminal(request.agent, window, cx);
                self.receiver_started(cx);
            }
            (Guarded::CreateHandoff(request), Ok(GuardedOutcome::HandoffQueued(queued))) => {
                self.handoffs.push(TrackedHandoff::queued(
                    queued.approval_id,
                    request.from.identity().unwrap_or("shell").to_owned(),
                    request.to.identity().unwrap_or("shell").to_owned(),
                ));
                self.say("Devir notu kuyruğa alındı", cx);
                self.publish_handoffs(cx);
                self.start_receipt_polling(window, cx);
            }
            (
                Guarded::RegisterProject { .. },
                Ok(GuardedOutcome::ProjectRegistered { already_registered }),
            ) => {
                self.say(
                    if already_registered {
                        "Proje zaten kayıtlı"
                    } else {
                        "Proje Raios'a kaydedildi"
                    },
                    cx,
                );
            }
            (_, Ok(_)) => self.say("Beklenmeyen daemon yanıtı", cx),
            (_, Err(error)) => {
                self.receiver_launch_failed(cx);
                self.say(error.message, cx);
            }
        }
    }

    // ----- handoff composer ------------------------------------------------------------

    /// Opens the composer; the sender is the selected agent terminal, else Claude.
    pub(super) fn open_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_some() {
            return;
        }
        let from = self.selected_agent_kind().unwrap_or(SessionKind::Claude);
        let theme = self.theme();
        let composer = cx.new(|cx| HandoffComposer::new(from, window, cx));
        composer.update(cx, |composer, cx| composer.set_theme(theme, cx));
        self.composer_subscription = Some(cx.subscribe_in(
            &composer,
            window,
            |this, _composer, event: &ComposerEvent, window, cx| match event.clone() {
                ComposerEvent::Close => this.close_composer(window, cx),
                ComposerEvent::Send { to, summary } => {
                    this.close_composer(window, cx);
                    this.queue_handoff(to, summary, window, cx);
                }
            },
        ));
        self.composer = Some(composer);
        cx.notify();
    }

    /// Closes the composer and returns focus to the workbench.
    pub fn close_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer = None;
        self.composer_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The open composer.
    pub fn composer(&self) -> Option<&gpui::Entity<HandoffComposer>> {
        self.composer.as_ref()
    }

    fn selected_agent_kind(&self) -> Option<SessionKind> {
        let pane = self.terminals.get(self.selected)?;
        let kind = pane.kind;
        kind.identity().map(|_| kind)
    }

    fn queue_handoff(
        &mut self,
        to: SessionKind,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(project), Some(from)) = (
            self.project.clone(),
            Some(self.selected_agent_kind().unwrap_or(SessionKind::Claude)),
        ) else {
            return;
        };
        let request = CreateHandoffRequest {
            project_root: project.info().root.clone(),
            from,
            to,
            summary,
            idempotency_key: IdempotencyKey::fresh(),
        };
        self.start_guarded(Guarded::CreateHandoff(request), window, cx);
    }

    // ----- receipts --------------------------------------------------------------------

    fn publish_handoffs(&mut self, cx: &mut Context<Self>) {
        let lines: Vec<_> = self
            .handoffs
            .iter()
            .map(|tracked| {
                (
                    tracked.approval_id.clone(),
                    tracked.label(),
                    tracked.can_start_receiver(),
                )
            })
            .collect();
        self.inspector
            .update(cx, |inspector, cx| inspector.set_handoffs(lines, cx));
    }

    fn start_receipt_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.receipt_task.is_some() {
            return;
        }
        self.receipt_task = Some(cx.spawn_in(window, async move |this, cx| loop {
            let open = this
                .update(cx, |workbench, _| workbench.open_receipts())
                .unwrap_or_default();
            let _ = this.update(cx, |workbench, cx| workbench.check_receiver(cx));
            if open.is_empty() {
                let _ = this.update(cx, |workbench, _| workbench.receipt_task = None);
                return;
            }
            let Some(plane) = this
                .update(cx, |workbench, _| workbench.plane.clone())
                .ok()
                .flatten()
            else {
                return;
            };
            let root = this
                .update(cx, |workbench, _| {
                    workbench
                        .project
                        .as_ref()
                        .map(|project| project.info().root.clone())
                })
                .ok()
                .flatten();
            if let Some(root) = root {
                for id in open {
                    let reader = Arc::clone(&plane);
                    let path = root.clone();
                    let wanted = id.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move { reader.handoff_receipt(&wanted, &path) })
                        .await;
                    let observation = observe_receipt(&result, &id);
                    let _ = this.update(cx, |workbench, cx| {
                        workbench.apply_receipt(&id, Observation::Receipt(observation), cx);
                    });
                }
            }
            cx.background_executor().timer(RECEIPT_POLL).await;
        }));
    }

    fn open_receipts(&self) -> Vec<String> {
        self.handoffs
            .iter()
            .filter(|tracked| tracked.is_open())
            .map(|tracked| tracked.approval_id.clone())
            .collect()
    }

    /// Applies an observation to the tracked handoff `approval_id`.
    pub fn apply_receipt(
        &mut self,
        approval_id: &str,
        observation: Observation,
        cx: &mut Context<Self>,
    ) {
        if let Some(tracked) = self
            .handoffs
            .iter_mut()
            .find(|tracked| tracked.approval_id == approval_id)
        {
            tracked.observe(&observation);
        }
        self.publish_handoffs(cx);
        cx.notify();
    }

    /// Starts the receiver of a pending handoff: the receipt is read again first, and the agent
    /// starts only when it is still pending and no owned receiver runs.
    pub fn start_receiver(
        &mut self,
        approval_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(plane), Some(project)) = (self.plane.clone(), self.project.clone()) else {
            return;
        };
        let root = project.info().root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let wanted = approval_id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { plane.handoff_receipt(&wanted, &root) })
                .await;
            let observation = Observation::Receipt(observe_receipt(&result, &approval_id));
            let _ = this.update_in(cx, |workbench, window, cx| {
                workbench.apply_receipt(&approval_id, observation, cx);
                workbench.launch_receiver(&approval_id, window, cx);
            });
        })
        .detach();
    }

    fn launch_receiver(&mut self, approval_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tracked) = self
            .handoffs
            .iter_mut()
            .find(|tracked| tracked.approval_id == approval_id)
        else {
            return;
        };
        if !tracked.can_start_receiver() {
            self.say("Alıcı şu an başlatılamaz", cx);
            return;
        }
        let kind = [
            SessionKind::Claude,
            SessionKind::Codex,
            SessionKind::Opencode,
            SessionKind::Agy,
        ]
        .into_iter()
        .find(|kind| kind.identity() == Some(tracked.to.as_str()));
        let Some(kind) = kind else {
            return;
        };
        tracked.observe(&Observation::LaunchRequested);
        self.publish_handoffs(cx);
        self.launch_agent(kind, window, cx);
        // The agent terminal appears after policy, consent and validation; until then the
        // receiver counts as launching, and a refusal below clears it.
        self.receiver_for = Some(approval_id.to_owned());
    }
}

impl Workbench {
    /// A guarded launch that was meant to start a handoff receiver did not go through.
    fn receiver_launch_failed(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.receiver_for.take() {
            self.apply_receipt(&id, Observation::LaunchFailed, cx);
        }
    }

    /// The agent terminal of a handoff receiver exists now.
    fn receiver_started(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.receiver_for.take() else {
            return;
        };
        if let Some(pane) = self.terminals.last() {
            self.receiver_session = Some((id.clone(), pane.id.clone()));
        }
        self.apply_receipt(&id, Observation::ReceiverRunning, cx);
    }

    /// Notices an owned receiver whose process ended.
    pub(super) fn check_receiver(&mut self, cx: &mut Context<Self>) {
        let Some((approval_id, session)) = self.receiver_session.clone() else {
            return;
        };
        let alive = self
            .sessions
            .list()
            .iter()
            .any(|info| info.id == session && info.running);
        if !alive {
            self.receiver_session = None;
            self.apply_receipt(&approval_id, Observation::ReceiverExited, cx);
        }
    }
}
