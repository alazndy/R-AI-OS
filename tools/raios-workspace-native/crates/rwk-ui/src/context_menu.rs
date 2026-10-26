//! The context menu: a small list opened at the pointer. It knows nothing about what its lines
//! do; it reports which line was chosen and the workbench maps that to an action. The keyboard
//! (arrows, Enter, Escape) is routed to it by the workbench, which owns the key handling of the
//! window.

use gpui::{
    div, px, Context, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use rwk_core::workbench::{Appearance, Theme};

/// Width of the menu panel.
pub const MENU_WIDTH: f32 = 252.0;
/// Height of one line.
const LINE_HEIGHT: f32 = 26.0;
/// Height of a rule.
const RULE_HEIGHT: f32 = 9.0;
/// Vertical padding inside the panel.
const PANEL_PAD: f32 = 4.0;
/// Space kept free between the panel and the window edge.
const EDGE: f32 = 6.0;

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A choice.
    Item {
        /// What the line says.
        label: SharedString,
        /// The shortcut when the line can run, the reason when it cannot.
        hint: Option<SharedString>,
        /// The line can be chosen.
        enabled: bool,
    },
    /// A thin rule between groups.
    Rule,
}

impl Line {
    /// A line the user can choose.
    pub fn item(label: impl Into<SharedString>, shortcut: Option<&'static str>) -> Self {
        Line::Item {
            label: label.into(),
            hint: shortcut.map(SharedString::from),
            enabled: true,
        }
    }

    /// A line that is shown but cannot be chosen, with the reason.
    pub fn disabled(label: impl Into<SharedString>, reason: Option<&'static str>) -> Self {
        Line::Item {
            label: label.into(),
            hint: reason.map(SharedString::from),
            enabled: false,
        }
    }

    fn selectable(&self) -> bool {
        matches!(self, Line::Item { enabled: true, .. })
    }

    fn height(&self) -> f32 {
        match self {
            Line::Item { .. } => LINE_HEIGHT,
            Line::Rule => RULE_HEIGHT,
        }
    }
}

/// What the menu asks of the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuEvent {
    /// The user chose the line at this index.
    Chosen(usize),
    /// The user dismissed the menu.
    Close,
}

impl EventEmitter<ContextMenuEvent> for ContextMenu {}

/// The menu view.
pub struct ContextMenu {
    lines: Vec<Line>,
    at: Point<Pixels>,
    highlighted: Option<usize>,
    theme: Theme,
}

impl ContextMenu {
    /// A menu with `lines` whose top-left corner is at `at`. The first line that can be chosen
    /// starts highlighted, so Enter does something sensible.
    pub fn new(lines: Vec<Line>, at: Point<Pixels>) -> Self {
        let highlighted = lines.iter().position(Line::selectable);
        Self {
            lines,
            at,
            highlighted,
            theme: Theme::of(Appearance::default()),
        }
    }

    /// Repaints the menu with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// The lines, top to bottom.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The highlighted line.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// Moves the highlight to the next (`forward`) or previous line that can be chosen,
    /// wrapping around at either end.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.lines.len();
        if count == 0 {
            return;
        }
        let start = self
            .highlighted
            .unwrap_or(if forward { count - 1 } else { 0 });
        let mut next = start;
        for _ in 0..count {
            next = if forward {
                (next + 1) % count
            } else {
                (next + count - 1) % count
            };
            if self.lines[next].selectable() {
                self.highlighted = Some(next);
                break;
            }
        }
        cx.notify();
    }

    /// Chooses line `index` when it can be chosen.
    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.lines.get(index).is_some_and(Line::selectable) {
            cx.emit(ContextMenuEvent::Chosen(index));
        }
    }

    /// Handles a key; true when the menu took it. Every key is taken: a menu is modal.
    pub fn key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "down" => self.step(true, cx),
            "up" => self.step(false, cx),
            "home" => {
                self.highlighted = self.lines.iter().position(Line::selectable);
                cx.notify();
            }
            "end" => {
                self.highlighted = self.lines.iter().rposition(Line::selectable);
                cx.notify();
            }
            "enter" => {
                if let Some(index) = self.highlighted {
                    self.choose(index, cx);
                }
            }
            "escape" => cx.emit(ContextMenuEvent::Close),
            _ => {}
        }
        true
    }

    /// Where the panel is painted: the pointer position, moved back inside the window when it
    /// would overflow, and never above or left of the edge margin.
    fn placement(&self, viewport: gpui::Size<Pixels>) -> Point<Pixels> {
        let height = PANEL_PAD * 2.0 + self.lines.iter().map(Line::height).sum::<f32>();
        let x = self
            .at
            .x
            .min(viewport.width - px(MENU_WIDTH + EDGE))
            .max(px(EDGE));
        let y = self
            .at
            .y
            .min(viewport.height - px(height + EDGE))
            .max(px(EDGE));
        gpui::point(x, y)
    }
}

impl Render for ContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let origin = self.placement(window.viewport_size());
        let lines = self
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| match line {
                Line::Rule => div()
                    .id(SharedString::from(format!("menu-rule-{index}")))
                    .h(px(RULE_HEIGHT))
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(gpui::rgb(theme.line)))
                    .into_any_element(),
                Line::Item {
                    label,
                    hint,
                    enabled,
                } => {
                    let enabled = *enabled;
                    let marked = enabled && self.highlighted == Some(index);
                    let row = div()
                        .id(SharedString::from(format!("menu-item-{index}")))
                        .h(px(LINE_HEIGHT))
                        .px_2()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .rounded_sm()
                        .text_sm()
                        .text_color(gpui::rgb(if enabled { theme.text } else { theme.muted }))
                        .when_marked(marked, theme)
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(label.clone()),
                        )
                        .children(hint.clone().map(|hint| {
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(gpui::rgb(theme.muted))
                                .child(hint)
                        }));
                    if enabled {
                        row.cursor_pointer()
                            .on_mouse_move(cx.listener(move |this, _, _window, cx| {
                                if this.highlighted != Some(index) {
                                    this.highlighted = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.choose(index, cx);
                            }))
                            .into_any_element()
                    } else {
                        row.into_any_element()
                    }
                }
            });
        // The backdrop catches every click outside the panel, so a click elsewhere only closes
        // the menu and never also presses what was underneath.
        div()
            .id("context-menu-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _window, cx| cx.emit(ContextMenuEvent::Close)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, _window, cx| cx.emit(ContextMenuEvent::Close)),
            )
            .child(
                div()
                    .id("context-menu")
                    .absolute()
                    .left(origin.x)
                    .top(origin.y)
                    .w(px(MENU_WIDTH))
                    .py(px(PANEL_PAD))
                    .px_1()
                    .rounded_md()
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .bg(gpui::rgb(theme.panel))
                    .border_1()
                    .border_color(gpui::rgb(if theme.lcars {
                        theme.accent
                    } else {
                        theme.line
                    }))
                    .text_color(gpui::rgb(theme.text))
                    // A click on the panel itself must not reach the backdrop behind it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .children(lines),
            )
    }
}

/// Styling of the highlighted line.
trait WhenMarked: Styled + Sized {
    fn when_marked(self, marked: bool, theme: Theme) -> Self {
        if marked {
            self.bg(gpui::rgb(if theme.lcars {
                theme.accent
            } else {
                theme.selected
            }))
        } else {
            self
        }
    }
}

impl<T: Styled + Sized> WhenMarked for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_items_are_selectable() {
        assert!(Line::item("Aç", None).selectable());
        assert!(!Line::disabled("Kaydet", Some("Açık dosya yok")).selectable());
        assert!(!Line::Rule.selectable());
    }
}
