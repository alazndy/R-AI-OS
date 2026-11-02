//! Pieces shared by the overlay panels (palette, pickers, prompts, views).

use gpui::{div, InteractiveElement, ParentElement, Styled};
use rwk_core::workbench::Theme;

/// The "×" in the top-right corner of an overlay panel. The panel's root must be positioned
/// (`absolute`); the caller attaches the click handler.
pub fn close_button(theme: Theme, id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .absolute()
        .top_1()
        .right_2()
        .px_2()
        .rounded_sm()
        .cursor_pointer()
        .text_color(gpui::rgb(theme.muted))
        .hover(move |style| {
            style
                .bg(gpui::rgb(theme.raised))
                .text_color(gpui::rgb(theme.text))
        })
        .child("×")
}
