use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Alignment, Rect},
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
            " ⚡ SYNCING... ",
            Style::new().fg(AMBER).add_modifier(Modifier::BOLD),
        )
    } else if app.system.memory_refresh_pending {
        Span::styled(" ↺ memory ", Style::new().fg(CYAN))
    } else if app.system.sync_status.is_some() {
        Span::styled(" ✓ SYNCED ", Style::new().fg(GREEN))
    } else {
        Span::styled("", Style::new())
    };
    let inbox_tag = if !app.system.pending_file_changes.is_empty() {
        Span::styled(
            format!(" 📥 {} PENDING ", app.system.pending_file_changes.len()),
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

    let mut header_spans = vec![
        title,
        version_tag,
        Span::raw(" "),
        sync_tag,
        Span::raw(" "),
        inbox_tag,
    ];
    header_spans.extend(port_spans);

    let header = Paragraph::new(Line::from(header_spans))
        .block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(DIM))
                .style(Style::new().bg(HEADER_BG)),
        )
        .alignment(Alignment::Left);
    frame.render_widget(header, area);
}
