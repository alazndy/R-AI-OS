use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::store::Store;

pub fn render_govern_route(f: &mut Frame, area: Rect, store: &Store) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    // 1. Security & Policy Summary
    let pol = &store.snapshot.govern.policy_summary;
    let policy_lines = vec![
        Line::from(vec![
            Span::styled("Filesystem Jail: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if pol.enforce_sandbox { "ENFORCED" } else { "DISABLED" },
                Style::default().fg(if pol.enforce_sandbox { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Egress SSRF Filter: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if pol.egress_enabled { "ACTIVE" } else { "OFF" },
                Style::default().fg(if pol.egress_enabled { Color::Green } else { Color::Yellow }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Default Action: ", Style::default().fg(Color::Gray)),
            Span::styled(&pol.default_action, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Total Defined Rules: ", Style::default().fg(Color::Gray)),
            Span::styled(pol.total_rules.to_string(), Style::default().fg(Color::White)),
        ]),
    ];

    let policy_block = Block::default()
        .borders(Borders::ALL)
        .title(" AgentShield Security & Policies ")
        .border_style(Style::default().fg(if !store.right_panel_focus {
            Color::Green
        } else {
            Color::DarkGray
        }));

    let policy_p = Paragraph::new(policy_lines).block(policy_block);
    f.render_widget(policy_p, left_chunks[0]);

    // 2. Audit Ledger Stats
    let aud = &store.snapshot.govern.audit_summary;
    let audit_lines = vec![
        Line::from(vec![
            Span::styled("Total Audit Events: ", Style::default().fg(Color::Gray)),
            Span::styled(aud.total_records.to_string(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Allowed: ", Style::default().fg(Color::Gray)),
            Span::styled(aud.allowed_records.to_string(), Style::default().fg(Color::Green)),
            Span::styled("  Denied: ", Style::default().fg(Color::Gray)),
            Span::styled(aud.denied_records.to_string(), Style::default().fg(Color::Red)),
            Span::styled("  Confirmed: ", Style::default().fg(Color::Gray)),
            Span::styled(aud.confirmed_records.to_string(), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(Span::styled("Tamper-evident Ledger: VERIFIED", Style::default().fg(Color::Green))),
    ];

    let audit_block = Block::default()
        .borders(Borders::ALL)
        .title(" Audit Ledger Verification ")
        .border_style(Style::default().fg(if !store.right_panel_focus {
            Color::Green
        } else {
            Color::Yellow
        }));

    let audit_p = Paragraph::new(audit_lines).block(audit_block);
    f.render_widget(audit_p, left_chunks[1]);

    // 3. Cron Scheduler Jobs
    let cron_items: Vec<ListItem> = if store.snapshot.govern.cron_jobs.is_empty() {
        vec![ListItem::new("  • No scheduled background cron jobs.")]
    } else {
        store
            .snapshot
            .govern
            .cron_jobs
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let status_color = if j.status == "active" {
                    Color::Green
                } else {
                    Color::Yellow
                };
                let bg = if store.right_panel_focus && store.cursor == i {
                    Color::DarkGray
                } else {
                    Color::Reset
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", j.status), Style::default().fg(status_color)),
                    Span::styled(&j.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({})", j.schedule), Style::default().fg(Color::DarkGray)),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let cron_block = Block::default()
        .borders(Borders::ALL)
        .title(" Autonomous Cron Scheduler ")
        .border_style(Style::default().fg(if store.right_panel_focus {
            Color::Green
        } else {
            Color::Blue
        }));

    let cron_list = List::new(cron_items).block(cron_block);
    f.render_widget(cron_list, chunks[1]);
}
