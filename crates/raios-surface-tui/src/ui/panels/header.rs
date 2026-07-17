use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let version_tag = Span::styled(
        format!(" v{} (Stable) ", env!("CARGO_PKG_VERSION")),
        Style::new().fg(DIM),
    );
    let title = Span::styled(
        "  R-AI-OS — CORE SYSTEM  ",
        Style::new()
            .fg(GREEN)
            .bg(HEADER_BG)
            .add_modifier(Modifier::BOLD),
    );
    let sync_tag = if app.system.is_syncing {
        Span::styled(
            " SYNCING... ",
            Style::new().fg(AMBER).add_modifier(Modifier::BOLD),
        )
    } else if app.system.memory_refresh_pending {
        Span::styled(" ↺ memory ", Style::new().fg(CYAN))
    } else if app.system.sync_status.is_some() {
        Span::styled(" SYNCED ", Style::new().fg(GREEN))
    } else {
        Span::styled("", Style::new())
    };
    let inbox_tag = if !app.system.pending_file_changes.is_empty() {
        Span::styled(
            format!(" {} PENDING ", app.system.pending_file_changes.len()),
            Style::new().fg(AMBER).bold(),
        )
    } else {
        Span::raw("")
    };

    let mut port_spans = vec![];
    for port in &app.system.active_ports {
        port_spans.push(Span::styled(
            format!(" ●:{} ", port),
            Style::new().fg(GREEN),
        ));
    }

    let remote_tag = if app.is_remote {
        let host = app.remote_host.as_deref().unwrap_or("hub");
        Span::styled(
            format!(" ⇄ REMOTE:{} ", host),
            Style::new()
                .fg(CYAN)
                .bg(HEADER_BG)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    };

    let mut header_spans = vec![
        title,
        version_tag,
        remote_tag,
        Span::raw(" "),
        sync_tag,
        Span::raw(" "),
        inbox_tag,
    ];
    header_spans.extend(port_spans);

    let header_block = || {
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM))
            .style(Style::new().bg(HEADER_BG))
    };

    if area.height >= 8 {
        let [banner_area, status_area] =
            Layout::horizontal([Constraint::Length(52), Constraint::Min(0)]).areas(area);
        let banner_colors = [AMBER, AMBER, AMBER, CYAN, CYAN, CYAN];
        let banner_lines: Vec<Line> = BANNER
            .lines()
            .enumerate()
            .map(|(idx, line)| {
                Line::from(Span::styled(
                    line,
                    Style::new().fg(banner_colors[idx % banner_colors.len()]),
                ))
            })
            .collect();
        let banner = Paragraph::new(banner_lines)
            .block(header_block())
            .alignment(Alignment::Left);
        frame.render_widget(banner, banner_area);

        let status = Paragraph::new(vec![
            Line::from(Span::styled(
                "  CONTROL PLANE",
                Style::new().fg(GREEN).bold(),
            )),
            Line::from(header_spans),
            Line::from(Span::styled(
                "  1 NOW   2 WORK   3 EXPLORE   4 GOVERN",
                Style::new().fg(DIM),
            )),
        ])
        .block(header_block())
        .alignment(Alignment::Left);
        frame.render_widget(status, status_area);
    } else {
        let header = Paragraph::new(Line::from(header_spans))
            .block(header_block())
            .alignment(Alignment::Left);
        frame.render_widget(header, area);
    }
}
