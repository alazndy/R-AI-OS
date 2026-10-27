//! The settings view: the switches and choices of the workbench in sections. A switch flips on
//! a click, Enter or space; a choice picks the option clicked, or the previous/next one with
//! the arrow keys. The workbench applies the change and sends the new values back.

use gpui::{
    div, px, Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use rwk_core::workbench::{Appearance, Theme};

/// A setting the view can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Save changed files when the window loses focus.
    AutoSave,
    /// Format with the language server before saving.
    FormatOnSave,
    /// Dark or light tone.
    Tone,
    /// The skin: standard, GT-LCARS or Aperture.
    Skin,
}

/// One option of a choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    /// What the option is called.
    pub label: String,
    /// Background, accent and blue of the option, drawn as a small preview.
    pub swatch: Option<[u32; 3]>,
}

/// The current value of a setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
    /// A switch.
    Switch(bool),
    /// One of several options.
    Choice {
        /// The options, in order.
        options: Vec<ChoiceOption>,
        /// Index of the chosen one.
        selected: usize,
    },
}

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRow {
    /// The setting.
    pub setting: Setting,
    /// The heading of the section the row sits under.
    pub section: &'static str,
    /// What it is.
    pub label: String,
    /// One line on what it does.
    pub hint: String,
    /// Its current value.
    pub value: SettingValue,
}

/// What the view asks of the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Flip the switch, or move a choice to its next option.
    Toggle(Setting),
    /// Pick option `index` of a choice.
    Choose(Setting, usize),
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
    /// A view of `rows`; it takes focus so the keys work.
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

    /// Asks for the highlighted switch to be flipped (a choice moves to its next option).
    pub fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.selected) {
            cx.emit(SettingsEvent::Toggle(row.setting));
        }
    }

    /// Asks for `setting` to be flipped.
    pub fn toggle(&mut self, setting: Setting, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::Toggle(setting));
    }

    /// Asks for option `index` of the choice `setting`.
    pub fn choose(&mut self, setting: Setting, index: usize, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::Choose(setting, index));
    }

    /// Moves the highlighted choice by `delta` options, staying inside the list.
    fn step_choice(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(self.selected) else {
            return;
        };
        if let SettingValue::Choice { options, selected } = &row.value {
            let next = selected
                .saturating_add_signed(delta)
                .min(options.len().saturating_sub(1));
            if next != *selected {
                cx.emit(SettingsEvent::Choose(row.setting, next));
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let last = self.rows.len().saturating_sub(1);
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SettingsEvent::Close),
            "down" => self.selected = (self.selected + 1).min(last),
            "up" => self.selected = self.selected.saturating_sub(1),
            "left" => self.step_choice(-1, cx),
            "right" => self.step_choice(1, cx),
            "enter" | "space" => self.toggle_selected(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The control on the right of a row.
    fn control(&self, ix: usize, row: &SettingRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = self.theme;
        let setting = row.setting;
        match &row.value {
            SettingValue::Switch(on) => {
                let on = *on;
                let track = div()
                    .id(("switch", ix))
                    .flex_none()
                    .w(px(40.0))
                    .h(px(22.0))
                    .px(px(3.0))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .border_1()
                    .border_color(gpui::rgb(if on { theme.accent } else { theme.line }))
                    .bg(gpui::rgb(if on { theme.accent } else { theme.raised }))
                    .when(on, |track| track.justify_end())
                    .child(div().size(px(14.0)).rounded_full().bg(gpui::rgb(if on {
                        theme.on_fill
                    } else {
                        theme.muted
                    })))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.selected = ix;
                        this.toggle(setting, cx);
                    }));
                track.into_any_element()
            }
            SettingValue::Choice { options, selected } => div()
                .flex()
                .flex_row()
                .gap_1p5()
                .children(options.iter().enumerate().map(|(index, option)| {
                    let chosen = index == *selected;
                    div()
                        .id(("choice", ix * 16 + index))
                        .flex_none()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_sm()
                        .cursor_pointer()
                        .border_1()
                        .border_color(gpui::rgb(if chosen { theme.accent } else { theme.line }))
                        .bg(gpui::rgb(if chosen { theme.selected } else { theme.panel }))
                        .text_color(gpui::rgb(if chosen { theme.text } else { theme.muted }))
                        .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                        .children(option.swatch.map(|[bg, accent, blue]| {
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .rounded_full()
                                .border_1()
                                .border_color(gpui::rgb(theme.line))
                                .bg(gpui::rgb(bg))
                                .px(px(2.0))
                                .gap(px(2.0))
                                .children([accent, blue].map(|dot| {
                                    div().size(px(8.0)).rounded_full().bg(gpui::rgb(dot))
                                }))
                        }))
                        .child(SharedString::from(option.label.clone()))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.selected = ix;
                            this.choose(setting, index, cx);
                        }))
                }))
                .into_any_element(),
        }
    }
}

use gpui::prelude::FluentBuilder as _;

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let selected = self.selected;
        let mut body = div().flex().flex_col().gap_1();
        let mut section = "";
        for (ix, row) in self.rows.clone().into_iter().enumerate() {
            if row.section != section {
                section = row.section;
                body = body.child(
                    div()
                        .mt_3()
                        .mb_1()
                        .px_3()
                        .text_size(px(10.0))
                        .text_color(gpui::rgb(theme.accent_ink))
                        .child(SharedString::from(row.section)),
                );
            }
            let control = self.control(ix, &row, cx);
            let line = div()
                .px_3()
                .py_2()
                .rounded_md()
                .flex()
                .flex_row()
                .items_center()
                .gap_4()
                .border_l_2()
                .border_color(gpui::rgb(if ix == selected {
                    theme.accent
                } else {
                    theme.panel
                }))
                .when(ix == selected, |line| line.bg(gpui::rgb(theme.raised)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(SharedString::from(row.label))
                        .child(
                            div()
                                .text_xs()
                                .text_color(gpui::rgb(theme.muted))
                                .child(SharedString::from(row.hint)),
                        ),
                )
                .child(control);
            body = body.child(line);
        }
        div()
            .absolute()
            .occlude()
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(640.0))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_5()
            .rounded_lg()
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
                    .child(div().flex_1().text_lg().child("Ayarlar"))
                    .child(
                        div()
                            .id("settings-close")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(gpui::rgb(theme.muted))
                            .hover(move |style| {
                                style
                                    .bg(gpui::rgb(theme.raised))
                                    .text_color(gpui::rgb(theme.text))
                            })
                            .child("Kapat · Esc")
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(SettingsEvent::Close)),
                            ),
                    ),
            )
            .child(body)
            .child(
                div()
                    .mt_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(gpui::rgb(theme.line))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child("↑ ↓ gezin · Enter / boşluk değiştir · ← → seçenek · Esc kapat")
                    .child(
                        "Dil sunucuları: ~/.config/raios-workspace-native/language-servers.json",
                    ),
            )
    }
}
