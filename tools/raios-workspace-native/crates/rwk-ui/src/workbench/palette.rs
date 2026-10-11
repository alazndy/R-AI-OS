//! The command palette overlay: a query box over the filtered command list.

use gpui::{
    div, App, AppContext, Context, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Styled, Subscription, Window,
};
use gpui_base::input::{InputEvent, InputState};
use gpui_base::Input;
use rwk_core::workbench::{palette_entries, CommandId, PaletteEntry, WorkbenchState};

/// What the palette tells the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteEvent {
    /// Run this command (it was enabled when chosen).
    Run(CommandId),
    /// Close without running anything.
    Close,
}

/// The open palette.
pub struct PaletteView {
    input: Entity<InputState>,
    state: WorkbenchState,
    entries: Vec<PaletteEntry>,
    selected: usize,
    _subscription: Subscription,
}

use gpui::Entity;

impl EventEmitter<PaletteEvent> for PaletteView {}

impl PaletteView {
    /// A palette over the commands available in `state`; the query box takes focus.
    pub fn new(state: WorkbenchState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription =
            cx.subscribe(&input, |this, _input, event: &InputEvent, cx| match event {
                InputEvent::Change => this.refilter(cx),
                InputEvent::PressEnter { .. } => this.confirm(cx),
                InputEvent::Focus | InputEvent::Blur => {}
            });
        Self {
            input,
            state,
            entries: palette_entries("", &state),
            selected: 0,
            _subscription: subscription,
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.entries = palette_entries(&query, &self.state);
        self.selected = 0;
        cx.notify();
    }

    /// The text in the query box.
    pub fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Replaces the query (as typing would).
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.refilter(cx);
    }

    /// The rows currently shown.
    pub fn entries(&self) -> &[PaletteEntry] {
        &self.entries
    }

    /// Index of the highlighted row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the highlight, wrapping at both ends.
    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        let count = self.entries.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        cx.notify();
    }

    /// Runs the highlighted command when it is available; an unavailable one stays on screen
    /// with its reason.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.state.enabled {
                cx.emit(PaletteEvent::Run(entry.id));
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(PaletteEvent::Close),
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for PaletteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.entries.iter().enumerate().map(|(index, entry)| {
            let marker = if index == self.selected { "› " } else { "  " };
            let shortcut = entry.shortcut.unwrap_or("");
            let reason = entry.state.reason.unwrap_or("");
            let text = if entry.state.enabled {
                format!("{marker}{}   {shortcut}", entry.label)
            } else {
                format!("{marker}{}   ({reason})", entry.label)
            };
            let row = div().px_2().child(SharedString::from(text));
            if entry.state.enabled {
                row
            } else {
                row.opacity(0.5)
            }
        });
        div()
            .absolute()
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(560.0))
            .p_2()
            .bg(gpui::rgb(0x252526))
            .text_color(gpui::rgb(0xe5e5e5))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(Input::new(&self.input))
            .child(div().flex().flex_col().children(rows))
    }
}
