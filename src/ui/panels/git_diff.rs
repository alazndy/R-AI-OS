use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render_git_diff_view(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Clear the whole screen
    frame.render_widget(Block::new().bg(PANEL_BG), size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Diff content
            Constraint::Length(3), // Footer
        ])
        .split(size);

    // Header
    let current_idx = app.system.pending_change_cursor;
    let total = app.system.pending_file_changes.len();
    let path = app
        .system
        .pending_file_changes
        .get(current_idx)
        .map(|p| p.path.clone())
        .unwrap_or_default();

    let header_text = format!(
        " FILE CHANGE APPROVAL [{}/{}] — {}",
        current_idx + 1,
        total,
        path
    );
    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(AMBER)),
        )
        .style(Style::new().fg(AMBER).bold())
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(header, chunks[0]);

    // Diff Content
    let mut lines = Vec::new();
    for line in app.projects.git_diff_lines.iter().skip(app.projects.git_diff_scroll as usize) {
        if line.starts_with('+') {
            lines.push(Line::from(Span::styled(line, Style::new().fg(GREEN))));
        } else if line.starts_with('-') {
            lines.push(Line::from(Span::styled(line, Style::new().fg(RED))));
        } else {
            lines.push(Line::from(Span::styled(line, Style::new().fg(MID))));
        }
    }

    let diff = Paragraph::new(lines)
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .wrap(Wrap { trim: false });
    frame.render_widget(diff, chunks[1]);

    // Footer / Actions
    let footer_text = vec![
        Span::styled(" [Y] ", Style::new().fg(GREEN).bold()),
        Span::styled("Approve  ", Style::new().fg(MID)),
        Span::styled(" [N] ", Style::new().fg(RED).bold()),
        Span::styled("Reject  ", Style::new().fg(MID)),
        Span::styled(" [←/→] ", Style::new().fg(CYAN).bold()),
        Span::styled("Cycle Items  ", Style::new().fg(MID)),
        Span::styled(" [Esc] ", Style::new().fg(MID).bold()),
        Span::styled("Dashboard", Style::new().fg(MID)),
    ];
    let footer = Paragraph::new(Line::from(footer_text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(DIM)),
        )
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(footer, chunks[2]);
}
