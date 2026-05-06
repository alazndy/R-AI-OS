use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};
use crate::app::{App, MENU_ITEMS};
use crate::ui::*;


pub fn render_dashboard(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let [header_area, main_area, launcher_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(area);

    let [menu_area, content_area] = Layout::horizontal([
        Constraint::Length(28),
        Constraint::Min(0),
    ])
    .areas(main_area);

    render_header(frame, header_area, app);
    render_menu(frame, menu_area, app);
    render_content(frame, content_area, app);
    render_launcher(frame, launcher_area, app);
}

pub fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let version_tag = Span::styled(
        format!(" v{} (Stable) ", env!("CARGO_PKG_VERSION")), 
        Style::new().fg(DIM)
    );
    let title = Span::styled(
        "  R-AI-OS — CORE SYSTEM  ",
        Style::new()
            .fg(GREEN)
            .bg(HEADER_BG)
            .add_modifier(Modifier::BOLD),
    );
    let sync_tag = if app.is_syncing {
        Span::styled(" ⚡ SYNCING... ", Style::new().fg(AMBER).add_modifier(Modifier::BOLD))
    } else if app.memory_refresh_pending {
        Span::styled(" ↺ memory ", Style::new().fg(CYAN))
    } else if app.sync_status.is_some() {
        Span::styled(" ✓ SYNCED ", Style::new().fg(GREEN))
    } else {
        Span::styled("", Style::new())
    };
    let inbox_tag = if !app.pending_file_changes.is_empty() {
        Span::styled(format!(" 📥 {} PENDING ", app.pending_file_changes.len()), Style::new().fg(AMBER).bold())
    } else {
        Span::raw("")
    };

    let mut port_spans = vec![];
    for port in &app.active_ports {
        port_spans.push(Span::styled(format!(" ●:{} ", port), Style::new().fg(GREEN)));
    }

    let mut header_spans = vec![
        title,
        version_tag,
        Span::raw(" "),
        sync_tag,
        Span::raw(" "),
        inbox_tag,
    ];
    header_spans.extend(port_spans);

    let header = Paragraph::new(Line::from(header_spans))
        .block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(DIM))
                .style(Style::new().bg(HEADER_BG)),
        )
        .alignment(Alignment::Left);
    frame.render_widget(header, area);
}

pub fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let has_files = matches!(i, 1 | 3 | 4 | 5);
            let arrow = if has_files { " ›" } else { "" };
            let full = format!("  {}{}  ", item, arrow);

            if i == app.menu_cursor {
                if app.right_panel_focus {
                    ListItem::new(Line::from(Span::styled(
                        full,
                        Style::new().fg(DIM),
                    )))
                } else {
                    ListItem::new(Line::from(Span::styled(
                        format!("▶ {}{}", item, arrow),
                        Style::new()
                            .fg(GREEN)
                            .bg(Color::Rgb(0, 30, 15))
                            .add_modifier(Modifier::BOLD),
                    )))
                }
            } else {
                ListItem::new(Line::from(Span::styled(full, Style::new().fg(MID))))
            }
        })
        .collect();

    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" MENU ", Style::new().fg(DIM)))
        .style(Style::new().bg(PANEL_BG));

    frame.render_widget(List::new(items).block(block), area);
}

pub fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    let menu_files = app.current_menu_files();

    if menu_files.is_empty() {
        render_content_body(frame, area, app);
        return;
    }

    let file_count = menu_files.len() as u16;
    let files_height = (file_count + 3).min(area.height / 2);

    let [body_area, files_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(files_height)]).areas(area);

    render_content_body(frame, body_area, app);
    render_file_panel(frame, files_area, app, &menu_files);
}

