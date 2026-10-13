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
        }
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
        let buttons = TARGETS.into_iter().filter(|to| *to != from).map(|to| {
            let name = to.identity().unwrap_or("shell");
            div()
                .id(SharedString::from(format!("handoff-to-{name}")))
                .px_3()
                .py_1()
                .border_1()
                .border_color(gpui::rgb(0x3794ff))
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
            .bg(gpui::rgb(0x252526))
            .text_color(gpui::rgb(0xe5e5e5))
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
                    .border_color(gpui::rgb(0x333333))
                    .child(Input::new(&self.input)),
            )
            .children(self.problem.clone().map(|problem| {
                div()
                    .text_xs()
                    .text_color(gpui::rgb(0xf14c4c))
                    .child(problem)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(0x9a9a9a))
                    .child("Not bir BLOCKER devri olarak kuyruğa alınır; parola ve anahtar yazmayın. Esc kapatır."),
            )
            .child(div().flex().flex_row().gap_2().children(buttons))
    }
}
