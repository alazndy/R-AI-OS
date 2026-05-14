use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render_help(frame: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let title = Paragraph::new(Line::from(vec![Span::styled(
        "  SYSTEM DOCUMENTATION & COMMANDS  ",
        Style::new().fg(CYAN).bold(),
    )]))
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );

    frame.render_widget(title, chunks[0]);

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  NAVIGATION & SHORTCUTS",
            Style::new().fg(GREEN).bold(),
        )),
        Line::from(vec![
            Span::styled("    Ctrl + P      ", Style::new().fg(AMBER)),
            Span::raw("Neural Fuzzy Search (Global)"),
        ]),
        Line::from(vec![
            Span::styled("    /             ", Style::new().fg(AMBER)),
            Span::raw("Open Command Palette"),
        ]),
        Line::from(vec![
            Span::styled("    Tab           ", Style::new().fg(AMBER)),
            Span::raw("Switch between Menu and Content"),
        ]),
        Line::from(vec![
            Span::styled("    Arrows / HJ KL", Style::new().fg(AMBER)),
            Span::raw("Navigate through lists"),
        ]),
        Line::from(vec![
            Span::styled("    Enter         ", Style::new().fg(AMBER)),
            Span::raw("Select / Open file / Execute"),
        ]),
        Line::from(vec![
            Span::styled("    Esc           ", Style::new().fg(AMBER)),
            Span::raw("Back / Close Search / Cancel"),
        ]),
        Line::from(vec![
            Span::styled("    G             ", Style::new().fg(AMBER)),
            Span::raw("Run Graphify Analysis (in Detail view)"),
        ]),
        Line::from(vec![
            Span::styled("    R             ", Style::new().fg(AMBER)),
            Span::raw("View Graphify Report (in Detail view)"),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl + C      ", Style::new().fg(AMBER)),
            Span::raw("Hard Exit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  TERMINAL COMMANDS (starts with /)",
            Style::new().fg(GREEN).bold(),
        )),
        Line::from(vec![
            Span::styled("    /sync         ", Style::new().fg(AMBER)),
            Span::raw("Synchronize all agent policies"),
        ]),
        Line::from(vec![
            Span::styled("    /health       ", Style::new().fg(AMBER)),
            Span::raw("Run system-wide compliance audit"),
        ]),
        Line::from(vec![
            Span::styled("    /reindex      ", Style::new().fg(AMBER)),
            Span::raw("Rebuild neural search index"),
        ]),
        Line::from(vec![
            Span::styled("    /open [proj]  ", Style::new().fg(AMBER)),
            Span::raw("Jump to specific project detail"),
        ]),
        Line::from(vec![
            Span::styled("    /rules        ", Style::new().fg(AMBER)),
            Span::raw("View system constitution"),
        ]),
        Line::from(vec![
            Span::styled("    /memory       ", Style::new().fg(AMBER)),
            Span::raw("Access Global Memory"),
        ]),
        Line::from(vec![
            Span::styled("    /graphify     ", Style::new().fg(AMBER)),
            Span::raw("Generate codebase knowledge graph"),
        ]),
        Line::from(vec![
            Span::styled("    /q            ", Style::new().fg(AMBER)),
            Span::raw("Quit Application"),
        ]),
        Line::from(""),
        Line::from(Span::styled("  EDITOR KEYS", Style::new().fg(GREEN).bold())),
        Line::from(vec![
            Span::styled("    Ctrl + S      ", Style::new().fg(AMBER)),
            Span::raw("Save file in Editor"),
        ]),
        Line::from(vec![
            Span::styled("    Esc           ", Style::new().fg(AMBER)),
            Span::raw("Exit Editor without saving"),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(help_text).wrap(Wrap { trim: false }),
        chunks[1],
    );
}
