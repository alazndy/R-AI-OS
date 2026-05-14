use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

pub fn render_policies(frame: &mut Frame, area: Rect, app: &App) {
    let _ = app;
    let lines = vec![
        Line::from(Span::styled(
            " SECURITY POLICIES",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  • Auto-Allow Skills:     ", Style::new().fg(MID)),
            Span::styled("ENFORCED", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • Safe Read Operations:  ", Style::new().fg(MID)),
            Span::styled("ENFORCED", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • RLS (Day 0):           ", Style::new().fg(MID)),
            Span::styled("MANDATORY", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • API Keys Client-Side:  ", Style::new().fg(MID)),
            Span::styled("FORBIDDEN", Style::new().fg(RED).bold()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
