use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

pub fn render_rules(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(
            " AI OS CONSTITUTION",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    for cat in &app.inventory.system_rules {
        lines.push(Line::from(vec![
            Span::styled(" ◈ ", Style::new().fg(GREEN)),
            Span::styled(cat.title, Style::new().fg(GREEN).bold()),
        ]));
        for rule in &cat.rules {
            lines.push(Line::from(vec![
                Span::styled("   • ", Style::new().fg(DIM)),
                Span::styled(*rule, Style::new().fg(MID)),
            ]));
        }
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
