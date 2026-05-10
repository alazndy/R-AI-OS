use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table, TableState, Wrap},
};
use crate::app::{App, state::SortMode};
use crate::ui::*;


pub fn render_projects(frame: &mut Frame, area: Rect, app: &App) {
    let indices = app.sorted_project_indices();
    let total = indices.len();
    
    let [title_area, table_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .areas(area);

    let title = Line::from(vec![
        Span::styled(" ALL PROJECTS", Style::new().fg(MID).bold()),
        Span::styled(format!("  ({} total)", total), Style::new().fg(DIM)),
        Span::styled("  sort: ", Style::new().fg(DIM)),
        Span::styled(app.project_sort.label(), Style::new().fg(AMBER).bold()),
        Span::styled(
            if app.right_panel_focus { "  [↑↓] navigate  [Enter] open  [s] cycle sort" }
            else { "  [→] focus  [/open <name>] jump" },
            Style::new().fg(DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), title_area);

    if indices.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  entities.json not found or empty", Style::new().fg(DIM).italic())),
            Line::from(Span::styled("  Expected: Dev Ops/entities.json", Style::new().fg(DIM))),
        ]);
        frame.render_widget(msg, table_area);
        return;
    }

    let header = Row::new(vec![
        "  Name",
        "V",
        "Status",
        "Category",
        "Health",
        "Dirty",
    ])
    .style(Style::new().fg(DIM).bold())
    .bottom_margin(1);

    let rows: Vec<Row> = indices.iter().map(|&orig_i| {
        let proj = &app.projects[orig_i];
        let sc = project_status_color(&proj.status);
        let cat = proj.category.replace('_', " ");
        
        let has_vault = app.vault_projects.contains(&proj.name);
        let vault_tag = if has_vault {
            Span::styled("V", Style::new().fg(AMBER).bold())
        } else {
            Span::styled("-", Style::new().fg(DIM))
        };

        // Use cached health report if available
        let health = app.health_report.iter().find(|h| h.name == proj.name);
        let grade = health.map(|h| h.compliance_grade.as_str()).unwrap_or("-");
        let gc = match grade {
            "A" => GREEN,
            "B" => CYAN,
            "C" => AMBER,
            "D" | "F" => RED,
            _ => DIM,
        };

        let dirty_status = health.and_then(|h| h.git_dirty);
        let dirty = match dirty_status {
            Some(true) => Span::styled("DIRTY", Style::new().fg(RED).bold()),
            Some(false) => Span::styled("clean", Style::new().fg(DIM)),
            None => Span::styled("?", Style::new().fg(DIM)),
        };

        Row::new(vec![
            Text::from(proj.name.clone()),
            Text::from(vault_tag),
            Text::from(Span::styled(proj.status.clone(), Style::new().fg(sc))),
            Text::from(cat),
            Text::from(Span::styled(grade, Style::new().fg(gc).bold())),
            Text::from(dirty),
        ])
    }).collect();

    let widths = [
        Constraint::Percentage(25),
        Constraint::Length(3),
        Constraint::Percentage(15),
        Constraint::Percentage(25),
        Constraint::Percentage(10),
        Constraint::Percentage(15),
    ];

    let mut state = TableState::default().with_selected(Some(app.project_cursor));

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::NONE))
        .row_highlight_style(Style::default().bg(HEADER_BG).fg(GREEN).bold())
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, table_area, &mut state);
}

