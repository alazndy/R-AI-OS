//! The handoff composer: a small modal where the user writes the context for the next agent and
//! picks who receives it. It only validates and reports; the workbench runs consent and sends.

use gpui::{
    div, Context, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
};
use gpui::{AppContext, Entity};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::handoff_input::{looks_like_secret, validate_summary};
use rwk_core::sessions::{SessionId, SessionKind};
use rwk_core::workbench::{Appearance, Theme};

/// The agents a handoff can be addressed to, in the order the buttons appear.
pub const TARGETS: [SessionKind; 4] = [
    SessionKind::Claude,
    SessionKind::Codex,
    SessionKind::Opencode,
    SessionKind::Agy,
];

/// What the composer asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerEvent {
    /// The user chose a receiver and the text passed validation.
    Send {
        /// Receiving agent.
        to: SessionKind,
        /// The summary text.
        summary: String,
    },
    /// The user wants the text pasted into a running terminal instead of queued as a handoff.
    /// Nothing is recorded: no receipt exists for a paste.
    PasteTo {
        /// The terminal to paste into.
        session: SessionId,
        /// The text, validated like a handoff summary.
        summary: String,
    },
    /// The user closed the composer.
    Close,
}

impl EventEmitter<ComposerEvent> for HandoffComposer {}

/// The composer view.
pub struct HandoffComposer {
    input: Entity<InputState>,
    from: SessionKind,
    problem: Option<SharedString>,
    /// Running agent terminals the text can be pasted into: session and the label on the button.
    paste_targets: Vec<(SessionId, String)>,
    theme: Theme,
}

impl HandoffComposer {
    /// A composer for a handoff from `from`; the text box takes focus.
    pub fn new(from: SessionKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| input.focus(window, cx));
        Self {
            input,
            from,
            problem: None,
            paste_targets: Vec::new(),
            theme: Theme::of(Appearance::default()),
        }
    }

    /// Repaints the composer with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Sets the terminals the text can be pasted into.
    pub fn set_paste_targets(&mut self, targets: Vec<(SessionId, String)>, cx: &mut Context<Self>) {
        self.paste_targets = targets;
        cx.notify();
    }

    /// Validates the text and, when it passes, asks the workbench to paste it into `session`.
    pub fn submit_paste(&mut self, session: SessionId, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if let Err(problem) = validate_summary(&text, &looks_like_secret) {
            self.problem = Some(problem.message().into());
        } else {
            self.problem = None;
            cx.emit(ComposerEvent::PasteTo {
                session,
                summary: text.trim().to_owned(),
            });
        }
        cx.notify();
    }

    /// The handing-off agent.
    pub fn from(&self) -> SessionKind {
        self.from
    }

    /// The text typed so far.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Replaces the text (as typing would).
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// The message shown under the text box.
    pub fn problem(&self) -> Option<String> {
        self.problem.as_ref().map(ToString::to_string)
    }

    /// Validates the text and, when it passes, asks the workbench to send it to `to`.
    pub fn submit(&mut self, to: SessionKind, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if to == self.from {
            self.problem = Some("Alıcı, gönderenle aynı olamaz.".into());
        } else if let Err(problem) = validate_summary(&text, &looks_like_secret) {
            self.problem = Some(problem.message().into());
        } else {
            self.problem = None;
            cx.emit(ComposerEvent::Send {
                to,
                summary: text.trim().to_owned(),
            });
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(ComposerEvent::Close);
            cx.stop_propagation();
        }
    }
}

