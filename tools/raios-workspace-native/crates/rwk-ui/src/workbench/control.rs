//! The workbench's daemon-facing operations: starting agents, queueing handoffs and registering
//! the project. Every one goes through the policy read and, when the policy says `confirm`, a
//! consent dialog that shows the exact payload; nothing is sent without an explicit yes.

use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Context, Window};
use rwk_core::consent::ConsentRequest;
use rwk_core::control::{
    ControlPlane, CreateHandoffRequest, EventSource, HandoffDecisionRequest, IdempotencyKey,
    LaunchAgentRequest, PolicyAction,
};
use rwk_core::error::AppError;
use rwk_core::guarded::{Guarded, GuardedOutcome};
use rwk_core::handoff_flow::{observe_receipt, TrackedHandoff};
use rwk_core::handoff_state::Observation;
use rwk_core::sessions::SessionKind;

use super::{PendingAction, Workbench};
use crate::handoff::{ComposerEvent, HandoffComposer, RejectHandoffPrompt, RejectPromptEvent};

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

    /// Connects (or removes) the daemon's live event stream: the inspector refreshes on typed
    /// events and folds the stream state into its banner, the 3 s poll stays as the fallback.
    pub fn set_event_source(
        &mut self,
        source: Option<Box<dyn EventSource>>,
        cx: &mut Context<Self>,
    ) {
        self.inspector
            .update(cx, |inspector, cx| inspector.set_event_source(source, cx));
    }

    /// Projects the application remembers as registered, so they are not offered again.
    pub fn set_known_projects(&mut self, projects: impl IntoIterator<Item = std::path::PathBuf>) {
        self.known_projects = projects.into_iter().collect();
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
                self.settle_resume(&op, cx);
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
                        self.settle_resume(&op, cx);
                        self.receiver_launch_failed(cx);
                        self.say(error.message, cx);
                    }
                }
            }
            Err(error) => {
                self.settle_resume(&op, cx);
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
                self.settle_resume(&op, cx);
                self.receiver_launch_failed(cx);
                self.say("Vazgeçildi; hiçbir şey gönderilmedi", cx);
            }
            Err(error) => {
                self.settle_resume(&op, cx);
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
            (Guarded::ResumeAgent { key, .. }, Ok(GuardedOutcome::AgentLaunchValidated)) => {
                self.resume_terminal(key, window, cx);
                self.receiver_started(cx);
            }
            (Guarded::CreateHandoff(request), Ok(GuardedOutcome::HandoffQueued(queued))) => {
                self.handoffs.push(
                    TrackedHandoff::queued(
                        queued.approval_id,
                        request.from.identity().unwrap_or("shell").to_owned(),
                        request.to.identity().unwrap_or("shell").to_owned(),
                    )
                    .with_task(self.queued_task.take()),
                );
                self.say("Devir notu kuyruğa alındı", cx);
                self.publish_handoffs(cx);
                self.start_receipt_polling(window, cx);
            }
            (Guarded::ApproveHandoff(request), Ok(GuardedOutcome::HandoffApproved)) => {
                // Consent only: the note still has to be consumed by the addressed agent's run.
                self.say(format!("Devir onaylandı: {}", request.approval_id), cx);
                self.inspector
                    .update(cx, |inspector, cx| inspector.refresh_now(cx));
            }
            (Guarded::RejectHandoff(request), Ok(GuardedOutcome::HandoffRejected)) => {
                self.say(format!("Devir reddedildi: {}", request.approval_id), cx);
                self.inspector
                    .update(cx, |inspector, cx| inspector.refresh_now(cx));
            }
            (
                Guarded::RegisterProject { root, .. },
                Ok(GuardedOutcome::ProjectRegistered { already_registered }),
            ) => {
                self.known_projects.insert(root.clone());
                cx.emit(super::WorkbenchEvent::ProjectRegistered(root));
                self.say(
                    if already_registered {
                        "Proje zaten kayıtlı"
                    } else {
                        "Proje Raios'a kaydedildi"
                    },
                    cx,
                );
            }
            (Guarded::CreateTask(_), Ok(GuardedOutcome::TaskCreated(_))) => {
                self.say("Görev eklendi", cx);
                self.refresh_tasks(cx);
            }
            (Guarded::UpdateTaskStatus(request), Ok(GuardedOutcome::TaskStatusChanged)) => {
                self.say(format!("Görev durumu: {}", request.status.label()), cx);
                self.refresh_tasks(cx);
            }
            (Guarded::AssignTask(request), Ok(GuardedOutcome::TaskAssigned(_))) => {
                let text = match request.assignee.and_then(SessionKind::agent) {
                    Some(agent) => format!("Görev {agent} ajanına atandı"),
                    None => "Görev ataması kaldırıldı".to_owned(),
                };
                self.say(text, cx);
                self.finish_bind(&request.task_id, request.assignee.is_some(), cx);
                self.refresh_tasks(cx);
            }
            (op, Ok(_)) => {
                self.settle_resume(&op, cx);
                self.say("Beklenmeyen daemon yanıtı", cx);
            }
            (op @ (Guarded::AssignTask(_) | Guarded::UpdateTaskStatus(_)), Err(error)) => {
                // The row may be stale: show what the daemon said and reload the list.
                let text = if error.code == rwk_core::error::ErrorCode::Conflict {
                    "Görev bu arada değişti; liste yenilendi, tekrar deneyin".to_owned()
                } else {
                    format!("Görev değiştirilemedi: {}", error.message)
                };
                drop(op);
                self.abandon_bind();
                self.say(text, cx);
                self.refresh_tasks(cx);
            }
            (op, Err(error)) => {
                if matches!(op, Guarded::CreateHandoff(_)) {
                    self.queued_task = None;
                }
                self.settle_resume(&op, cx);
                self.receiver_launch_failed(cx);
                self.say(error.message, cx);
            }
        }
    }

    // ----- tasks -----------------------------------------------------------------------

    fn refresh_tasks(&mut self, cx: &mut Context<Self>) {
        self.inspector
            .update(cx, |inspector, cx| inspector.refresh_now(cx));
    }

    /// Asks for the title of a new task in the open project.
    pub(super) fn open_new_task_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::rename::{RenamePromptEvent, RenameTerminalPrompt};

        if self.rename_prompt.is_some() {
            return;
        }
        let theme = self.theme();
        let prompt = cx.new(|cx| RenameTerminalPrompt::new_task(window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::NewTask(title) => {
                    this.close_rename_prompt(window, cx);
                    this.create_task(title.clone(), window, cx);
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::Rename(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    /// Adds an operator task (policy, consent, daemon).
    pub fn create_task(&mut self, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let title = match rwk_core::tasks::validate_title(&title) {
            Ok(title) => title,
            Err(error) => {
                self.say(error.message, cx);
                return;
            }
        };
        self.start_guarded(
            Guarded::CreateTask(rwk_core::control::CreateTaskRequest {
                project_root: project.info().root.clone(),
                title,
                priority: 50,
                idempotency_key: IdempotencyKey::fresh(),
            }),
            window,
            cx,
        );
    }

    /// Sets the status of an operator task (policy, consent, daemon).
    pub fn set_task_status(
        &mut self,
        task_id: String,
        status: rwk_core::tasks::TaskStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.start_guarded(
            Guarded::UpdateTaskStatus(rwk_core::control::UpdateTaskStatusRequest {
                project_root: project.info().root.clone(),
                task_id,
                status,
                idempotency_key: IdempotencyKey::fresh(),
            }),
            window,
            cx,
        );
    }

    /// Gives a task to an agent, or clears the assignee. `updated_at` is the revision the user
    /// saw; a newer change in the daemon is not overwritten.
    pub fn assign_task(
        &mut self,
        task_id: String,
        updated_at: String,
        assignee: Option<SessionKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.start_guarded(
            Guarded::AssignTask(rwk_core::control::AssignTaskRequest {
                project_root: project.info().root.clone(),
                task_id,
                assignee,
                expected_updated_at: Some(updated_at).filter(|revision| !revision.is_empty()),
                idempotency_key: IdempotencyKey::fresh(),
            }),
            window,
            cx,
        );
    }

    /// Gives a task to the agent that runs in terminal `index`. A shell has no agent to give it
    /// to, and nothing is sent then.
    pub fn assign_task_to_terminal(
        &mut self,
        task_id: String,
        updated_at: String,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.terminals.get(index).map(|pane| pane.kind);
        match kind {
            Some(kind) if kind.agent().is_some() => {
                if let Some(pane) = self.terminals.get(index) {
                    self.begin_bind(task_id.clone(), pane.id.clone());
                }
                self.assign_task(task_id, updated_at, Some(kind), window, cx);
            }
            Some(_) => self.say("Bu terminal bir ajan değil; görev atanamaz", cx),
            None => self.say("Önce bir ajan terminali seçin", cx),
        }
    }

    // ----- handoff composer ------------------------------------------------------------

    /// Opens the composer; the sender is the selected agent terminal, else Claude.
    /// With a task, the note starts from the task's title and the task follows the handoff once
    /// the receipt says it was delivered.
    pub(super) fn open_composer(
        &mut self,
        task: Option<rwk_core::handoff_flow::TaskLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.is_some() {
            return;
        }
        let from = self.selected_agent_kind().unwrap_or(SessionKind::Claude);
        let theme = self.theme();
        let seed = rwk_core::handoff_flow::handoff_seed(
            &self.terminal_title(self.selected),
            task.as_ref().map(|task| task.title.as_str()),
        );
        self.composer_task = task;
        let sender = self
            .terminals
            .get(self.selected)
            .map(|pane| pane.id.clone());
        let paste_targets: Vec<(rwk_core::sessions::SessionId, String)> = self
            .terminals
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                pane.kind.agent().is_some()
                    && Some(&pane.id) != sender.as_ref()
                    && self.session_running(&pane.id)
            })
            .map(|(index, pane)| (pane.id.clone(), self.terminal_title(index)))
            .collect();
        let composer = cx.new(|cx| HandoffComposer::new(from, window, cx));
        composer.update(cx, |composer, cx| {
            composer.set_paste_targets(paste_targets, cx);
            composer.set_theme(theme, cx);
            composer.set_text(&seed, window, cx);
        });
        self.composer_subscription = Some(cx.subscribe_in(
            &composer,
            window,
            |this, _composer, event: &ComposerEvent, window, cx| match event.clone() {
                ComposerEvent::Close => this.close_composer(window, cx),
                ComposerEvent::PasteTo { session, summary } => {
                    this.close_composer(window, cx);
                    this.paste_handoff(&session, summary, window, cx);
                }
                ComposerEvent::Send { to, summary } => {
                    let task = this.composer_task.clone();
                    this.close_composer(window, cx);
                    this.queued_task = task;
                    this.queue_handoff(to, summary, window, cx);
                }
            },
        ));
        self.composer = Some(composer);
        cx.notify();
    }

    /// Puts the handoff text into the paste preview of a running terminal and selects it. The
    /// user still has to press Enter there; nothing is recorded in the daemon.
    fn paste_handoff(
        &mut self,
        session: &rwk_core::sessions::SessionId,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.terminals.iter().position(|pane| &pane.id == session) else {
            self.say("Hedef terminal artık yok", cx);
            return;
        };
        if !self.session_running(session) {
            self.say("Hedef terminal çalışmıyor", cx);
            return;
        }
        self.select_terminal(index, window, cx);
        self.terminals[index]
            .view
            .update(cx, |view, cx| view.paste_text(summary, cx));
        let waiting = self.terminals[index].view.read(cx).has_pending_paste();
        self.say(
            if waiting {
                "Metin yapıştırma onayında: Enter gönderir, Esc vazgeçer. Devir kaydı oluşmadı"
            } else {
                "Metin terminale yazıldı, gönderilmedi: göndermek için orada Enter'a basın. Devir kaydı oluşmadı"
            },
            cx,
        );
    }

    /// Closes the composer and returns focus to the workbench.
    pub fn close_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer = None;
        self.composer_task = None;
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

    // ----- approvals ---------------------------------------------------------------------

    /// Approves a pending handoff approval: the control-plane owner's consent, not delivery.
    /// The addressed agent's own run still has to consume the note.
    pub fn approve_handoff(
        &mut self,
        approval_id: String,
        revision: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.start_guarded(
            Guarded::ApproveHandoff(HandoffDecisionRequest {
                project_root: project.info().root.clone(),
                approval_id,
                expected_revision: revision,
                reason: None,
                idempotency_key: IdempotencyKey::fresh(),
            }),
            window,
            cx,
        );
    }

    /// Asks for the reason to reject a pending handoff approval. The decision is sent only once
    /// the reason passes validation and the policy/consent guard has allowed it.
    pub fn request_reject_handoff(
        &mut self,
        approval_id: String,
        revision: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.reject_prompt.is_some() {
            return;
        }
        let theme = self.theme();
        let target = approval_id.clone();
        let prompt = cx.new(|cx| RejectHandoffPrompt::new(target, window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.reject_prompt_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            move |this, _prompt, event: &RejectPromptEvent, window, cx| match event {
                RejectPromptEvent::Reject { reason } => {
                    this.close_reject_prompt(window, cx);
                    this.send_rejection(approval_id.clone(), revision, reason.clone(), window, cx);
                }
                RejectPromptEvent::Close => this.close_reject_prompt(window, cx),
            },
        ));
        self.reject_prompt = Some(prompt);
        cx.notify();
    }

    /// Sends the rejection with the reason the user gave.
    fn send_rejection(
        &mut self,
        approval_id: String,
        revision: i64,
        reason: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.start_guarded(
            Guarded::RejectHandoff(HandoffDecisionRequest {
                project_root: project.info().root.clone(),
                approval_id,
                expected_revision: revision,
                reason: Some(reason),
                idempotency_key: IdempotencyKey::fresh(),
            }),
            window,
            cx,
        );
    }

    /// Closes the reject prompt and returns focus to the workbench.
    pub fn close_reject_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reject_prompt = None;
        self.reject_prompt_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The open reject prompt.
    pub fn reject_prompt(&self) -> Option<&gpui::Entity<RejectHandoffPrompt>> {
        self.reject_prompt.as_ref()
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
                    let _ = this.update_in(cx, |workbench, window, cx| {
                        workbench.apply_receipt(&id, Observation::Receipt(observation), cx);
                        workbench.move_task_after_delivery(&id, window, cx);
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

    /// Hands the task of a delivered handoff to the receiver. Delivery is what the receipt
    /// proved; a handoff that never arrived leaves the task where it was. The revision is the
    /// task's current one, so a change made in between is not overwritten silently.
    pub fn move_task_after_delivery(
        &mut self,
        approval_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tracked) = self
            .handoffs
            .iter_mut()
            .find(|tracked| tracked.approval_id == approval_id && tracked.task_move_due())
        else {
            return;
        };
        tracked.task_moved = true;
        let (Some(link), to) = (tracked.task.clone(), tracked.to.clone()) else {
            return;
        };
        let receiver = [
            SessionKind::Claude,
            SessionKind::Codex,
            SessionKind::Opencode,
            SessionKind::Agy,
        ]
        .into_iter()
        .find(|kind| kind.identity() == Some(to.as_str()));
        let revision = self
            .inspector
            .read(cx)
            .model()
            .tasks
            .iter()
            .find(|task| task.id == link.task_id)
            .map(|task| task.updated_at.clone());
        match (receiver, revision) {
            (Some(kind), Some(revision)) => {
                self.assign_task(link.task_id, revision, Some(kind), window, cx)
            }
            _ => self.say(
                format!("“{}” görevi güncel listede yok; elle atayın", link.title),
                cx,
            ),
        }
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

impl Workbench {
    /// A project the daemon does not know yet is registered with Raios (through the same policy
    /// and consent guard); a known one is left untouched. Nothing happens without a daemon.
    pub(super) fn register_if_unknown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(plane), Some(project)) = (self.plane.clone(), self.project.clone()) else {
            return;
        };
        let root = project.info().root.clone();
        if self.known_projects.contains(&root) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let known = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move {
                        plane.work_snapshot().map(|work| {
                            work.projects
                                .iter()
                                .any(|project| std::path::Path::new(&project.path) == root)
                        })
                    }
                })
                .await;
            // An unreachable daemon or a failed read registers nothing.
            if let Ok(false) = known {
                let _ = this.update_in(cx, |workbench, window, cx| {
                    let still_open = workbench
                        .project
                        .as_ref()
                        .is_some_and(|project| project.info().root == root);
                    if still_open {
                        workbench.register_project(window, cx);
                    }
                });
            }
        })
        .detach();
    }
}