pub fn render_content_body(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::NONE)
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.is_syncing {
        let text = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ⚡ UNIVERSAL SYNC IN PROGRESS...",
                Style::new().fg(GREEN).bold(),
            )),
            Line::from(Span::styled(
                "  Aligning all agent constitutions with MASTER.md",
                Style::new().fg(DIM),
            )),
        ]);
        frame.render_widget(text, inner);
        return;
    }

    if let Some(ref status) = app.sync_status {
        let msg = format!("  ✓ {}", status);
        let badge = Paragraph::new(Span::styled(msg, Style::new().fg(GREEN)));
        let badge_area = Rect { height: 1, y: inner.y, ..inner };
        frame.render_widget(badge, badge_area);
    }

    match app.menu_cursor {
        0 => render_recent(frame, inner, app),
        1 => render_rules(frame, inner, app),
        2 => render_diagnostics(frame, inner, app),
        3 => render_agents(frame, inner, app),
        4 => render_policies(frame, inner, app),
        5 => render_mempalace_info(frame, inner, app),
        6 => render_search_panel(frame, inner, app),
        7 => render_projects(frame, inner, app),
        8 => render_timeline(frame, inner, app),
        9 => render_logs(frame, inner, app),
        10 => render_help(frame, inner, app),
        11 => render_system_audit(frame, inner, app),
        _ => {}
    }
}

pub fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::NONE)
        .style(Style::new().bg(PANEL_BG));
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut items = Vec::new();
    for act in &app.activities {
        let style = match act.level {
            "Warning" => Style::new().fg(AMBER),
            "Error" => Style::new().fg(RED),
            _ => Style::new().fg(MID),
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", act.timestamp), Style::new().fg(DIM)),
            Span::styled(format!(" [{:<6}] ", act.source), Style::new().fg(CYAN).bold()),
            Span::styled(&act.message, style),
        ])));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled("  No recent activities recorded.", Style::new().fg(DIM).italic()))));
    }

    let list = List::new(items).block(Block::new().title(" SYSTEM TIMELINE ").title_style(Style::new().fg(DIM)));
    frame.render_widget(list, inner);
}

pub fn render_logs(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([
        Constraint::Length(30),
        Constraint::Min(0),
    ]).areas(area);

    // Left Panel: Active Agents
    let mut agent_items = Vec::new();
    for (i, agent) in app.active_agents.iter().enumerate() {
        let style = if i == app.selected_agent_idx {
            Style::new().bg(Color::Rgb(0, 40, 20)).fg(GREEN).bold()
        } else {
            Style::new().fg(MID)
        };
        
        let status_color = match agent.status.as_str() {
            "Running" => GREEN,
            s if s.contains("Completed") => CYAN,
            _ => RED,
        };

        agent_items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(if i == app.selected_agent_idx { "▶ " } else { "  " }, Style::new().fg(GREEN)),
                Span::styled(&agent.name, style),
            ]),
            Line::from(vec![
                Span::styled("    ", Style::new()),
                Span::styled(&agent.status, Style::new().fg(status_color).italic()),
            ]),
        ]));
    }

    if agent_items.is_empty() {
        agent_items.push(ListItem::new(Line::from(Span::styled("  No active agents.", Style::new().fg(DIM).italic()))));
    }

    let agent_list = List::new(agent_items)
        .block(Block::new().borders(Borders::RIGHT).border_style(Style::new().fg(DIM)).title(" AGENTS "));
    frame.render_widget(agent_list, left);

    // Right Panel: Agent Logs
    let mut log_lines = Vec::new();
    if let Some(agent) = app.active_agents.get(app.selected_agent_idx) {
        for log in &agent.logs {
            let style = if log.contains("[stderr]") { Style::new().fg(RED) } else { Style::new().fg(MID) };
            log_lines.push(Line::from(Span::styled(log, style)));
        }
    }

    if log_lines.is_empty() {
        log_lines.push(Line::from(Span::styled("  No logs for selected agent.", Style::new().fg(DIM).italic())));
    }

    let log_para = Paragraph::new(log_lines)
        .block(Block::new().title(" AGENT LOGS "))
        .wrap(Wrap { trim: false });
    frame.render_widget(log_para, right);
}