impl Render for HandoffComposer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let from = self.from;
        let theme = self.theme;
        let buttons = TARGETS.into_iter().filter(|to| *to != from).map(|to| {
            let name = to.identity().unwrap_or("shell");
            div()
                .id(SharedString::from(format!("handoff-to-{name}")))
                .px_3()
                .py_1()
                .border_1()
                .border_color(gpui::rgb(theme.accent_ink))
                .child(SharedString::from(format!("→ {name}")))
                .on_click(cx.listener(move |this, _, _window, cx| this.submit(to, cx)))
        });
        let pastes: Vec<_> = self
            .paste_targets
            .iter()
            .enumerate()
            .map(|(index, (session, label))| {
                let session = session.clone();
                div()
                    .id(SharedString::from(format!("handoff-paste-{index}")))
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(SharedString::from(format!("Yapıştır → {label}")))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.submit_paste(session.clone(), cx)
                    }))
            })
            .collect();
        div()
            .absolute()
            .occlude()
            .child(crate::overlay::close_button(self.theme, "composer-close").on_click(cx.listener(|_, _, _window, cx| cx.emit(ComposerEvent::Close))))
            .top_16()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(620.0))
            .p_4()
            .rounded_md()
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_2()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(SharedString::from(format!(
                "Devir notu · gönderen {}",
                from.identity().unwrap_or("shell")
            )))
            .child(
                div()
                    .h(gpui::px(160.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.input)),
            )
            .children(self.problem.clone().map(|problem| {
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.bad))
                    .child(problem)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child("Not bir BLOCKER devri olarak kuyruğa alınır; parola ve anahtar yazmayın. Aynı ajana devredilemez (gönderenin düğmesi yoktur). Esc kapatır."),
            )
            .child(div().flex().flex_row().gap_2().children(buttons))
            .children((!pastes.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme.muted))
                            .child("Çalışan bir terminale yapıştır: metin onay önizlemesinde bekler, Enter ile gider. Bu bir devir kaydı değildir; ajan metni olduğu gibi girdi olarak alır."),
                    )
                    .child(div().flex().flex_row().flex_wrap().gap_2().children(pastes))
            }))
    }
}

/// The longest rejection reason the daemon accepts, in bytes.
const MAX_REASON_BYTES: usize = 2_000;

/// What the reject prompt asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectPromptEvent {
    /// The user confirmed the reason.
    Reject {
        /// The rejection reason, trimmed.
        reason: String,
    },
    /// The user closed the prompt.
    Close,
}

impl EventEmitter<RejectPromptEvent> for RejectHandoffPrompt {}

/// The reject prompt: a small modal asking why a pending handoff approval is refused. It only
/// validates and reports; the workbench runs consent and sends.
pub struct RejectHandoffPrompt {
    approval_id: String,
    input: Entity<InputState>,
    problem: Option<SharedString>,
    theme: Theme,
    _subscription: Subscription,
}

impl RejectHandoffPrompt {
    /// A prompt for `approval_id`; the text box takes focus.
    pub fn new(approval_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe(&input, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit(cx);
            }
        });
        Self {
            approval_id,
            input,
            problem: None,
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// The approval being decided.
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }

    /// Repaints the prompt with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// The text typed so far.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Replaces the text (as typing would).
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// The message shown under the text box.
    pub fn problem(&self) -> Option<String> {
        self.problem.as_ref().map(ToString::to_string)
    }

    /// Validates the reason and, when it passes, asks the workbench to reject the handoff. The
    /// daemon accepts at most 2000 bytes, and the reason lands in the audit ledger, so a
    /// credential-shaped text is refused here too.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let reason = text.trim();
        if reason.is_empty() {
            self.problem = Some("Reddetmek için bir gerekçe gerekli.".into());
        } else if reason.len() > MAX_REASON_BYTES {
            self.problem =
                Some(format!("Gerekçe en fazla {MAX_REASON_BYTES} bayt olabilir.").into());
        } else if looks_like_secret(reason) {
            self.problem =
                Some("Metin bir kimlik bilgisi gibi görünüyor; gerekçeden çıkarın.".into());
        } else {
            self.problem = None;
            cx.emit(RejectPromptEvent::Reject {
                reason: reason.to_owned(),
            });
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(RejectPromptEvent::Close);
            cx.stop_propagation();
        }
    }
}

impl Render for RejectHandoffPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id("reject-handoff-prompt")
            .absolute()
            .occlude()
            .child(crate::overlay::close_button(self.theme, "reject-close").on_click(cx.listener(|_, _, _window, cx| cx.emit(RejectPromptEvent::Close))))
            .top_16()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(520.0))
            .p_4()
            .rounded_md()
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_2()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(SharedString::from(format!(
                "Devir reddi · onay {}",
                self.approval_id
            )))
            .child(
                div()
                    .h(gpui::px(30.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.input)),
            )
            .children(self.problem.clone().map(|problem| {
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.bad))
                    .child(problem)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child("Gerekçe denetim kaydına geçer; parola ve anahtar yazmayın. Enter reddeder, Esc kapatır."),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("reject-handoff-confirm")
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(gpui::rgb(theme.bad))
                            .child(SharedString::from("Reddet"))
                            .on_click(cx.listener(|this, _, _window, cx| this.submit(cx))),
                    )
                    .child(
                        div()
                            .id("reject-handoff-cancel")
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(gpui::rgb(theme.line))
                            .child(SharedString::from("Vazgeç"))
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(RejectPromptEvent::Close);
                            })),
                    ),
            )
    }
}
