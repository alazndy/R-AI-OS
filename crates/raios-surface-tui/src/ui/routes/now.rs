use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::store::Store;

pub fn render_now_route(f: &mut Frame, area: Rect, store: &Store) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // Pending Approvals & Blockers
            Constraint::Percentage(35), // Active Runs
            Constraint::Percentage(25), // System Alerts / Telemetry
        ])
        .split(area);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[0]);

    // 1. Pending Approvals
    let approval_items: Vec<ListItem> = if store.snapshot.now.approvals.is_empty() {
        vec![ListItem::new(Span::styled(
            "  ✓ No pending human approvals required.",
            Style::default().fg(Color::Green),
        ))]
    } else {
        store
            .snapshot
            .now
            .approvals
            .iter()
            .enumerate()
            .map(|(i, app)| {
                let is_selected = store.cursor == i && !store.right_panel_focus;
                let bg = if is_selected {
                    Color::DarkGray
                } else {
                    Color::Reset
                };

                let risk_color = if app.score > 70 {
                    Color::Red
                } else if app.score > 30 {
                    Color::Yellow
                } else {
                    Color::Cyan
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[Risk:{:2}] ", app.score),
                        Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&app.title, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" ({})", app.origin_agent),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let approvals_block = Block::default()
        .borders(Borders::ALL)
        .title(" 🚨 Pending Approvals (High Priority) ")
        .border_style(Style::default().fg(Color::Yellow));

    let approvals_list = List::new(approval_items).block(approvals_block);
    f.render_widget(approvals_list, top_chunks[0]);

    // 2. Blocked Tasks
    let blocked_items: Vec<ListItem> = if store.snapshot.now.blocked_tasks.is_empty() {
        vec![ListItem::new(Span::styled(
            "  ✓ No blocked tasks.",
            Style::default().fg(Color::Green),
        ))]
    } else {
        store
            .snapshot
            .now
            .blocked_tasks
            .iter()
            .map(|t| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", t.project_name), Style::default().fg(Color::DarkGray)),
                    Span::styled(&t.title, Style::default().fg(Color::LightRed)),
                ]))
            })
            .collect()
    };

    let blocked_block = Block::default()
        .borders(Borders::ALL)
        .title(" 🛑 Blocked Tasks ")
        .border_style(Style::default().fg(Color::Red));

    let blocked_list = List::new(blocked_items).block(blocked_block);
    f.render_widget(blocked_list, top_chunks[1]);

    // 3. Active Agent Runs
    let run_items: Vec<ListItem> = if store.snapshot.now.active_runs.is_empty() {
        vec![ListItem::new(Span::styled(
            "  • No agent runs actively executing.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        store
            .snapshot
            .now
            .active_runs
            .iter()
            .map(|r| {
                ListItem::new(Line::from(vec![
                    Span::styled("⏳ RUNNING ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{:<14} ", r.agent_name), Style::default().fg(Color::White)),
                    Span::styled(format!("proj: {:<16} ", r.project_name), Style::default().fg(Color::Gray)),
                    Span::styled(format!("({}s)", r.duration_secs), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect()
    };

    let runs_block = Block::default()
        .borders(Borders::ALL)
        .title(" 🏃 Active Agent Execution Runs ")
        .border_style(Style::default().fg(Color::Cyan));

    let runs_list = List::new(run_items).block(runs_block);
    f.render_widget(runs_list, chunks[1]);

    // 4. Alerts / Live Telemetry Logs
    let log_lines: Vec<Line> = store
        .logs
        .iter()
        .rev()
        .take(10)
        .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::Gray))))
        .collect();

    let alerts_block = Block::default()
        .borders(Borders::ALL)
        .title(" 🔔 System Live Telemetry & Alerts ")
        .border_style(Style::default().fg(Color::Blue));

    let alerts_p = Paragraph::new(log_lines)
        .block(alerts_block)
        .wrap(Wrap { trim: true });
    f.render_widget(alerts_p, chunks[2]);
}