pub fn render_help(frame: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
    ]).split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled("  SYSTEM DOCUMENTATION & COMMANDS  ", Style::new().fg(CYAN).bold()),
    ])).block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    
    frame.render_widget(title, chunks[0]);

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled("  NAVIGATION & SHORTCUTS", Style::new().fg(GREEN).bold())),
        Line::from(vec![Span::styled("    Ctrl + P      ", Style::new().fg(AMBER)), Span::raw("Neural Fuzzy Search (Global)")]),
        Line::from(vec![Span::styled("    /             ", Style::new().fg(AMBER)), Span::raw("Open Command Palette")]),
        Line::from(vec![Span::styled("    Tab           ", Style::new().fg(AMBER)), Span::raw("Switch between Menu and Content")]),
        Line::from(vec![Span::styled("    Arrows / HJ KL", Style::new().fg(AMBER)), Span::raw("Navigate through lists")]),
        Line::from(vec![Span::styled("    Enter         ", Style::new().fg(AMBER)), Span::raw("Select / Open file / Execute")]),
        Line::from(vec![Span::styled("    Esc           ", Style::new().fg(AMBER)), Span::raw("Back / Close Search / Cancel")]),
        Line::from(vec![Span::styled("    G             ", Style::new().fg(AMBER)), Span::raw("Run Graphify Analysis (in Detail view)")]),
        Line::from(vec![Span::styled("    R             ", Style::new().fg(AMBER)), Span::raw("View Graphify Report (in Detail view)")]),
        Line::from(vec![Span::styled("    Ctrl + C      ", Style::new().fg(AMBER)), Span::raw("Hard Exit")]),
        Line::from(""),
        Line::from(Span::styled("  TERMINAL COMMANDS (starts with /)", Style::new().fg(GREEN).bold())),
        Line::from(vec![Span::styled("    /sync         ", Style::new().fg(AMBER)), Span::raw("Synchronize all agent policies")]),
        Line::from(vec![Span::styled("    /health       ", Style::new().fg(AMBER)), Span::raw("Run system-wide compliance audit")]),
        Line::from(vec![Span::styled("    /reindex      ", Style::new().fg(AMBER)), Span::raw("Rebuild neural search index")]),
        Line::from(vec![Span::styled("    /open [proj]  ", Style::new().fg(AMBER)), Span::raw("Jump to specific project detail")]),
        Line::from(vec![Span::styled("    /rules        ", Style::new().fg(AMBER)), Span::raw("View system constitution")]),
        Line::from(vec![Span::styled("    /memory       ", Style::new().fg(AMBER)), Span::raw("Access Global Memory")]),
        Line::from(vec![Span::styled("    /graphify     ", Style::new().fg(AMBER)), Span::raw("Generate codebase knowledge graph")]),
        Line::from(vec![Span::styled("    /q            ", Style::new().fg(AMBER)), Span::raw("Quit Application")]),
        Line::from(""),
        Line::from(Span::styled("  EDITOR KEYS", Style::new().fg(GREEN).bold())),
        Line::from(vec![Span::styled("    Ctrl + S      ", Style::new().fg(AMBER)), Span::raw("Save file in Editor")]),
        Line::from(vec![Span::styled("    Esc           ", Style::new().fg(AMBER)), Span::raw("Exit Editor without saving")]),
    ];

    frame.render_widget(Paragraph::new(help_text).wrap(Wrap { trim: false }), chunks[1]);
}

pub fn render_recent(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Percentage(40),
    ]).areas(area);

    let banner_lines: Vec<Line> = BANNER
        .lines()
        .map(|l| Line::from(Span::styled(l, Style::new().fg(GREEN))))
        .collect();

    let mut lines = banner_lines;
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " RECENT PROJECTS & CHANGES",
        Style::new().fg(MID).bold(),
    )));
    lines.push(Line::from(""));

    for proj in &app.recent_projects {
        let git_tag = match proj.git_dirty {
            Some(true)  => Span::styled(" ● dirty", Style::new().fg(AMBER)),
            Some(false) => Span::styled(" ○ clean", Style::new().fg(GREEN)),
            None        => Span::styled("", Style::new()),
        };
        let branch_tag = proj.git_branch.as_deref()
            .map(|b| Span::styled(format!(" [{}]", b), Style::new().fg(DIM)))
            .unwrap_or_else(|| Span::styled("", Style::new()));

        lines.push(Line::from(vec![
            Span::styled(" 📁 ", Style::new().fg(CYAN)),
            Span::styled(proj.name.as_str(), Style::new().fg(CYAN).bold()),
            git_tag,
            branch_tag,
            Span::styled("  ", Style::new()),
            Span::styled(proj.rel_path.as_str(), Style::new().fg(DIM)),
        ]));
        for change in &proj.changes {
            lines.push(Line::from(vec![
                Span::styled("    • ", Style::new().fg(DIM)),
                Span::styled(change.as_str(), Style::new().fg(MID)),
            ]));
        }
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_tasks_panel(frame, right, app);
}