pub fn render_project_detail(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let Some(ref proj) = app.active_project else { return };

    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let sc = project_status_color(&proj.status);
    let cat = proj.category.replace('_', " ");
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(proj.name.as_str(), Style::new().fg(GREEN).bold()),
            Span::styled(format!("  [{}]", proj.status), Style::new().fg(sc).bold()),
            Span::styled(format!("  {}", cat), Style::new().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(
                proj.github.as_deref().unwrap_or("(no GitHub link)"),
                Style::new().fg(CYAN),
            ),
        ]),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(header, header_area);

    // Left / Right split
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Percentage(62),
        Constraint::Percentage(38),
    ])
    .areas(main_area);

    // ── Left: memory.md ──────────────────────────────────────────────────────
    let mem_color = if !app.project_panel_focus { GREEN } else { DIM };
    let mem_block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(Style::new().fg(mem_color))
        .title(Span::styled(" memory.md ", Style::new().fg(mem_color)));
    let mem_inner = mem_block.inner(left_area);
    frame.render_widget(mem_block, left_area);

    let scroll = app.project_memory_scroll as usize;
    let visible = mem_inner.height as usize;
    let mut in_block = false;
    for line in app.project_memory_lines.iter().take(scroll) {
        update_code_block_state(line, "md", &mut in_block);
    }
    let mem_lines: Vec<Line> = if app.project_memory_lines.is_empty() {
        vec![Line::from(Span::styled("  Loading...", Style::new().fg(DIM)))]
    } else {
        let mut out = Vec::new();
        for line in app.project_memory_lines.iter().skip(scroll).take(visible) {
            out.push(Line::from(highlight_line(line, &mut in_block, "md")));
        }
        out
    };
    frame.render_widget(Paragraph::new(Text::from(mem_lines)), mem_inner);

    // ── Right: git log + stats ────────────────────────────────────────────────
    let git_color = if app.project_panel_focus { GREEN } else { DIM };
    let [git_area, stats_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(6),
    ])
    .areas(right_area);

    let git_block = Block::new()
        .borders(Borders::NONE)
        .title(Span::styled(" git log ", Style::new().fg(git_color)));
    let git_inner = git_block.inner(git_area);
    frame.render_widget(git_block, git_area);

    let git_lines: Vec<Line> = if app.project_git_log.is_empty() {
        vec![Line::from(Span::styled("  Loading...", Style::new().fg(DIM)))]
    } else {
        app.project_git_log
            .iter()
            .map(|entry| {
                if let Some(sp) = entry.find(' ') {
                    Line::from(vec![
                        Span::styled(format!(" {}", &entry[..sp]), Style::new().fg(AMBER)),
                        Span::styled(entry[sp..].to_string(), Style::new().fg(MID)),
                    ])
                } else {
                    Line::from(Span::styled(entry.as_str(), Style::new().fg(DIM)))
                }
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(Text::from(git_lines)), git_inner);

    // Stats
    let graph_status = if let Some(h) = app.health_report.iter().find(|h| h.name == proj.name) {
        if h.graphify_done {
            Span::styled("ready", Style::new().fg(GREEN))
        } else {
            Span::styled("pending", Style::new().fg(AMBER))
        }
    } else {
        Span::styled("unknown", Style::new().fg(DIM))
    };

    let mut version_text = proj.version.clone().unwrap_or_else(|| "0.0.0".to_string());
    if let Some(ref nick) = proj.version_nickname {
        version_text.push_str(&format!(" ({})", nick));
    }

    let stats = vec![
        Line::from(vec![
            Span::styled(" Status  ", Style::new().fg(DIM)),
            Span::styled(proj.status.as_str(), Style::new().fg(sc).bold()),
            Span::styled("  Ver ", Style::new().fg(DIM)),
            Span::styled(version_text, Style::new().fg(CYAN).bold()),
        ]),
        Line::from(vec![
            Span::styled(" Cat     ", Style::new().fg(DIM)),
            Span::styled(cat, Style::new().fg(MID)),
        ]),
        Line::from(vec![
            Span::styled(" Graph   ", Style::new().fg(DIM)),
            graph_status,
        ]),
        Line::from(vec![
            Span::styled(" GitHub  ", Style::new().fg(DIM)),
            Span::styled(format!("⭐ {}  ", proj.stars.unwrap_or(0)), Style::new().fg(AMBER)),
            Span::styled(proj.last_commit.as_deref().unwrap_or("never"), Style::new().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled(" Path    ", Style::new().fg(DIM)),
            Span::styled(
                proj.local_path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                Style::new().fg(DIM),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(stats)), stats_area);

    // Footer
    let total = app.project_memory_lines.len();
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" Ln {}/{}", scroll + 1, total),
            Style::new().fg(DIM),
        ),
        Span::styled(
            "   [↑↓] scroll  [Tab] switch panel  [G] Graphify  [R] report  [e] edit memory  [Esc] back",
            Style::new().fg(DIM),
        ),
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

pub fn render_graph_report(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let proj_name = app.active_project.as_ref().map(|p| p.name.as_str()).unwrap_or("Unknown");
    let header = Block::bordered()
        .title(format!(" Graphify Report: {} ", proj_name))
        .border_style(Style::new().fg(CYAN))
        .border_type(BorderType::Rounded);
    frame.render_widget(header, rows[0]);

    let display_h = rows[1].height as usize;
    let lines: Vec<Line> = app.graph_report_lines
        .iter()
        .skip(app.graph_report_scroll as usize)
        .take(display_h)
        .map(|l| Line::from(highlight_markdown(l)))
        .collect();

    let content = Paragraph::new(lines)
        .block(Block::new().bg(PANEL_BG))
        .wrap(Wrap { trim: false });
    frame.render_widget(content, rows[1]);

    let footer = Paragraph::new(" [ESC/Q] Back  [UP/DOWN/J/K] Scroll  [PAGEUP/PAGEDOWN] Fast Scroll ")
        .style(Style::new().fg(DIM).bg(HEADER_BG));
    frame.render_widget(footer, rows[2]);
}


