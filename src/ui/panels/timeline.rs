use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

pub fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::NONE)
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut items = Vec::new();
    for act in &app.timeline.activities {
        let style = match act.level {
            "Warning" => Style::new().fg(AMBER),
            "Error" => Style::new().fg(RED),
            _ => Style::new().fg(MID),
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", act.timestamp), Style::new().fg(DIM)),
            Span::styled(
                format!(" [{:<6}] ", act.source),
                Style::new().fg(CYAN).bold(),
            ),
            Span::styled(&act.message, style),
        ])));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  No recent activities recorded.",
            Style::new().fg(DIM).italic(),
        ))));
    }

    let list = List::new(items).block(
        Block::new()
            .title(" SYSTEM TIMELINE ")
            .title_style(Style::new().fg(DIM)),
    );
    frame.render_widget(list, inner);
}
