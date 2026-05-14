use crate::app::{App, MENU_ITEMS};
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

pub fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let has_files = matches!(i, 1 | 3 | 4 | 5);
            let arrow = if has_files { " ›" } else { "" };
            let full = format!("  {}{}  ", item, arrow);

            if i == app.ui.menu_cursor {
                if app.ui.right_panel_focus {
                    ListItem::new(Line::from(Span::styled(full, Style::new().fg(DIM))))
                } else {
                    ListItem::new(Line::from(Span::styled(
                        format!("▶ {}{}", item, arrow),
                        Style::new()
                            .fg(GREEN)
                            .bg(Color::Rgb(0, 30, 15))
                            .add_modifier(Modifier::BOLD),
                    )))
                }
            } else {
                ListItem::new(Line::from(Span::styled(full, Style::new().fg(MID))))
            }
        })
        .collect();

    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" MENU ", Style::new().fg(DIM)))
        .style(Style::new().bg(PANEL_BG));

    frame.render_widget(List::new(items).block(block), area);
}
