use raios_contracts::ProjectDto;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::store::Store;

fn project_status_color(status: &str) -> Color {
    match status.to_ascii_lowercase().as_str() {
        "active" | "production" | "healthy" => Color::Green,
        "paused" | "maintenance" | "stale" => Color::Yellow,
        "blocked" | "error" | "archived" => Color::Red,
        _ => Color::Cyan,
    }
}

fn selected_project(store: &Store) -> Option<&ProjectDto> {
    let selected = store.selected_project_path.as_deref();
    store
        .snapshot
        .work
        .projects
        .iter()
        .find(|project| {
            Some(project.path.as_str()) == selected || Some(project.name.as_str()) == selected
        })
        .or_else(|| store.snapshot.work.projects.first())
}

pub fn render_work_route(f: &mut Frame, area: Rect, store: &Store) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // 1. Projects Sidebar
    let project_items: Vec<ListItem> = if store.snapshot.work.projects.is_empty() {
        vec![ListItem::new("No projects registered.")]
    } else {
        store
            .snapshot
            .work
            .projects
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_selected = store.cursor == i && !store.right_panel_focus;
                let bg = if is_selected {
                    Color::DarkGray
                } else {
                    Color::Reset
                };

                let dirty_icon = if p.dirty_files > 0 { "DIRTY" } else { "CLEAN" };
                let branch = p.git_branch.as_deref().unwrap_or("main");
                let memory_label = if p.has_memory {
                    "MEM:READY"
                } else {
                    "MEM:MISSING"
                };
                let memory_color = if p.has_memory {
                    Color::Green
                } else {
                    Color::Red
                };

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("[{}] ", p.status),
                            Style::default().fg(project_status_color(&p.status)),
                        ),
                        Span::styled(
                            &p.name,
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("  {} ", dirty_icon),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(
                            format!("{} ", memory_label),
                            Style::default().fg(memory_color),
                        ),
                        Span::styled(
                            format!("branch:{}", branch),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ])
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let proj_block = Block::default()
        .borders(Borders::ALL)
        .title(" Projects: Status, Git, Memory ")
        .border_style(Style::default().fg(if !store.right_panel_focus {
            Color::Green
        } else {
            Color::Cyan
        }));

    let proj_list = List::new(project_items).block(proj_block);
    f.render_widget(proj_list, chunks[0]);

    // Right detail column
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // 2. Active Tasks
    let task_items: Vec<ListItem> = if store.snapshot.work.tasks.is_empty() {
        vec![ListItem::new("No active tasks in control plane.")]
    } else {
        store
            .snapshot
            .work
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let bg = if store.right_panel_focus && store.cursor == i {
                    Color::DarkGray
                } else {
                    Color::Reset
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[P{}] ", t.priority),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(&t.title, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" -> {}", t.assignee.as_deref().unwrap_or("unassigned")),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let tasks_block = Block::default()
        .borders(Borders::ALL)
        .title(" Active Tasks & Assignments ")
        .border_style(Style::default().fg(if store.right_panel_focus {
            Color::Green
        } else {
            Color::Blue
        }));

    let tasks_list = List::new(task_items).block(tasks_block);
    f.render_widget(tasks_list, right_chunks[0]);

    // 3. Selected project's actual status and bounded memory.md preview.
    let detail_text = match selected_project(store) {
        Some(project) => {
            let branch = project.git_branch.as_deref().unwrap_or("unknown");
            let last_active = project.last_active.as_deref().unwrap_or("not recorded");
            let memory_state = if project.has_memory {
                "AVAILABLE"
            } else {
                "MISSING"
            };
            let memory_color = if project.has_memory {
                Color::Green
            } else {
                Color::Red
            };
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Project: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        &project.name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        &project.status,
                        Style::default().fg(project_status_color(&project.status)),
                    ),
                    Span::styled(
                        format!("  Branch: {}", branch),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("Memory: {}  Last activity: {}", memory_state, last_active),
                    Style::default().fg(memory_color),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "memory.md preview",
                    Style::default().fg(Color::Cyan),
                )),
            ];

            match project.memory_preview.as_deref() {
                Some(preview) => lines.extend(preview.lines().map(|line| {
                    Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(Color::White),
                    ))
                })),
                None if project.has_memory => lines.push(Line::from(Span::styled(
                    "  Memory file is empty or cannot be previewed.",
                    Style::default().fg(Color::DarkGray),
                ))),
                None => lines.push(Line::from(Span::styled(
                    "  No memory.md found for this project.",
                    Style::default().fg(Color::Red),
                ))),
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "Select a project to inspect its status and memory.",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    let detail_block = Block::default()
        .borders(Borders::ALL)
        .title(" Selected Project: Status & Memory ")
        .border_style(Style::default().fg(Color::DarkGray));

    let detail_p = Paragraph::new(detail_text)
        .block(detail_block)
        .wrap(Wrap { trim: true });
    f.render_widget(detail_p, right_chunks[1]);
}
