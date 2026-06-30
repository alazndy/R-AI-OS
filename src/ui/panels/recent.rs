use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

pub fn render_recent(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    let gradient = [
        Color::Rgb(255, 90, 0),    // Orange-Red
        Color::Rgb(255, 145, 0),   // Orange
        Color::Reset,              // Default (white/black depending on background)
        Color::Rgb(0, 200, 180),   // Turquoise
        Color::Rgb(0, 180, 255),   // Cyan
        Color::Rgb(30, 120, 255),  // Blue
    ];

    let banner_lines: Vec<Line> = BANNER
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let color = gradient[i % gradient.len()];
            Line::from(Span::styled(l, Style::new().fg(color)))
        })
        .collect();

    let mut lines = banner_lines;
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " RECENT PROJECTS & CHANGES",
        Style::new().fg(MID).bold(),
    )));
    lines.push(Line::from(""));

    for proj in &app.projects.recent {
        let git_tag = match proj.git_dirty {
            Some(true) => Span::styled(" ● dirty", Style::new().fg(AMBER)),
            Some(false) => Span::styled(" ○ clean", Style::new().fg(GREEN)),
            None => Span::styled("", Style::new()),
        };
        let branch_tag = proj
            .git_branch
            .as_deref()
            .map(|b| Span::styled(format!(" [{}]", b), Style::new().fg(DIM)))
            .unwrap_or_else(|| Span::styled("", Style::new()));

        lines.push(Line::from(vec![
            Span::styled(" 📁 ", Style::new().fg(CYAN)),
            Span::styled(proj.name.as_str(), Style::new().fg(CYAN).bold()),
            git_tag,
            branch_tag,
            Span::styled("  ", Style::new()),
            Span::styled(proj.rel_path.as_str(), Style::new().fg(DIM)),
        ]));
        for change in &proj.changes {
            lines.push(Line::from(vec![
                Span::styled("    • ", Style::new().fg(DIM)),
                Span::styled(change.as_str(), Style::new().fg(MID)),
            ]));
        }
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let [tasks_area, stats_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(10)]).areas(right);

    render_tasks_panel(frame, tasks_area, app);
    render_quick_stats(frame, stats_area, app);
}
