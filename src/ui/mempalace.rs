use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render_mempalace_info(frame: &mut Frame, area: Rect, app: &App) {
    let count: usize = app.mempalace.rooms.iter().map(|r| r.projects.len()).sum();
    let hint = if count > 0 {
        format!(
            "  {} rooms · {} projects — /mempalace to open full view",
            app.mempalace.rooms.len(),
            count
        )
    } else {
        "  Loading workspace map...".into()
    };
    let lines = vec![
        Line::from(Span::styled(" MEMPALACE", Style::new().fg(MID).bold())),
        Line::from(""),
        Line::from(Span::styled(hint, Style::new().fg(DIM))),
        Line::from(""),
        Line::from(Span::styled(
            "  Select a file below →  or  /mempalace",
            Style::new().fg(DIM).italic(),
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

pub fn render_mempalace_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // ── Header ───────────────────────────────────────────────────────────────
    let total_projects: usize = app.mempalace.rooms.iter().map(|r| r.projects.len()).sum();
    let filter_hint = if app.mempalace.filter.is_empty() {
        Span::styled("", Style::new())
    } else {
        Span::styled(
            format!("  filter: {}", app.mempalace.filter),
            Style::new().fg(AMBER).bold(),
        )
    };

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  MEM", Style::new().fg(RED).bold()),
            Span::styled("PALACE", Style::new().fg(MID).bold()),
            Span::styled(
                format!(
                    "  {} rooms  ·  {} projects",
                    app.mempalace.rooms.len(),
                    total_projects
                ),
                Style::new().fg(DIM),
            ),
            filter_hint,
        ]),
        Line::from(Span::styled(
            "  Dev Ops workspace — filesystem scan",
            Style::new().fg(DIM),
        )),
    ])
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(header, header_area);

    // ── Content — two columns ────────────────────────────────────────────────
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
            .areas(content_area);

    // Build visible room/project rows, filtering by filter
    let filter = app.mempalace.filter.to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line_idx = 0;
    let mut current_line_idx = 0;

    for (ri, room) in app.mempalace.rooms.iter().enumerate() {
        let is_room_selected = ri == app.mempalace.room_cursor && app.mempalace.proj_cursor.is_none();
        if is_room_selected {
            selected_line_idx = current_line_idx;
        }
        let expanded = app.mempalace.expanded.get(ri).copied().unwrap_or(true);

        let proj_count = if filter.is_empty() {
            room.projects.len()
        } else {
            room.projects
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&filter))
                .count()
        };

        if !filter.is_empty() && proj_count == 0 {
            continue;
        }

        // Room header row
        let toggle = if expanded { "▾" } else { "▸" };
        let room_color = if is_room_selected { GREEN } else { CYAN };
        let prefix = if is_room_selected { "▶ " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::new().fg(GREEN).bold()),
            Span::styled(room.icon, Style::new()),
            Span::styled(format!(" {} ", toggle), Style::new().fg(DIM)),
            Span::styled(
                room.folder_name.as_str(),
                Style::new().fg(room_color).bold(),
            ),
            Span::styled(format!("  ({} projects)", proj_count), Style::new().fg(DIM)),
        ]));
        current_line_idx += 1;

        if !expanded {
            continue;
        }

        // Project rows
        for (pi, proj) in room.projects.iter().enumerate() {
            if !filter.is_empty() && !proj.name.to_lowercase().contains(&filter) {
                continue;
            }

            let is_proj_selected = ri == app.mempalace.room_cursor && app.mempalace.proj_cursor == Some(pi);
            if is_proj_selected {
                selected_line_idx = current_line_idx;
            }

            let mem_icon = if proj.has_memory {
                Span::styled("✓", Style::new().fg(GREEN))
            } else {
                Span::styled("✗", Style::new().fg(DIM))
            };

            let proj_color = if is_proj_selected { GREEN } else { MID };
            let proj_prefix = if is_proj_selected {
                "    ▶ "
            } else {
                "      "
            };
            let date_str = if proj.date == "—" {
                "".to_string()
            } else {
                format!("  {}", proj.date)
            };

            lines.push(Line::from(vec![
                Span::styled(proj_prefix, Style::new().fg(GREEN).bold()),
                mem_icon,
                Span::styled(" ", Style::new()),
                Span::styled(proj.name.as_str(), Style::new().fg(proj_color)),
                Span::styled(date_str, Style::new().fg(DIM)),
            ]));
            current_line_idx += 1;
        }
    }

    let visible_h = left_area.height as usize;
    let scroll = if selected_line_idx >= visible_h {
        selected_line_idx - visible_h + 1
    } else {
        0
    };

    let visible: Vec<Line> = lines.iter().skip(scroll).take(visible_h).cloned().collect();
    frame.render_widget(Paragraph::new(Text::from(visible)), left_area);

    // ── Right panel: selected project preview ────────────────────────────────
    let right_block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" Preview ", Style::new().fg(DIM)));
    let right_inner = right_block.inner(right_area);
    frame.render_widget(right_block, right_area);

    let preview_lines = build_preview(app);
    frame.render_widget(
        Paragraph::new(Text::from(preview_lines)).wrap(Wrap { trim: false }),
        right_inner,
    );

    // ── Footer ────────────────────────────────────────────────────────────────
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            " [↑↓] rooms  [→] projects  [←] back  [Enter] open / expand  [Space] toggle  [/] filter  [Esc] back",
            Style::new().fg(DIM),
        ),
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}
