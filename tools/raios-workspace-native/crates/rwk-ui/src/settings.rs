//! The settings view: the switches of the workbench in one list. Choosing a row flips it; the
//! workbench applies the change and sends the new values back.

use gpui::{
    div, px, Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use rwk_core::workbench::{Appearance, Theme};

/// A switch the view can flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Save changed files when the window loses focus.
    AutoSave,
    /// Format with the language server before saving.
    FormatOnSave,
    /// Dark or light tone.
    Tone,
    /// The neutral or the GT-LCARS skin.
    Skin,
}

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRow {
    /// The switch.
    pub setting: Setting,
    /// What it does.
    pub label: String,
    /// Its current value.
    pub value: String,
}

/// What the view asks of the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Flip this switch.
    Toggle(Setting),
    /// The user closed the view.
    Close,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

/// The settings view.
pub struct SettingsView {
    rows: Vec<SettingRow>,
    selected: usize,
    theme: Theme,
    focus: FocusHandle,
}

impl SettingsView {
    /// A view of `rows`; it takes focus so the arrow keys, Enter and Esc work.
    pub fn new(rows: Vec<SettingRow>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            rows,
            selected: 0,
            theme: Theme::of(Appearance::default()),
            focus,
        }
    }

    /// Repaints the view with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Shows the values after a change.
    pub fn set_rows(&mut self, rows: Vec<SettingRow>, cx: &mut Context<Self>) {
        self.rows = rows;
        cx.notify();
    }

    /// The lines on screen.
    pub fn rows(&self) -> &[SettingRow] {
        &self.rows
    }

    /// Asks for the highlighted switch to be flipped.
    pub fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.selected) {
            cx.emit(SettingsEvent::Toggle(row.setting));
        }
    }

    /// Asks for `setting` to be flipped.
    pub fn toggle(&mut self, setting: Setting, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::Toggle(setting));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let last = self.rows.len().saturating_sub(1);
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SettingsEvent::Close),
            "down" => self.selected = (self.selected + 1).min(last),
            "up" => self.selected = self.selected.saturating_sub(1),
            "enter" | "space" => self.toggle_selected(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let selected = self.selected;
        let rows = self.rows.iter().cloned().enumerate().map(|(ix, row)| {
            let line = div()
                .id(("setting", ix))
                .px_3()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                .flex()
                .flex_row()
                .gap_2()
                .child(div().flex_1().child(SharedString::from(row.label)))
                .child(
                    div()
                        .text_color(gpui::rgb(theme.accent_ink))
                        .child(SharedString::from(row.value)),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.selected = ix;
                    this.toggle_selected(cx);
                }));
            if ix == selected {
                line.bg(gpui::rgb(theme.selected))
            } else {
                line
            }
        });
        div()
            .absolute()
            .occlude()
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(560.0))
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
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_color(gpui::rgb(theme.muted))
                            .child("AYARLAR"),
                    )
                    .child(
                        div()
                            .id("settings-close")
                            .px_3()
                            .py_0p5()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                            .child("Kapat (Esc)")
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(SettingsEvent::Close)),
                            ),
                    ),
            )
            .children(rows)
            .child(
                div().text_xs().text_color(gpui::rgb(theme.muted)).child(
                    "Dil sunucuları: ~/.config/raios-workspace-native/language-servers.json",
                ),
            )
    }
}
