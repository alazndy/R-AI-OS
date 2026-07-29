//! Help reference panel rendering.

use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Full-screen help view (AppState::HelpView) — opened with `?` or `/help`.
pub fn render_help_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(frame, header_area, app);
    render_help(frame, body_area, app);

    let footer = Paragraph::new(Span::styled(
        "  press any key to return",
        Style::new().fg(DIM).italic(),
    ));
    frame.render_widget(footer, footer_area);
}

fn key_line(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("    {:<14} ", key), Style::new().fg(AMBER)),
        Span::raw(desc),
    ])
}

fn section(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", title),
        Style::new().fg(GREEN).bold(),
    ))
}

/// Renders the keyboard shortcuts and commands reference help panel.
pub fn render_help(frame: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let filter_text = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  Help Reference", Style::new().fg(CYAN).bold()),
            Span::styled(" — press ", Style::new().fg(DIM)),
            Span::styled("Esc", Style::new().fg(AMBER)),
            Span::styled(" or ", Style::new().fg(DIM)),
            Span::styled("q", Style::new().fg(AMBER)),
            Span::styled(" to close", Style::new().fg(DIM)),
        ]),
        Line::from(Span::styled(
            "  Keyboard Shortcuts & Available Commands",
            Style::new().fg(DIM),
        )),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(filter_text, chunks[0]);

    let text = vec![
        Line::from(""),
        section("NAVIGATION & SEARCH"),
        key_line("/", "Command Palette (type to filter)"),
        key_line("Ctrl+P", "Quick Search modal"),
        key_line("1..4", "Jump to route (1:NOW 2:WORK 3:EXPLORE 4:GOVERN)"),
        key_line("Tab / Shift+Tab", "Next / previous section"),
        key_line("j / k or ↓ / ↑", "Move cursor down / up"),
        key_line("h / l or ← / →", "Move between panels / tabs"),
        key_line("Enter", "Select item / open detail view"),
        key_line("Esc / q", "Back / close view"),
        Line::from(""),
        section("PROJECTS & ACTIONS"),
        key_line("Ctrl+O", "Open project selector"),
        key_line("s", "Sort projects (Activity / Name / Health)"),
        key_line("e", "Edit active project file"),
        key_line("v", "View active project file"),
        key_line("r", "Re-run Sentinel check on active project"),
        key_line("g", "Generate Knowledge Graph (Graphify)"),
        Line::from(""),
        section("COMMAND PALETTE QUICK REF"),
        key_line("/now", "Now route (approvals, blockers, runs)"),
        key_line("/work", "Work route (projects, tasks, factory)"),
        key_line("/explore", "Explore route (search, traces, logs)"),
        key_line("/govern", "Govern route (policies, audit, jobs)"),
        key_line("/ocak", "Ocak product lifecycle manager"),
        key_line("/discover", "Rescan Dev Ops for projects"),
        key_line("/sync", "Sync all agents with MASTER.md"),
        key_line("/memo <text>", "Record quick project note"),
        key_line("/task add <t>", "Add new task"),
        key_line("/quit", "Exit R-AI-OS TUI"),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::new().style(Style::new().bg(PANEL_BG)))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, chunks[1]);
}
