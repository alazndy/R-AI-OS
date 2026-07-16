use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::store::Store;

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

                let dirty_icon = if p.dirty_files > 0 { "✏️" } else { "✓" };
                let branch = p.git_branch.as_deref().unwrap_or("main");

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", dirty_icon), Style::default().fg(Color::Yellow)),
                    Span::styled(&p.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({})", branch), Style::default().fg(Color::DarkGray)),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let proj_block = Block::default()
        .borders(Borders::ALL)
        .title(" 📁 Registered Workspace Projects ")
        .border_style(Style::default().fg(Color::Cyan));

    let proj_list = List::new(project_items).block(proj_block);
    f.render_widget(proj_list, chunks[0]);

    // Right detail column
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // 2. Active Tasks
    let task_items: Vec<ListItem> = if store.snapshot.work.tasks.is_empty() {
        vec![ListItem::new("  • No active tasks in control plane.")]
    } else {
        store
            .snapshot
            .work
            .tasks
            .iter()
            .map(|t| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[P{}] ", t.priority), Style::default().fg(Color::Yellow)),
                    Span::styled(&t.title, Style::default().fg(Color::White)),
                    Span::styled(
                        format!(" -> {}", t.assignee.as_deref().unwrap_or("unassigned")),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
            })
            .collect()
    };

    let tasks_block = Block::default()
        .borders(Borders::ALL)
        .title(" 📋 Active Tasks & Assignments ")
        .border_style(Style::default().fg(Color::Blue));

    let tasks_list = List::new(task_items).block(tasks_block);
    f.render_widget(tasks_list, right_chunks[0]);

    // 3. Artifacts / Overview detail
    let selected_project_name = store
        .snapshot
        .work
        .projects
        .get(store.cursor)
        .map(|p| p.name.as_str())
        .unwrap_or("None");

    let detail_text = vec![
        Line::from(vec![
            Span::styled("Selected Project: ", Style::default().fg(Color::Gray)),
            Span::styled(selected_project_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("Recent Code Artifacts & Handoff Notes:", Style::default().fg(Color::Cyan))),
        Line::from("  • AGENT_CONSTITUTION.md (Updated)"),
        Line::from("  • SIGMAP.md (Generated)"),
        Line::from("  • raios-contracts (Active Crate Migration)"),
    ];

    let detail_block = Block::default()
        .borders(Borders::ALL)
        .title(" 📄 Project Overview & Artifacts ")
        .border_style(Style::default().fg(Color::DarkGray));

    let detail_p = Paragraph::new(detail_text).block(detail_block);
    f.render_widget(detail_p, right_chunks[1]);
}
