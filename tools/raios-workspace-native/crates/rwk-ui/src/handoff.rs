//! The handoff composer: a small modal where the user writes the context for the next agent and
//! picks who receives it. It only validates and reports; the workbench runs consent and sends.

use gpui::{
    div, Context, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui::{AppContext, Entity};
use gpui_base::input::{Input, InputState};
use rwk_core::handoff_input::{looks_like_secret, validate_summary};
use rwk_core::sessions::SessionKind;
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
    /// The user closed the composer.
    Close,
}

impl EventEmitter<ComposerEvent> for HandoffComposer {}

/// The composer view.
pub struct HandoffComposer {
    input: Entity<InputState>,
    from: SessionKind,
    problem: Option<SharedString>,
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
            theme: Theme::of(Appearance::default()),
        }
    }

    /// Repaints the composer with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
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
        div()
            .absolute()
            .top_16()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(620.0))
            .p_3()
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
                    .child("Not bir BLOCKER devri olarak kuyruğa alınır; parola ve anahtar yazmayın. Esc kapatır."),
            )
            .child(div().flex().flex_row().gap_2().children(buttons))
    }
}
