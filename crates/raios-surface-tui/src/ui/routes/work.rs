//! Work route rendering (projects list, tasks, artifacts, and Product Factory posture).

use raios_contracts::ProjectDto;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::store::{Store, WorkFocus};

/// Shared WORK-route rectangles used by both rendering and mouse hit testing.
/// Keeping this layout in one place prevents input coordinates drifting when
/// the visible panels change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkRouteLayout {
    pub(crate) projects: Rect,
    pub(crate) factory: Rect,
    pub(crate) tasks: Rect,
    pub(crate) task_actions: Rect,
    pub(crate) detail: Rect,
}

/// A bounded, visible action offered by the WORK task-action strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkTaskAction {
    Create,
    SetInProgress,
    SetBlocked,
    SetCompleted,
}

/// A bounded click target inside the personal-task composer overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskComposerMouseAction {
    DecreasePriority,
    IncreasePriority,
    Cancel,
    Submit,
}

/// Calculates the stable WORK-route panel layout for a content area.
pub(crate) fn work_route_layout(area: Rect) -> WorkRouteLayout {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Percentage(45),
            Constraint::Length(3),
            Constraint::Min(6),
        ])
        .split(columns[1]);

    WorkRouteLayout {
        projects: columns[0],
        factory: right[0],
        tasks: right[1],
        task_actions: right[2],
        detail: right[3],
    }
}

/// Maps a click in the visible task-action strip to a typed UI action.
pub(crate) fn work_task_action_at(
    layout: WorkRouteLayout,
    column: u16,
    row: u16,
) -> Option<WorkTaskAction> {
    let action_row = layout.task_actions.y.saturating_add(1);
    if row != action_row
        || column < layout.task_actions.x
        || column
            >= layout
                .task_actions
                .x
                .saturating_add(layout.task_actions.width)
    {
        return None;
    }

    let starts = [
        (1, 8, WorkTaskAction::Create),
        (11, 26, WorkTaskAction::SetInProgress),
        (27, 36, WorkTaskAction::SetBlocked),
        (37, 49, WorkTaskAction::SetCompleted),
    ];
    let relative_column = column.saturating_sub(layout.task_actions.x);
    starts.into_iter().find_map(|(start, end, action)| {
        ((start..end).contains(&relative_column)).then_some(action)
    })
}

