//! The regular-expression replace bar of the open file: a pattern, what takes its place
//! (`$1` inserts a group), a live match count and one button that replaces every match as an
//! undoable edit. The built-in find box stays for plain text.

use gpui::{
    div, px, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::workbench::{Appearance, Theme};

/// What the bar asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileReplaceEvent {
    /// The pattern, the replacement or the case toggle changed: count the matches.
    Changed {
        /// The pattern.
        find: String,
        /// The case toggle's state.
        ignore_case: bool,
    },
    /// Replace every match.
    Apply {
        /// The pattern.
        find: String,
        /// The replacement text.
        with: String,
        /// The case toggle's state.
        ignore_case: bool,
    },
    /// The user closed the bar.
    Close,
}

impl EventEmitter<FileReplaceEvent> for FileReplaceView {}

/// The replace bar.
pub struct FileReplaceView {
    find: Entity<InputState>,
    with: Entity<InputState>,
    ignore_case: bool,
    note: String,
    theme: Theme,
    _subscriptions: Vec<Subscription>,
}

impl FileReplaceView {
    /// A bar whose pattern box takes focus.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find = cx.new(|cx| InputState::new(window, cx).placeholder("Desen (regex)"));
        find.update(cx, |input, cx| input.focus(window, cx));
        let with = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder("Yerine (ör. $1)")
        });
        let on_find = cx.subscribe(&find, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.changed(cx);
            }
        });
        let on_with = cx.subscribe(&with, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.apply(cx);
            }
        });
        Self {
            find,
            with,
            ignore_case: false,
            note: String::new(),
            theme: Theme::of(Appearance::default()),
            _subscriptions: vec![on_find, on_with],
        }
    }

    /// Repaints the bar with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Types into the pattern box (as typing would).
    pub fn set_find(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.find
            .update(cx, |input, cx| input.set_value(text, window, cx));
        // Count at once; typing announces itself through the box.
        self.changed(cx);
    }

    /// Types into the replacement box.
    pub fn set_with(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.with
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// What the bar says under its boxes: the match count, or why the pattern is refused.
    pub fn note(&self) -> &str {
        &self.note
    }

    /// Sets that text.
    pub fn set_note(&mut self, note: String, cx: &mut Context<Self>) {
        self.note = note;
        cx.notify();
    }

    /// Flips the case toggle.
    pub fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.ignore_case = !self.ignore_case;
        self.changed(cx);
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        let find = self.find.read(cx).value().to_string();
        cx.emit(FileReplaceEvent::Changed {
            find,
            ignore_case: self.ignore_case,
        });
    }

    /// Asks for every match to be replaced.
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        cx.emit(FileReplaceEvent::Apply {
            find: self.find.read(cx).value().to_string(),
            with: self.with.read(cx).value().to_string(),
            ignore_case: self.ignore_case,
        });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(FileReplaceEvent::Close);
            cx.stop_propagation();
        }
    }
}

impl Render for FileReplaceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let boxed = |input: &Entity<InputState>| {
            div()
                .flex_1()
                .h(px(32.0))
                .overflow_hidden()
                .rounded_sm()
                .border_1()
                .border_color(gpui::rgb(theme.line))
                .child(Input::new(input))
        };
        let button = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(gpui::rgb(if on { theme.accent_ink } else { theme.line }))
                .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                .child(label)
        };
        div()
            .absolute()
            .occlude()
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(720.0))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_md()
            .shadow_lg()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_color(gpui::rgb(theme.muted))
                            .child("DOSYADA REGEX DEĞİŞTİR"),
                    )
                    .child(
                        button("file-replace-case", "Aa", self.ignore_case)
                            .on_click(cx.listener(|this, _, _w, cx| this.toggle_case(cx))),
                    )
                    .child(
                        button("file-replace-close", "Kapat (Esc)", false)
                            .on_click(cx.listener(|_, _, _w, cx| cx.emit(FileReplaceEvent::Close))),
                    ),
            )
            .child(boxed(&self.find))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(boxed(&self.with))
                    .child(
                        button("file-replace-apply", "Hepsini değiştir", false)
                            .on_click(cx.listener(|this, _, _w, cx| this.apply(cx))),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(SharedString::from(self.note.clone())),
            )
    }
}
