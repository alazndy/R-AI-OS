//! Now route rendering (approvals, active agent runs, blocked tasks, and system alerts).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::operations::OperationPanel;
use crate::app::store::Store;

/// Fixed height of the mission summary panel in the NOW route.
pub const NOW_SUMMARY_HEIGHT: u16 = 6;
/// Fixed height of the contextual actions panel in the NOW route.
pub const NOW_ACTIONS_HEIGHT: u16 = 7;

/// Renders the Now route panel view.
pub fn render_now_route(f: &mut Frame, area: Rect, store: &Store) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(NOW_SUMMARY_HEIGHT),
            Constraint::Min(6),
            Constraint::Length(NOW_ACTIONS_HEIGHT),
        ])
        .split(area);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let project = store.selected_project();
    let project_name = project
        .map(|project| project.name.as_str())
        .unwrap_or("No project selected");
    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "MISSION CONTROL  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                project_name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!(
            "{} pending approvals  ·  {} blockers  ·  {} active agents",
            store.snapshot.now.approvals.len(),
            store.snapshot.now.blocked_tasks.len(),
            store.snapshot.now.active_runs.len(),
        )),
        Line::from(Span::styled(
            "Space changes focus · Enter runs a selected next action · a/r resolve an approval",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" NOW · Operational Console ")
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    f.render_widget(summary, chunks[0]);

    let attention_items: Vec<ListItem> =
        if store.snapshot.now.approvals.is_empty() && store.snapshot.now.blocked_tasks.is_empty() {
            vec![ListItem::new(Span::styled(
                "Clear. No pending approvals or blocked tasks.",
                Style::default().fg(Color::Green),
            ))]
        } else {
            let mut items: Vec<ListItem> = store
                .snapshot
                .now
                .approvals
                .iter()
                .enumerate()
                .map(|(i, app)| {
                    let is_selected =
                        store.cursor == i && store.operations.panel == OperationPanel::Attention;
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
                            format!("[APPROVAL {} -> {}] ", app.origin_agent, app.target_agent),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            format!("[Risk:{:2}] ", app.score),
                            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(&app.title, Style::default().fg(Color::White)),
                        Span::styled(format!(" [{}]", app.kind), Style::default().fg(Color::Gray)),
                    ]))
                    .style(Style::default().bg(bg))
                })
                .collect();
            items.extend(store.snapshot.now.blocked_tasks.iter().enumerate().map(
                |(index, task)| {
                    let selected_index = store.snapshot.now.approvals.len() + index;
                    let is_selected = store.cursor == selected_index
                        && store.operations.panel == OperationPanel::Attention;
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            "[BLOCKED] ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} · ", task.project_name),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(&task.title, Style::default().fg(Color::LightRed)),
                    ]))
                    .style(Style::default().bg(if is_selected {
                        Color::DarkGray
                    } else {
                        Color::Reset
                    }))
                },
            ));
            items
        };

    let attention_block = Block::default()
        .borders(Borders::ALL)
        .title(" Attention Queue ")
        .border_style(Style::default().fg(
            if store.operations.panel == OperationPanel::Attention {
                Color::Green
            } else {
                Color::Yellow
            },
        ));

    f.render_widget(
        List::new(attention_items).block(attention_block),
        top_chunks[0],
    );

    let project_lines = if let Some(project) = project {
        let branch = project.git_branch.as_deref().unwrap_or("unknown");
        let memory = if project.has_memory {
            "ready"
        } else {
            "missing"
        };
        let project_runs: Vec<Line> = store
            .snapshot
            .now
            .active_runs
            .iter()
            .filter(|run| run.project_name == project.name)
            .take(3)
            .map(|run| {
                Line::from(format!(
                    "RUNNING {} · {}s",
                    run.agent_name, run.duration_secs
                ))
            })
            .collect();
        let mut lines = vec![
            Line::from(Span::styled(
                &project.name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "{} · branch:{} · {} dirty files",
                project.status, branch, project.dirty_files
            )),
            Line::from(format!("Memory: {}", memory)),
            Line::from(Span::styled(
                "Active agents:",
                Style::default().fg(Color::Cyan),
            )),
        ];
        if project_runs.is_empty() {
            lines.push(Line::from(Span::styled(
                "No active agent run.",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.extend(project_runs);
        }
        lines
    } else {
        vec![Line::from(Span::styled(
            "No registered project. Refresh when the control plane is connected.",
            Style::default().fg(Color::DarkGray),
        ))]
    };
    let project_block = Block::default()
        .borders(Borders::ALL)
        .title(" Selected Project ")
        .border_style(
            Style::default().fg(if store.operations.panel == OperationPanel::Project {
                Color::Green
            } else {
                Color::Cyan
            }),
        );
    f.render_widget(
        Paragraph::new(project_lines)
            .block(project_block)
            .wrap(Wrap { trim: true }),
        top_chunks[1],
    );

    let action_items: Vec<ListItem> = store
        .operations
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let selected = store.operations.panel == OperationPanel::Actions
                && store.operations.action_cursor == index;
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}  {}", if selected { "▶" } else { " " }, action.label),
                    Style::default()
                        .fg(if selected { Color::Green } else { Color::White })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!(" — {}", action.detail),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .style(Style::default().bg(if selected {
                Color::DarkGray
            } else {
                Color::Reset
            }))
        })
        .collect();
    let actions_block = Block::default()
        .borders(Borders::ALL)
        .title(" Next Actions · [Space] focus · [Enter] execute ")
        .border_style(
            Style::default().fg(if store.operations.panel == OperationPanel::Actions {
                Color::Green
            } else {
                Color::Yellow
            }),
        );
    f.render_widget(List::new(action_items).block(actions_block), chunks[2]);
}