/// Maps a click in the composer button row to a local draft action.
pub(crate) fn task_composer_action_at(
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TaskComposerMouseAction> {
    let popup = crate::ui::components::center_rect(76, 9, area);
    let button_row = popup.y.saturating_add(6);
    if row != button_row
        || popup.width < 28
        || column < popup.x
        || column >= popup.x.saturating_add(popup.width)
    {
        return None;
    }

    let relative_column = column.saturating_sub(popup.x);
    let cancel_start = popup.width.saturating_sub(22);
    let submit_start = popup.width.saturating_sub(11);
    [
        (1, 6, TaskComposerMouseAction::DecreasePriority),
        (7, 12, TaskComposerMouseAction::IncreasePriority),
        (
            cancel_start,
            cancel_start + 10,
            TaskComposerMouseAction::Cancel,
        ),
        (
            submit_start,
            submit_start + 10,
            TaskComposerMouseAction::Submit,
        ),
    ]
    .into_iter()
    .find_map(|(start, end, action)| ((start..end).contains(&relative_column)).then_some(action))
}

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

/// Renders the Work route panel view.
pub fn render_work_route(f: &mut Frame, area: Rect, store: &Store) {
    let layout = work_route_layout(area);

    // 1. Projects Sidebar
    let project_items: Vec<ListItem> = if store.snapshot.work.projects.is_empty() {
        vec![ListItem::new("No projects registered.")]
    } else {
        store
            .work_project_indices()
            .into_iter()
            .enumerate()
            .map(|(i, project_index)| {
                let p = &store.snapshot.work.projects[project_index];
                let is_selected = store.cursor == i && store.work_focus == WorkFocus::Projects;
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
        .title(format!(
            " Projects: Status, Git, Memory — sort: {} ",
            store.work_sort.label()
        ))
        .border_style(
            Style::default().fg(if store.work_focus == WorkFocus::Projects {
                Color::Green
            } else {
                Color::Cyan
            }),
        );

    let proj_list = List::new(project_items).block(proj_block);
    f.render_widget(proj_list, layout.projects);

    // Right detail column. Factory is a compact read-only overview inside the
    // existing WORK route, not a fifth top-level workflow.
    let factory = &store.snapshot.work.factory;
    let factory_state = if factory.enabled { "READY" } else { "DISABLED" };
    let factory_state_color = if factory.enabled {
        Color::Green
    } else {
        Color::Yellow
    };
    let summary_line = |index: usize, label: &str, value: u32| {
        let selected = store.work_focus == WorkFocus::Ocak && store.cursor == index;
        Line::from(Span::styled(
            format!("{} {}: {}", if selected { "▶" } else { " " }, label, value),
            if selected {
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            },
        ))
    };
    let factory_text = vec![
        Line::from(vec![
            Span::styled("State: ", Style::default().fg(Color::Gray)),
            Span::styled(factory_state, Style::default().fg(factory_state_color)),
            Span::styled(
                "  Read-only projection",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        summary_line(0, "Products", factory.product_count),
        summary_line(1, "Active cycles", factory.active_cycle_count),
        summary_line(2, "Pending changes", factory.pending_change_request_count),
        summary_line(3, "Open support", factory.open_support_items),
        summary_line(4, "Quality blockers", factory.blocking_quality_profiles),
        summary_line(5, "Release drafts", factory.release_drafts),
        Line::from(if factory.enabled {
            "Enter drafts the matching audited /ocak command"
        } else {
            "Enable in config before local Ocak commands are accepted"
        }),
    ];
    let factory_panel = Paragraph::new(factory_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Ocak ")
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(factory_panel, layout.factory);

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
                let bg = if store.work_focus == WorkFocus::Tasks && store.cursor == i {
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
                        format!(" [{}]", t.status),
                        Style::default().fg(project_status_color(&t.status)),
                    ),
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
        .border_style(
            Style::default().fg(if store.work_focus == WorkFocus::Tasks {
                Color::Green
            } else {
                Color::Blue
            }),
        );

    let tasks_list = List::new(task_items).block(tasks_block);
    f.render_widget(tasks_list, layout.tasks);

    let has_selected_task = store.snapshot.work.tasks.get(store.cursor).is_some();
    let status_style = if has_selected_task {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let task_actions = Paragraph::new(Line::from(vec![
        Span::styled("[ New ]   ", Style::default().fg(Color::Green)),
        Span::styled("[ In Progress ] ", status_style),
        Span::styled("[ Block ] ", status_style),
        Span::styled("[ Complete ]", status_style),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Task Actions ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(task_actions, layout.task_actions);

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
    f.render_widget(detail_p, layout.detail);

    if store.task_composer.is_open {
        render_task_composer(f, area, store);
    }
}

fn render_task_composer(f: &mut Frame, area: Rect, store: &Store) {
    let popup = crate::ui::components::center_rect(76, 9, area);
    let project_path = store
        .selected_project()
        .map(|project| project.path.as_str())
        .unwrap_or("No selected project");
    let composer = &store.task_composer;
    let lines = vec![
        Line::from(Span::styled(
            "Create a personal task for the selected project.",
            Style::default().fg(Color::Cyan),
        )),
        Line::from(vec![
            Span::styled("Title: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}█", composer.title),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!(
            "Priority: {}  keyboard [+/-] or mouse buttons",
            composer.priority
        )),
        Line::from(Span::styled(
            format!("Project: {project_path}"),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::styled("[ - ] [ + ]", Style::default().fg(Color::Cyan)),
            Span::raw(" ".repeat(popup.width.saturating_sub(34) as usize)),
            Span::styled("[ Cancel ] ", Style::default().fg(Color::Yellow)),
            Span::styled("[ Create ]", Style::default().fg(Color::Green)),
        ]),
        Line::from(Span::styled(
            "[Enter] create through control plane   [Esc] cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Create Personal Task ")
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: true }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        task_composer_action_at, work_route_layout, work_task_action_at, TaskComposerMouseAction,
        WorkTaskAction,
    };
    use ratatui::layout::Rect;

    #[test]
    fn task_action_hit_regions_follow_the_visible_button_order() {
        let layout = work_route_layout(Rect::new(0, 11, 120, 26));
        let row = layout.task_actions.y + 1;

        assert_eq!(
            work_task_action_at(layout, layout.task_actions.x + 2, row),
            Some(WorkTaskAction::Create)
        );
        assert_eq!(
            work_task_action_at(layout, layout.task_actions.x + 13, row),
            Some(WorkTaskAction::SetInProgress)
        );
        assert_eq!(
            work_task_action_at(layout, layout.task_actions.x + 28, row),
            Some(WorkTaskAction::SetBlocked)
        );
        assert_eq!(
            work_task_action_at(layout, layout.task_actions.x + 38, row),
            Some(WorkTaskAction::SetCompleted)
        );
    }

    #[test]
    fn composer_hit_regions_cover_only_explicit_buttons() {
        let area = Rect::new(0, 11, 120, 26);
        let popup = crate::ui::components::center_rect(76, 9, area);
        let row = popup.y + 6;

        assert_eq!(
            task_composer_action_at(area, popup.x + 2, row),
            Some(TaskComposerMouseAction::DecreasePriority)
        );
        assert_eq!(
            task_composer_action_at(area, popup.x + 8, row),
            Some(TaskComposerMouseAction::IncreasePriority)
        );
        assert_eq!(
            task_composer_action_at(area, popup.x + popup.width - 20, row),
            Some(TaskComposerMouseAction::Cancel)
        );
        assert_eq!(
            task_composer_action_at(area, popup.x + popup.width - 9, row),
            Some(TaskComposerMouseAction::Submit)
        );
    }
}