pub fn render_tasks_panel(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.right_panel_focus && app.menu_cursor == 0;
    let border_color = if focused { GREEN } else { DIM };

    let hint = if focused {
        " [Space] done  [c] Claude  [g] Gemini  [a] Antigravity "
    } else {
        " [→] focus "
    };

    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(border_color))
        .title(Span::styled(" TASKS ", Style::new().fg(GREEN).bold()))
        .title_bottom(Span::styled(hint, Style::new().fg(DIM)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    if app.tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No tasks — /task add <text> [@agent] [#project]",
            Style::new().fg(DIM).italic(),
        )));
    } else {
        let pending: Vec<_> = app.tasks.iter().enumerate().filter(|(_, t)| !t.completed).collect();
        let done: Vec<_> = app.tasks.iter().enumerate().filter(|(_, t)| t.completed).collect();

        for (i, task) in pending.iter().chain(done.iter()) {
            let selected = *i == app.task_cursor && focused;

            let (mark, text_style) = if task.completed {
                ("✓", Style::new().fg(DIM))
            } else {
                ("☐", Style::new().fg(MID))
            };

            let cursor_span = if selected {
                Span::styled(" ❯ ", Style::new().fg(GREEN).bold())
            } else {
                Span::styled("   ", Style::new())
            };

            let text_color = if selected { GREEN } else if task.completed { DIM } else { MID };

            // Agent badge
            let agent_span = task
                .agent_label()
                .map(|label| {
                    let color = match task.agent.as_deref() {
                        Some("claude")      => GREEN,
                        Some("gemini")      => CYAN,
                        Some("antigravity") => AMBER,
                        _ => DIM,
                    };
                    Span::styled(format!(" {}", label), Style::new().fg(color).bold())
                })
                .unwrap_or_else(|| Span::styled("", Style::new()));

            // Project badge
            let proj_span = task
                .project
                .as_deref()
                .map(|p| Span::styled(format!(" #{}", p), Style::new().fg(DIM)))
                .unwrap_or_else(|| Span::styled("", Style::new()));

            lines.push(Line::from(vec![
                cursor_span,
                Span::styled(format!("{} ", mark), text_style),
                Span::styled(task.display().to_string(), Style::new().fg(text_color)),
                agent_span,
                proj_span,
            ]));
        }

        // Summary footer
        let total = app.tasks.len();
        let done_count = app.tasks.iter().filter(|t| t.completed).count();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}/{} done", done_count, total),
            Style::new().fg(DIM),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

