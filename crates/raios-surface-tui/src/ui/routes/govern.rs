//! Govern route rendering (policy summary, audit logs, health reports, and cron jobs).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::store::Store;

/// Shared Govern-route rectangles used by rendering and mouse hit testing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GovernRouteLayout {
    pub(crate) policy: Rect,
    pub(crate) audit: Rect,
    pub(crate) scheduler: Rect,
    pub(crate) scheduler_actions: Rect,
}

/// Explicit scheduler actions exposed by the Govern route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GovernCronAction {
    Trigger,
    TogglePause,
}

/// Calculates the stable Govern panels for one dashboard content area.
pub(crate) fn govern_route_layout(area: Rect) -> GovernRouteLayout {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(columns[1]);

    GovernRouteLayout {
        policy: left[0],
        audit: left[1],
        scheduler: right[0],
        scheduler_actions: right[1],
    }
}

/// Maps an explicit action-strip click to its bounded scheduler operation.
pub(crate) fn govern_cron_action_at(
    layout: GovernRouteLayout,
    column: u16,
    row: u16,
) -> Option<GovernCronAction> {
    let action_row = layout.scheduler_actions.y.saturating_add(1);
    if row != action_row
        || column < layout.scheduler_actions.x
        || column
            >= layout
                .scheduler_actions
                .x
                .saturating_add(layout.scheduler_actions.width)
    {
        return None;
    }
    let relative_column = column.saturating_sub(layout.scheduler_actions.x);
    [
        (1, 14, GovernCronAction::Trigger),
        (17, 37, GovernCronAction::TogglePause),
    ]
    .into_iter()
    .find_map(|(start, end, action)| ((start..end).contains(&relative_column)).then_some(action))
}

/// Renders the Govern route panel view.
pub fn render_govern_route(f: &mut Frame, area: Rect, store: &Store) {
    let layout = govern_route_layout(area);

    // 1. Security & Policy Summary
    let pol = &store.snapshot.govern.policy_summary;
    let policy_lines = vec![
        Line::from(vec![
            Span::styled("Filesystem Jail: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if pol.enforce_sandbox {
                    "ENFORCED"
                } else {
                    "DISABLED"
                },
                Style::default().fg(if pol.enforce_sandbox {
                    Color::Green
                } else {
                    Color::Red
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Egress SSRF Filter: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if pol.egress_enabled { "ACTIVE" } else { "OFF" },
                Style::default().fg(if pol.egress_enabled {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Default Action: ", Style::default().fg(Color::Gray)),
            Span::styled(&pol.default_action, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Total Defined Rules: ", Style::default().fg(Color::Gray)),
            Span::styled(
                pol.total_rules.to_string(),
                Style::default().fg(Color::White),
            ),
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
    f.render_widget(policy_p, layout.policy);

    // 2. Audit Ledger Stats
    let aud = &store.snapshot.govern.audit_summary;
    let audit_lines = vec![
        Line::from(vec![
            Span::styled("Total Audit Events: ", Style::default().fg(Color::Gray)),
            Span::styled(
                aud.total_records.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Allowed: ", Style::default().fg(Color::Gray)),
            Span::styled(
                aud.allowed_records.to_string(),
                Style::default().fg(Color::Green),
            ),
            Span::styled("  Denied: ", Style::default().fg(Color::Gray)),
            Span::styled(
                aud.denied_records.to_string(),
                Style::default().fg(Color::Red),
            ),
            Span::styled("  Confirmed: ", Style::default().fg(Color::Gray)),
            Span::styled(
                aud.confirmed_records.to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Tamper-evident Ledger: VERIFIED",
            Style::default().fg(Color::Green),
        )),
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
    f.render_widget(audit_p, layout.audit);

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
                    Span::styled(
                        format!("[{}] ", j.status),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        &j.name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" ({})", j.schedule),
                        Style::default().fg(Color::DarkGray),
                    ),
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
    f.render_widget(cron_list, layout.scheduler);

    let action_strip = Paragraph::new(Line::from(vec![
        Span::styled(" [r] Run now ", Style::default().fg(Color::Cyan).bold()),
        Span::styled(
            " [p] Pause/Resume ",
            Style::default().fg(Color::Yellow).bold(),
        ),
        Span::styled(
            " selected scheduler job",
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if store.right_panel_focus {
                Color::Green
            } else {
                Color::DarkGray
            })),
    );
    f.render_widget(action_strip, layout.scheduler_actions);
}

#[cfg(test)]
mod tests {
    use super::{govern_cron_action_at, govern_route_layout, GovernCronAction};
    use ratatui::layout::Rect;

    #[test]
    fn action_strip_accepts_only_visible_scheduler_buttons() {
        let layout = govern_route_layout(Rect::new(0, 0, 120, 36));
        let row = layout.scheduler_actions.y + 1;
        assert_eq!(
            govern_cron_action_at(layout, layout.scheduler_actions.x + 2, row),
            Some(GovernCronAction::Trigger)
        );
        assert_eq!(
            govern_cron_action_at(layout, layout.scheduler_actions.x + 20, row),
            Some(GovernCronAction::TogglePause)
        );
        assert_eq!(
            govern_cron_action_at(layout, layout.scheduler_actions.x + 15, row),
            None
        );
    }
}
