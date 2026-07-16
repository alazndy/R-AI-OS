use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::store::Store;

pub fn render_explore_route(f: &mut Frame, area: Rect, store: &Store) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),      // Search bar
            Constraint::Percentage(50), // Search results / Traces
            Constraint::Percentage(50), // Live Log Replay
        ])
        .split(area);

    // 1. Search input bar
    let search_block = Block::default()
        .borders(Borders::ALL)
        .title(" 🔍 Search Code & Cortex (Trigram / Vector) [Press '/' to edit] ")
        .border_style(Style::default().fg(Color::Yellow));

    let search_p = Paragraph::new(format!("  Query: {}_", store.search_input)).block(search_block);
    f.render_widget(search_p, chunks[0]);

    // 2. Tool Traces Timeline & Results
    let trace_items: Vec<ListItem> = if store.snapshot.explore.recent_traces.is_empty() {
        vec![ListItem::new("  • No tool trace history recorded yet.")]
    } else {
        store
            .snapshot
            .explore
            .recent_traces
            .iter()
            .map(|t| {
                let status_color = if t.status == "SUCCESS" {
                    Color::Green
                } else {
                    Color::Red
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", t.status), Style::default().fg(status_color)),
                    Span::styled(&t.tool_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({}ms)", t.duration_ms), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect()
    };

    let traces_block = Block::default()
        .borders(Borders::ALL)
        .title(" ⏱️ Tool Execution Traces Timeline ")
        .border_style(Style::default().fg(Color::Cyan));

    let traces_list = List::new(trace_items).block(traces_block);
    f.render_widget(traces_list, chunks[1]);

    // 3. Live Logs Replay
    let log_items: Vec<ListItem> = if store.snapshot.explore.recent_logs.is_empty() {
        vec![ListItem::new("  • No logs available.")]
    } else {
        store
            .snapshot
            .explore
            .recent_logs
            .iter()
            .rev()
            .take(20)
            .map(|l| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", l.category), Style::default().fg(Color::Blue)),
                    Span::styled(&l.message, Style::default().fg(Color::Gray)),
                ]))
            })
            .collect()
    };

    let logs_block = Block::default()
        .borders(Borders::ALL)
        .title(" 📜 Daemon Log Stream Replay ")
        .border_style(Style::default().fg(Color::DarkGray));

    let logs_list = List::new(log_items).block(logs_block);
    f.render_widget(logs_list, chunks[2]);
}