pub fn render_rules(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(" AI OS CONSTITUTION", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    for cat in &app.system_rules {
        lines.push(Line::from(vec![
            Span::styled(" ◈ ", Style::new().fg(GREEN)),
            Span::styled(cat.title, Style::new().fg(GREEN).bold()),
        ]));
        for rule in &cat.rules {
            lines.push(Line::from(vec![
                Span::styled("   • ", Style::new().fg(DIM)),
                Span::styled(*rule, Style::new().fg(MID)),
            ]));
        }
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

pub fn render_agents(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(
            " AI AGENT RULE FILES",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    if app.agent_rule_groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Scanning agent configs...",
            Style::new().fg(DIM),
        )));
    } else {
        for group in &app.agent_rule_groups {
            let file_count = group.files.len();
            let (status_mark, status_color) = if group.exists() {
                ("●", CYAN)
            } else {
                ("○", DIM)
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} {} ", status_mark, group.icon),
                    Style::new().fg(status_color).bold(),
                ),
                Span::styled(group.agent.as_str(), Style::new().fg(CYAN).bold()),
                Span::styled(
                    format!("  {}", group.config_dir),
                    Style::new().fg(DIM),
                ),
                Span::styled(
                    format!("  [{} files]", file_count),
                    Style::new().fg(if file_count > 0 { GREEN } else { DIM }),
                ),
            ]));

            for entry in &group.files {
                let exist_color = if entry.exists() { MID } else { DIM };
                let ro = if entry.read_only { " [ro]" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled("      › ", Style::new().fg(DIM)),
                    Span::styled(entry.name.as_str(), Style::new().fg(exist_color)),
                    Span::styled(ro, Style::new().fg(DIM)),
                ]));
            }

            if file_count == 0 {
                lines.push(Line::from(Span::styled(
                    "      (not configured)",
                    Style::new().fg(DIM).italic(),
                )));
            }
            lines.push(Line::from(""));
        }
    }

    // Runtime agents (binaries in PATH)
    if !app.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            " RUNTIME AGENTS (PATH)",
            Style::new().fg(MID).bold(),
        )));
        lines.push(Line::from(""));
        for agent in &app.agents {
            let (mark, color) = if agent.exists() { ("●", GREEN) } else { ("○", DIM) };
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", mark), Style::new().fg(color)),
                Span::styled(agent.name, Style::new().fg(MID)),
                Span::styled(
                    format!("  {}", agent.path.to_string_lossy()),
                    Style::new().fg(DIM),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Skills
    if !app.skills.is_empty() {
        lines.push(Line::from(Span::styled(
            " INSTALLED SKILLS & AI EXTENSIONS",
            Style::new().fg(MID).bold(),
        )));
        lines.push(Line::from(""));
        
        for s in &app.skills {
            let color = match s.category {
                "Global AI" => Color::Cyan,
                "Local" => Color::Yellow,
                _ => Color::Magenta,
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  ◈ {} ", s.name), Style::new().fg(color)),
                Span::styled(format!(" ({}) ", s.category), Style::new().fg(DIM).italic()),
                Span::styled(&s.description, Style::new().fg(DIM)),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), area);
}

pub fn render_policies(frame: &mut Frame, area: Rect, app: &App) {
    let _ = app;
    let lines = vec![
        Line::from(Span::styled(" SECURITY POLICIES", Style::new().fg(MID).bold())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  • Auto-Allow Skills:     ", Style::new().fg(MID)),
            Span::styled("ENFORCED", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • Safe Read Operations:  ", Style::new().fg(MID)),
            Span::styled("ENFORCED", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • RLS (Day 0):           ", Style::new().fg(MID)),
            Span::styled("MANDATORY", Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled("  • API Keys Client-Side:  ", Style::new().fg(MID)),
            Span::styled("FORBIDDEN", Style::new().fg(RED).bold()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

pub fn render_git_diff_view(frame: &mut Frame, app: &App) {
    let size = frame.area();
    
    // Clear the whole screen
    frame.render_widget(Block::new().bg(PANEL_BG), size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Diff content
            Constraint::Length(3), // Footer
        ])
        .split(size);

    // Header
    let current_idx = app.pending_change_cursor;
    let total = app.pending_file_changes.len();
    let path = app.pending_file_changes.get(current_idx).map(|p| p.path.clone()).unwrap_or_default();
    
    let header_text = format!(" FILE CHANGE APPROVAL [{}/{}] — {}", current_idx + 1, total, path);
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::new().fg(AMBER)))
        .style(Style::new().fg(AMBER).bold())
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(header, chunks[0]);

    // Diff Content
    let mut lines = Vec::new();
    for line in app.git_diff_lines.iter().skip(app.git_diff_scroll as usize) {
        if line.starts_with('+') {
            lines.push(Line::from(Span::styled(line, Style::new().fg(GREEN))));
        } else if line.starts_with('-') {
            lines.push(Line::from(Span::styled(line, Style::new().fg(RED))));
        } else {
            lines.push(Line::from(Span::styled(line, Style::new().fg(MID))));
        }
    }

    let diff = Paragraph::new(lines)
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .wrap(Wrap { trim: false });
    frame.render_widget(diff, chunks[1]);

    // Footer / Actions
    let footer_text = vec![
        Span::styled(" [Y] ", Style::new().fg(GREEN).bold()),
        Span::styled("Approve  ", Style::new().fg(MID)),
        Span::styled(" [N] ", Style::new().fg(RED).bold()),
        Span::styled("Reject  ", Style::new().fg(MID)),
        Span::styled(" [←/→] ", Style::new().fg(CYAN).bold()),
        Span::styled("Cycle Items  ", Style::new().fg(MID)),
        Span::styled(" [Esc] ", Style::new().fg(MID).bold()),
        Span::styled("Dashboard", Style::new().fg(MID)),
    ];
    let footer = Paragraph::new(Line::from(footer_text))
        .block(Block::default().borders(Borders::ALL).border_style(Style::new().fg(DIM)))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(footer, chunks[2]);
}


