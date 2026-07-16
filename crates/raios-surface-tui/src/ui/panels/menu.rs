use raios_surface_tui::app::{route::Route, App};
use raios_surface_tui::ui::*;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

pub fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = Route::all()
        .iter()
        .map(|route| {
            let label = format!("{} {}", route.icon(), route.title());
            let full = format!("  {label}  ");

            if *route == app.store.current_route {
                if app.store.right_panel_focus {
                    ListItem::new(Line::from(Span::styled(full, Style::new().fg(DIM))))
                } else {
                    ListItem::new(Line::from(Span::styled(
                        format!("▶ {label}"),
                        Style::new()
                            .fg(GREEN)
                            .bg(Color::Rgb(0, 20, 45))
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
