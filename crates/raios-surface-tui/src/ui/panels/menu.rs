use raios_surface_tui::app::{route::Route, App};
use raios_surface_tui::ui::*;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Tabs},
    Frame,
};

pub fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Route::all()
        .iter()
        .enumerate()
        .map(|(idx, route)| {
            Line::from(vec![
                Span::styled(format!("{} ", idx + 1), Style::new().fg(DIM)),
                Span::styled(route.tab_label(), Style::new().fg(MID)),
            ])
        })
        .collect();

    let block = Block::new()
        .borders(Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" ROUTES ", Style::new().fg(DIM)))
        .style(Style::new().bg(PANEL_BG));

    let tabs = Tabs::new(titles)
        .block(block)
        .select(app.store.current_route.to_index())
        .highlight_style(
            Style::new()
                .fg(GREEN)
                .bg(Color::Rgb(0, 20, 45))
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" | ", Style::new().fg(DIM)));

    frame.render_widget(tabs, area);
}
