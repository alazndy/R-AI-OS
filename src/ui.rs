use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect, Alignment, Direction},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, AppState, MENU_ITEMS, filtered_palette};
use crate::filebrowser::FileEntry;

// ─── Colour palette ──────────────────────────────────────────────────────────

const GREEN: Color = Color::Rgb(0, 255, 136);
const CYAN: Color = Color::Rgb(0, 220, 220);
const DIM: Color = Color::Rgb(80, 80, 80);
const MID: Color = Color::Rgb(170, 170, 170);
const AMBER: Color = Color::Rgb(255, 170, 0);
const RED: Color = Color::Rgb(255, 80, 80);
const PANEL_BG: Color = Color::Rgb(8, 12, 16);
const HEADER_BG: Color = Color::Rgb(0, 20, 12);

// ─── Spinner ─────────────────────────────────────────────────────────────────

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn spinner_char(tick: u64) -> char {
    SPINNER[(tick as usize) % SPINNER.len()]
}

// ─── Banner (6 lines) ────────────────────────────────────────────────────────

const BANNER: &str = "\
  ██████╗       █████╗ ██╗      ██████╗ ███████╗\n\
  ██╔══██╗     ██╔══██╗██║     ██╔═══██╗██╔════╝\n\
  ██████╔╝     ███████║██║     ██║   ██║███████╗\n\
  ██╔══██╗     ██╔══██║██║     ██║   ██║╚════██║\n\
  ██║  ██║     ██║  ██║██║     ╚██████╔╝███████║\n\
  ╚═╝  ╚═╝     ╚═╝  ╚═╝╚═╝      ╚═════╝ ╚══════╝";

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    match app.state {
        AppState::Booting        => render_boot(frame, app),
        AppState::Setup          => render_setup(frame, app),
        AppState::Search         => render_search(frame, app),
        AppState::Dashboard      => render_dashboard(frame, app),
        AppState::FileView       => render_file_view(frame, app),
        AppState::FileEdit       => render_file_edit(frame, app),
        AppState::ProjectDetail  => render_project_detail(frame, app),
        AppState::HealthView     => render_health_view(frame, app),
        AppState::MemPalaceView  => render_mempalace_view(frame, app),
        AppState::GraphReport    => render_graph_report(frame, app),
    }
    // Overlays rendered after everything else
    if app.show_launcher {
        render_launcher_modal(frame, app);
    }
    if app.command_mode && !app.command_buf.is_empty() {
        render_command_palette(frame, app);
    }
    if app.file_changed_externally {
        render_file_changed_badge(frame, app);
    }
}

// ─── Boot screen ─────────────────────────────────────────────────────────────

fn render_boot(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let total = 5u16;
    let done = app.boot_results.len() as u16;
    let progress = if total > 0 { (done * 100 / total).min(100) } else { 0 };

    let center = center_rect(60, (total + 10).min(area.height), area);

    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(center);

    let spin = spinner_char(app.tick);
    let heading = Paragraph::new(format!(
        " {}  Initializing R-AI-OS v0.2.3 Core...",
        spin
    ))
    .style(Style::new().fg(GREEN).add_modifier(Modifier::BOLD));
    frame.render_widget(heading, rows[0]);

    let gauge = Gauge::default()
        .block(Block::new())
        .gauge_style(Style::new().fg(GREEN).bg(DIM))
        .percent(progress)
        .label(format!("{}/{} checks", done, total));
    frame.render_widget(gauge, rows[2]);

    let items: Vec<ListItem> = app
        .boot_results
        .iter()
        .map(|(name, pass)| {
            let (mark, color) = if *pass { ("✓", GREEN) } else { ("✗", RED) };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", mark), Style::new().fg(color).bold()),
                Span::styled(name.as_str(), Style::new().fg(MID)),
            ]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, rows[3]);
}

// ─── Setup / First-run ───────────────────────────────────────────────────────

fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let center = center_rect(72, 22, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(AMBER))
        .title(Span::styled(" R-AI-OS FIRST RUN SETUP ", Style::new().fg(AMBER).bold()))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(center);
    frame.render_widget(block, center);

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(inner);

    let mut left_lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "  Configure your workspace paths:",
            Style::new().fg(DIM),
        )),
        Line::from(""),
    ];

    for (i, field) in app.setup_fields.iter().enumerate() {
        let is_selected = i == app.setup_cursor;
        let is_editing  = is_selected && app.setup_editing;

        let label_style = if is_selected {
            Style::new().fg(AMBER).bold()
        } else {
            Style::new().fg(MID)
        };

        left_lines.push(Line::from(vec![
            Span::styled(format!(" {} ", if is_selected { "▶" } else { " " }), label_style),
            Span::styled(field.label, label_style),
        ]));
        left_lines.push(Line::from(Span::styled(
            format!("     {}", field.hint),
            Style::new().fg(DIM).italic(),
        )));

        let (display_val, val_style) = if is_editing {
            (format!("  {}█", app.setup_input), Style::new().fg(Color::White).bg(Color::Rgb(20, 30, 20)))
        } else if field.value.is_empty() {
            ("  (not set — press Enter to edit)".into(), Style::new().fg(RED))
        } else if field.auto_detected {
            (format!("  ✓ {}", field.value), Style::new().fg(GREEN))
        } else {
            (format!("  ✏ {}", field.value), Style::new().fg(CYAN))
        };

        left_lines.push(Line::from(Span::styled(display_val, val_style)));
        left_lines.push(Line::from(""));
    }

    if let Some(ref status) = app.setup_status {
        let color = if status.starts_with("Save") { RED } else { AMBER };
        left_lines.push(Line::from(Span::styled(format!(" ⚠ {}", status), Style::new().fg(color).bold())));
    }

    left_lines.push(Line::from(""));
    left_lines.push(Line::from(Span::styled(
        " [↑↓] navigate  [Enter] edit  [s] save  [q] quit",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(left_lines)), layout[0]);

    // Right side: Requirements
    let mut right_lines = vec![
        Line::from(Span::styled(" System Requirements:", Style::new().fg(DIM))),
        Line::from(""),
    ];

    for req in &app.requirements {
        let (icon, color) = if req.installed { ("✓", GREEN) } else { ("✗", RED) };
        let crit_tag = if req.critical && !req.installed {
            Span::styled(" [CRITICAL]", Style::new().fg(RED).bold())
        } else {
            Span::styled("", Style::new())
        };
        right_lines.push(Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::new().fg(color).bold()),
            Span::styled(req.name, Style::new().fg(MID)),
            crit_tag,
        ]));
        if req.installed {
            right_lines.push(Line::from(Span::styled(
                format!("    {}", req.version.split('\n').next().unwrap_or("")),
                Style::new().fg(DIM).italic(),
            )));
        } else {
            right_lines.push(Line::from(Span::styled(
                format!("    missing binary: {}", req.command),
                Style::new().fg(Color::Rgb(100, 50, 50)).italic(),
            )));
        }
        right_lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(right_lines)), layout[1]);
}

// ─── Search Modal (Ctrl+P) ──────────────────────────────────────────────────

fn render_search(frame: &mut Frame, app: &App) {
    // Render dashboard in background
    render_dashboard(frame, app);

    // Dim the background
    let area = frame.area();
    // let dim = Block::new().style(Style::new().bg(Color::Rgb(0, 0, 0)));
    // frame.render_widget(dim, area);

    let popup_area = center_rect(80, 70, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CYAN))
        .title(vec![
            Span::styled(" FUZZY FINDER ", Style::new().fg(CYAN).bold()),
            Span::styled(" (indexer active) ", Style::new().fg(DIM).italic()),
        ])
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    // Search Input
    let input = Paragraph::new(format!(" > {}", app.search_query))
        .style(Style::new().fg(Color::White))
        .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(input, chunks[0]);

    // Results
    let mut items = Vec::new();
    for (i, res) in app.search_results.iter().enumerate() {
        let is_selected = i == app.search_cursor;
        let style = if is_selected {
            Style::new().bg(Color::Rgb(30, 40, 50)).fg(AMBER).bold()
        } else {
            Style::new().fg(MID)
        };

        let file_name = res.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let mut line_content = vec![
            Span::styled(format!(" {:<20} ", res.project), Style::new().fg(DIM)),
            Span::styled(format!("{:<30} ", file_name), style),
            Span::styled(format!("Ln {:<4} ", res.line), Style::new().fg(DIM)),
        ];
        
        if is_selected {
            line_content.push(Span::styled(format!("| {}", res.snippet), Style::new().fg(AMBER).italic()));
        } else {
            line_content.push(Span::styled(format!("| {}", res.snippet), Style::new().fg(DIM)));
        }

        items.push(ListItem::new(Line::from(line_content)));
    }

    if items.is_empty() && !app.search_query.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled("   No results found.", Style::new().fg(RED)))));
    }

    let results_list = List::new(items)
        .highlight_style(Style::new().add_modifier(Modifier::BOLD))
        .block(Block::new());
    
    frame.render_widget(results_list, chunks[1]);
}

// ─── Dashboard ───────────────────────────────────────────────────────────────

fn render_dashboard(frame: &mut Frame, app: &App) {
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

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let version_tag = Span::styled(" v0.2.3 (Stable) ", Style::new().fg(DIM));
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

    let header = Paragraph::new(Line::from(vec![title, version_tag, sync_tag]))
        .block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(DIM))
                .style(Style::new().bg(HEADER_BG)),
        )
        .alignment(Alignment::Left);
    frame.render_widget(header, area);
}

fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_content(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_content_body(frame: &mut Frame, area: Rect, app: &App) {
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
        _ => {}
    }
}

fn render_timeline(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_logs(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::NONE)
        .style(Style::new().bg(PANEL_BG));
    
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut items = Vec::new();
    for log in app.logs.iter().rev().take(50) {
        items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(format!(" {} ", log.timestamp), Style::new().fg(DIM)),
                Span::styled(format!(" {} ", log.sender), Style::new().fg(GREEN).bold()),
            ]),
            Line::from(Span::styled(format!("  {}", log.content), Style::new().fg(MID))),
            Line::from(""),
        ]));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled("  Waiting for logs...", Style::new().fg(DIM).italic()))));
    }

    let list = List::new(items).block(Block::new().title(" LIVE LOGS ").title_style(Style::new().fg(DIM)));
    frame.render_widget(list, inner);
}

fn render_help(frame: &mut Frame, area: Rect, _app: &App) {
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

fn render_recent(frame: &mut Frame, area: Rect, app: &App) {
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

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_rules(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_diagnostics(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(" SYSTEM DIAGNOSTICS", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    for (name, pass) in &app.boot_results {
        let (mark, color) = if *pass { ("✓", GREEN) } else { ("✗", RED) };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", mark), Style::new().fg(color)),
            Span::styled(name.as_str(), Style::new().fg(MID)),
        ]));
    }

    if let Some(ref report) = app.compliance {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(" PROJECT COMPLIANCE REPORT", Style::new().fg(MID).bold())));
        lines.push(Line::from(format!("  Score: {}/100", report.score)));
        lines.push(Line::from(""));

        if report.violations.is_empty() {
            lines.push(Line::from(Span::styled("  ✓ No compliance issues found. Excellent work!", Style::new().fg(GREEN))));
        } else {
            for v in &report.violations {
                lines.push(Line::from(vec![
                    Span::styled("  ⚠ ", Style::new().fg(AMBER)),
                    Span::styled(format!("Line {}: ", v.line), Style::new().fg(DIM)),
                    Span::styled(v.rule, Style::new().fg(MID)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Press 'f' to attempt Auto-Fix with Claude", Style::new().fg(CYAN).italic())));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_agents(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_policies(frame: &mut Frame, area: Rect, app: &App) {
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

fn render_mempalace_info(frame: &mut Frame, area: Rect, app: &App) {
    let count: usize = app.mp_rooms.iter().map(|r| r.projects.len()).sum();
    let hint = if count > 0 {
        format!("  {} rooms · {} projects — /mempalace to open full view", app.mp_rooms.len(), count)
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

fn render_search_panel(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(
            " NEURAL SEARCH — Content-Aware Project Index",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    // Index status line
    let (status_text, status_color) = if app.is_indexing {
        ("  Building index...".to_string(), AMBER)
    } else if let Some(ref s) = app.index_status {
        (format!("  {}", s), GREEN)
    } else {
        ("  No index — use /search <query> to build".to_string(), DIM)
    };
    lines.push(Line::from(Span::styled(status_text, Style::new().fg(status_color))));
    lines.push(Line::from(""));

    if app.search_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Use /search <query> to search across all projects",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Example: /search hata yönetimi",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Example: /search async error handling",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  /reindex  — force rebuild the index",
            Style::new().fg(DIM),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} results", app.search_results.len()),
                Style::new().fg(CYAN).bold(),
            ),
            Span::styled(
                "  [→] focus  [↑↓] navigate  [Enter] open",
                Style::new().fg(DIM),
            ),
        ]));
        lines.push(Line::from(""));

        for (i, result) in app.search_results.iter().enumerate() {
            let is_selected = app.right_panel_focus && i == app.search_cursor;

            let file_name = result
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled("  ▶ ", Style::new().fg(GREEN).bold()),
                    Span::styled(file_name.to_string(), Style::new().fg(GREEN).bold()),
                    Span::styled(
                        format!(":{}", result.line),
                        Style::new().fg(AMBER),
                    ),
                    Span::styled(
                        format!("  [{}]", result.project),
                        Style::new().fg(DIM),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("      {}", result.snippet),
                    Style::new().fg(MID),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::new()),
                    Span::styled(file_name.to_string(), Style::new().fg(CYAN)),
                    Span::styled(
                        format!(":{}", result.line),
                        Style::new().fg(DIM),
                    ),
                    Span::styled(
                        format!("  [{}]", result.project),
                        Style::new().fg(DIM),
                    ),
                ]));
                let snippet: String = result.snippet.chars().take(70).collect();
                lines.push(Line::from(Span::styled(
                    format!("      {}", snippet),
                    Style::new().fg(DIM),
                )));
            }
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), area);
}

fn project_status_color(status: &str) -> Color {
    match status {
        "production" => GREEN,
        "active" => CYAN,
        "early" => AMBER,
        "legacy" => DIM,
        _ => MID,
    }
}

fn render_projects(frame: &mut Frame, area: Rect, app: &App) {
    let total = app.projects.len();
    let visible_h = area.height.saturating_sub(2) as usize; // header + spacer
    
    let scroll = if app.project_cursor >= visible_h {
        app.project_cursor - visible_h + 1
    } else {
        0
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" ALL PROJECTS", Style::new().fg(MID).bold()),
            Span::styled(
                format!("  ({} total)", total),
                Style::new().fg(DIM),
            ),
            Span::styled(
                if app.right_panel_focus { "  [↑↓] navigate  [Enter] open  [←] menu" }
                else { "  [→] focus  [/open <name>] jump" },
                Style::new().fg(DIM),
            ),
        ]),
        Line::from(""),
    ];

    if app.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            "  entities.json not found or empty",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Expected: Dev Ops/entities.json",
            Style::new().fg(DIM),
        )));
    } else {
        for (i, proj) in app.projects.iter().enumerate().skip(scroll).take(visible_h) {
            let is_selected = app.right_panel_focus && i == app.project_cursor;
            let sc = project_status_color(&proj.status);
            let cat = proj.category.replace('_', " ");

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled("  ▶ ", Style::new().fg(GREEN).bold()),
                    Span::styled(proj.name.as_str(), Style::new().fg(GREEN).bold()),
                    Span::styled(format!("  [{}]", proj.status), Style::new().fg(sc).bold()),
                    Span::styled(format!("  {}", cat), Style::new().fg(DIM)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::new()),
                    Span::styled(proj.name.as_str(), Style::new().fg(MID)),
                    Span::styled(format!("  [{}]", proj.status), Style::new().fg(sc)),
                    Span::styled(format!("  {}", cat), Style::new().fg(DIM)),
                ]));
            }
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), area);
}

fn render_project_detail(frame: &mut Frame, app: &App) {
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
        Constraint::Length(5),
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

    let stats = vec![
        Line::from(vec![
            Span::styled(" Status  ", Style::new().fg(DIM)),
            Span::styled(proj.status.as_str(), Style::new().fg(sc).bold()),
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

fn render_file_panel(frame: &mut Frame, area: Rect, app: &App, files: &[FileEntry]) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::from(vec![
        Span::styled(
            if app.right_panel_focus { " FILES " } else { " FILES " },
            Style::new()
                .fg(if app.right_panel_focus { GREEN } else { DIM })
                .bold(),
        ),
        Span::styled(
            if app.right_panel_focus { "[↑↓] nav  [Enter] view  [e] edit  [o] ext  [←] menu" } else { "[→] focus" },
            Style::new().fg(DIM),
        ),
    ])];

    for (i, entry) in files.iter().enumerate() {
        let exist_mark = if entry.exists() {
            Span::styled("✓ ", Style::new().fg(GREEN))
        } else {
            Span::styled("✗ ", Style::new().fg(DIM))
        };
        let ro_tag = if entry.read_only {
            Span::styled(" [RO]", Style::new().fg(DIM))
        } else {
            Span::styled("", Style::new())
        };

        if app.right_panel_focus && i == app.right_file_cursor {
            lines.push(Line::from(vec![
                exist_mark,
                Span::styled(format!("▶ {}", entry.name), Style::new().fg(GREEN).bold()),
                ro_tag,
            ]));
        } else {
            lines.push(Line::from(vec![
                exist_mark,
                Span::styled(format!("  {}", entry.name), Style::new().fg(MID)),
                ro_tag,
            ]));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn render_launcher(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = if app.command_mode {
        Line::from(vec![
            Span::styled(" ❯ ", Style::new().fg(GREEN).bold()),
            Span::styled(app.command_buf.as_str(), Style::new().fg(Color::White)),
            Span::styled("█", Style::new().fg(GREEN)),
        ])
    } else if app.right_panel_focus {
        Line::from(Span::styled(
            " [↑↓] navigate  [Enter] view  [e] edit  [o] VS Code  [←] menu  [/] command",
            Style::new().fg(DIM),
        ))
    } else {
        let hint = if !app.current_menu_files().is_empty() { "  [→] files" } else { "" };
        Line::from(Span::styled(
            format!(" [↑↓] menu  [/] or [Tab] command{}", hint),
            Style::new().fg(DIM),
        ))
    };

    frame.render_widget(Paragraph::new(content), inner);
}

// ─── File viewer ─────────────────────────────────────────────────────────────

fn render_file_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let Some(ref file) = app.active_file else { return };

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let ro_tag = if file.read_only { " [READONLY]" } else { "" };
    let compliance_span = if let Some(ref report) = app.compliance {
        let color = match report.score_color() {
            0 => GREEN,
            1 => AMBER,
            _ => RED,
        };
        let lang = report.language();
        let lang_part = if lang.is_empty() { String::new() } else { format!(" [{}]", lang) };
        let issues = report.violations.len();
        Span::styled(
            format!("  📊 {}/100 {}{} [{} issues]", report.score, report.grade(), lang_part, issues),
            Style::new().fg(color).bold(),
        )
    } else {
        Span::styled("", Style::new())
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(file.name.as_str(), Style::new().fg(GREEN).bold()),
            Span::styled(ro_tag, Style::new().fg(AMBER)),
            compliance_span,
        ]),
        Line::from(Span::styled(
            format!("  {}", file.path.display()),
            Style::new().fg(DIM),
        )),
    ])
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(header, header_area);

    // Content with line numbers + syntax highlighting
    let scroll = app.file_scroll as usize;
    let visible_h = content_area.height as usize;
    let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut in_block = false;
    for line in app.file_lines.iter().take(scroll) {
        update_code_block_state(line, ext, &mut in_block);
    }

    let mut lines: Vec<Line> = Vec::new();
    for (i, line) in app.file_lines.iter().enumerate().skip(scroll).take(visible_h) {
        let lno = Span::styled(format!("{:>4} │ ", i + 1), Style::new().fg(DIM));
        let mut spans = vec![lno];
        spans.extend(highlight_line(line, &mut in_block, ext));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Footer
    let total = app.file_lines.len();
    let pct = if total > 0 {
        ((scroll + visible_h).min(total) * 100 / total).min(100)
    } else {
        100
    };
    let edit_hint = if !file.read_only { "  [e] edit" } else { "" };
    let issue_span = app
        .compliance
        .as_ref()
        .and_then(|r| r.first_issue())
        .map(|txt| {
            Span::styled(format!("   ⚠ {}", txt), Style::new().fg(AMBER))
        });
    let mut footer_spans = vec![
        Span::styled(
            format!(" Ln {}/{} ({}%)", scroll + 1, total, pct),
            Style::new().fg(DIM),
        ),
        Span::styled(
            format!("   [↑↓/PgUp/PgDn] scroll{}  [Esc/q] back", edit_hint),
            Style::new().fg(DIM),
        ),
    ];
    if let Some(s) = issue_span {
        footer_spans.push(s);
    }
    let footer = Paragraph::new(Line::from(footer_spans))
        .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

// ─── Syntax highlighting ──────────────────────────────────────────────────────

const PURPLE: Color = Color::Rgb(180, 100, 220);
const TEAL: Color   = Color::Rgb(100, 200, 200);
const OLIVE: Color  = Color::Rgb(100, 180, 100);

pub fn update_code_block_state(line: &str, ext: &str, in_block: &mut bool) {
    if ext == "md" {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            *in_block = !*in_block;
        }
    }
}

pub fn highlight_line<'a>(line: &'a str, in_block: &mut bool, ext: &str) -> Vec<Span<'a>> {
    match ext {
        "md" => {
            let t = line.trim();
            if t.starts_with("```") || t.starts_with("~~~") {
                *in_block = !*in_block;
                return vec![Span::styled(line, Style::new().fg(AMBER))];
            }
            if *in_block {
                return vec![Span::styled(line, Style::new().fg(AMBER))];
            }
            highlight_markdown(line)
        }
        "rs" => highlight_rust(line),
        "ts" | "tsx" | "js" | "jsx" => highlight_typescript(line),
        "py" => highlight_python(line),
        "toml" => highlight_toml(line),
        "yaml" | "yml" => highlight_yaml(line),
        _ => vec![Span::styled(line, Style::new().fg(MID))],
    }
}

fn highlight_markdown(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("# ") || t == "#" {
        return vec![Span::styled(line, Style::new().fg(GREEN).bold())];
    }
    if t.starts_with("## ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("### ") {
        return vec![Span::styled(line, Style::new().fg(AMBER).bold())];
    }
    if t.starts_with("####") {
        return vec![Span::styled(line, Style::new().fg(MID).bold())];
    }
    if t.trim() == "---" || t.trim() == "===" || t.trim() == "___" {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("> ") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("- [x] ") || t.starts_with("- [X] ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("- [ ] ") {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        let prefix_len = line.len() - t.len() + 2;
        return vec![
            Span::styled(&line[..prefix_len], Style::new().fg(GREEN)),
            Span::styled(&line[prefix_len..], Style::new().fg(MID)),
        ];
    }
    if t.starts_with('|') {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.starts_with("    ") || t.starts_with('\t') {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_rust(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("///") || t.starts_with("//!") {
        return vec![Span::styled(line, Style::new().fg(GREEN).italic())];
    }
    if t.starts_with("//") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("#[") || t.starts_with("#![") {
        return vec![Span::styled(line, Style::new().fg(PURPLE))];
    }
    if t.contains("fn ") && (t.starts_with("fn ") || t.starts_with("pub ") || t.starts_with("async ")) {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("pub struct ") || t.starts_with("struct ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("pub enum ") || t.starts_with("enum ") {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.starts_with("impl") {
        return vec![Span::styled(line, Style::new().fg(AMBER))];
    }
    if t.starts_with("pub trait ") || t.starts_with("trait ") {
        return vec![Span::styled(line, Style::new().fg(TEAL))];
    }
    if t.starts_with("use ") || t.starts_with("mod ") || t.starts_with("pub mod ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("type ") || t.starts_with("pub type ") || t.starts_with("const ") || t.starts_with("pub const ") || t.starts_with("static ") {
        return vec![Span::styled(line, Style::new().fg(OLIVE))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_typescript(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with("//") {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("import ") || t.starts_with("export {") || t.starts_with("export * ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with("interface ") || t.starts_with("export interface ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("type ") || t.starts_with("export type ") {
        return vec![Span::styled(line, Style::new().fg(CYAN))];
    }
    if t.contains("function ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("class ") || t.starts_with("export class ") || t.starts_with("abstract class ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_python(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with("def ") || t.starts_with("async def ") {
        return vec![Span::styled(line, Style::new().fg(GREEN))];
    }
    if t.starts_with("class ") {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    if t.starts_with("import ") || t.starts_with("from ") {
        return vec![Span::styled(line, Style::new().fg(DIM))];
    }
    if t.starts_with('@') {
        return vec![Span::styled(line, Style::new().fg(PURPLE))];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_toml(line: &str) -> Vec<Span<'_>> {
    let t = line.trim();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if t.starts_with('[') && t.ends_with(']') {
        return vec![Span::styled(line, Style::new().fg(GREEN).bold())];
    }
    if let Some(pos) = line.find(" = ") {
        return vec![
            Span::styled(&line[..pos], Style::new().fg(CYAN)),
            Span::styled(&line[pos..], Style::new().fg(AMBER)),
        ];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

fn highlight_yaml(line: &str) -> Vec<Span<'_>> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return vec![Span::styled(line, Style::new().fg(DIM).italic())];
    }
    if let Some(pos) = line.find(": ") {
        return vec![
            Span::styled(&line[..pos + 1], Style::new().fg(CYAN)),
            Span::styled(&line[pos + 1..], Style::new().fg(MID)),
        ];
    }
    if t.ends_with(':') {
        return vec![Span::styled(line, Style::new().fg(CYAN).bold())];
    }
    vec![Span::styled(line, Style::new().fg(MID))]
}

// ─── File editor ─────────────────────────────────────────────────────────────

fn render_file_edit(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let Some(ref file) = app.active_file else { return };

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let edit_compliance_span = if let Some(ref report) = app.compliance {
        let color = match report.score_color() {
            0 => GREEN,
            1 => AMBER,
            _ => RED,
        };
        Span::styled(
            format!("  📊 {}/100 {}", report.score, report.grade()),
            Style::new().fg(color),
        )
    } else {
        Span::styled("", Style::new())
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ✏ ", Style::new().fg(AMBER)),
            Span::styled(file.name.as_str(), Style::new().fg(AMBER).bold()),
            Span::styled("  ", Style::new()),
            Span::styled("EDITING", Style::new().fg(Color::White).bg(Color::Rgb(60, 20, 0)).bold()),
            edit_compliance_span,
        ]),
        Line::from(Span::styled(
            format!("  {}", file.path.display()),
            Style::new().fg(DIM),
        )),
    ])
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(header, header_area);

    // Editor content
    let ed = &app.editor;
    let scroll = ed.scroll;
    let visible_h = content_area.height as usize;

    // Cursor position in the frame
    let cursor_screen_row = ed.cursor_row.saturating_sub(scroll);
    let cursor_screen_col = ed.cursor_col + 7; // 7 = "  NNN │ " prefix width

    let lines: Vec<Line> = ed
        .lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
        .map(|(i, line)| {
            let lno = Span::styled(format!("  {:>3} │ ", i + 1), Style::new().fg(DIM));
            if i == ed.cursor_row {
                // Highlight cursor line
                Line::from(vec![
                    lno,
                    Span::styled(line.as_str(), Style::new().fg(Color::White).bg(Color::Rgb(15, 25, 35))),
                ])
            } else {
                Line::from(vec![
                    lno,
                    Span::styled(line.as_str(), Style::new().fg(MID)),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Place the terminal cursor
    if cursor_screen_row < content_area.height as usize {
        frame.set_cursor_position((
            content_area.x + cursor_screen_col as u16,
            content_area.y + cursor_screen_row as u16,
        ));
    }

    // Footer
    let save_msg = app
        .edit_save_msg
        .as_deref()
        .map(|m| {
            let color = if m.starts_with("Error") { RED } else { GREEN };
            Span::styled(format!("  {}", m), Style::new().fg(color).bold())
        })
        .unwrap_or_else(|| Span::styled("", Style::new()));

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" Ln {}/{}", ed.cursor_row + 1, ed.lines.len()),
            Style::new().fg(DIM),
        ),
        Span::styled(
            "   [Ctrl+S] save  [Ctrl+Q/Esc] cancel",
            Style::new().fg(DIM),
        ),
        save_msg,
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

// ─── MemPalace full-screen view ───────────────────────────────────────────────

fn render_mempalace_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // ── Header ───────────────────────────────────────────────────────────────
    let total_projects: usize = app.mp_rooms.iter().map(|r| r.projects.len()).sum();
    let filter_hint = if app.mp_filter.is_empty() {
        Span::styled("", Style::new())
    } else {
        Span::styled(
            format!("  filter: {}", app.mp_filter),
            Style::new().fg(AMBER).bold(),
        )
    };

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  MEM", Style::new().fg(RED).bold()),
            Span::styled("PALACE", Style::new().fg(MID).bold()),
            Span::styled(
                format!("  {} rooms  ·  {} projects", app.mp_rooms.len(), total_projects),
                Style::new().fg(DIM),
            ),
            filter_hint,
        ]),
        Line::from(Span::styled(
            "  Dev Ops workspace — filesystem scan",
            Style::new().fg(DIM),
        )),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(header, header_area);

    // ── Content — two columns ────────────────────────────────────────────────
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Percentage(48),
        Constraint::Percentage(52),
    ])
    .areas(content_area);

    // Build visible room/project rows, filtering by mp_filter
    let filter = app.mp_filter.to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line_idx = 0;
    let mut current_line_idx = 0;

    for (ri, room) in app.mp_rooms.iter().enumerate() {
        let is_room_selected = ri == app.mp_room_cursor && app.mp_proj_cursor.is_none();
        if is_room_selected {
            selected_line_idx = current_line_idx;
        }
        let expanded = app.mp_expanded.get(ri).copied().unwrap_or(true);

        let proj_count = if filter.is_empty() {
            room.projects.len()
        } else {
            room.projects.iter()
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
            Span::styled(room.folder_name.as_str(), Style::new().fg(room_color).bold()),
            Span::styled(
                format!("  ({} projects)", proj_count),
                Style::new().fg(DIM),
            ),
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

            let is_proj_selected = ri == app.mp_room_cursor
                && app.mp_proj_cursor == Some(pi);
            if is_proj_selected {
                selected_line_idx = current_line_idx;
            }

            let mem_icon = if proj.has_memory {
                Span::styled("✓", Style::new().fg(GREEN))
            } else {
                Span::styled("✗", Style::new().fg(DIM))
            };

            let proj_color = if is_proj_selected { GREEN } else { MID };
            let proj_prefix = if is_proj_selected { "    ▶ " } else { "      " };
            let date_str = if proj.date == "—" { "".to_string() } else { format!("  {}", proj.date) };

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

fn build_preview(app: &App) -> Vec<Line<'static>> {
    let room = match app.mp_rooms.get(app.mp_room_cursor) {
        Some(r) => r,
        None => return vec![Line::from(Span::styled("  No rooms loaded", Style::new().fg(DIM)))],
    };

    // If on a specific project, show its status
    if let Some(pi) = app.mp_proj_cursor {
        if let Some(proj) = room.projects.get(pi) {
            let mut lines = vec![
                Line::from(Span::styled(proj.name.clone(), Style::new().fg(GREEN).bold())),
                Line::from(Span::styled(
                    format!("  {}", room.folder_name),
                    Style::new().fg(DIM),
                )),
                Line::from(""),
            ];

            let mem_icon = if proj.has_memory { "✓ memory.md" } else { "✗ no memory.md" };
            let mem_color = if proj.has_memory { GREEN } else { RED };
            lines.push(Line::from(Span::styled(mem_icon, Style::new().fg(mem_color))));

            if proj.date != "—" {
                lines.push(Line::from(Span::styled(
                    format!("  Last update: {}", proj.date),
                    Style::new().fg(DIM),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Status:", Style::new().fg(DIM))));

            let status = proj.status.clone();
            let truncated: String = status.chars().take(120).collect();
            for chunk in truncated.chars().collect::<Vec<_>>().chunks(55) {
                let s: String = chunk.iter().collect();
                lines.push(Line::from(Span::styled(
                    format!("  {}", s),
                    Style::new().fg(MID),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  [Enter] open detail",
                Style::new().fg(DIM).italic(),
            )));

            return lines;
        }
    }

    // Room-level preview: list all projects
    let mut lines = vec![
        Line::from(vec![
            Span::styled(room.icon, Style::new()),
            Span::styled(format!("  {}", room.folder_name), Style::new().fg(CYAN).bold()),
        ]),
        Line::from(Span::styled(
            format!("  {} projects", room.projects.len()),
            Style::new().fg(DIM),
        )),
        Line::from(""),
    ];

    let with_mem = room.projects.iter().filter(|p| p.has_memory).count();
    lines.push(Line::from(Span::styled(
        format!("  ✓ memory.md: {}/{}", with_mem, room.projects.len()),
        Style::new().fg(if with_mem == room.projects.len() { GREEN } else { AMBER }),
    )));
    lines.push(Line::from(""));

    // Latest projects
    lines.push(Line::from(Span::styled("  Recent:", Style::new().fg(DIM))));
    for proj in room.projects.iter().take(6) {
        let color = if proj.has_memory { MID } else { DIM };
        lines.push(Line::from(Span::styled(
            format!("  · {}", proj.name),
            Style::new().fg(color),
        )));
    }

    lines
}

// ─── Health Dashboard ─────────────────────────────────────────────────────────

fn render_health_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let status = if app.is_checking_health {
        Span::styled(" Checking projects...", Style::new().fg(AMBER).bold())
    } else {
        Span::styled(
            format!(" {} projects checked", app.health_report.len()),
            Style::new().fg(GREEN),
        )
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  HEALTH DASHBOARD", Style::new().fg(MID).bold()),
            status,
        ]),
        Line::from(Span::styled(
            "  Constitution compliance + git status across all projects",
            Style::new().fg(DIM),
        )),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(header, header_area);

    // Content
    if app.is_checking_health && app.health_report.is_empty() {
        let msg = Paragraph::new(Span::styled(
            "  Running checks...",
            Style::new().fg(DIM).italic(),
        ));
        frame.render_widget(msg, content_area);
        return;
    }

    let visible = content_area.height as usize;
    let scroll = app.health_cursor.saturating_sub(visible / 2);
    let mut lines: Vec<Line> = Vec::new();

    for (i, h) in app.health_report.iter().enumerate().skip(scroll).take(visible) {
        let selected = i == app.health_cursor;

        let git_span = match h.git_dirty {
            Some(true)  => Span::styled(" ●dirty ", Style::new().fg(AMBER)),
            Some(false) => Span::styled(" ○clean ", Style::new().fg(GREEN)),
            None        => Span::styled(" -      ", Style::new().fg(DIM)),
        };

        let score_color = match h.compliance_score {
            Some(s) if s >= 80 => GREEN,
            Some(s) if s >= 60 => AMBER,
            Some(_) => RED,
            None => DIM,
        };
        let score_str = h.compliance_score
            .map(|s| format!("{:3}/100 {}", s, h.compliance_grade))
            .unwrap_or_else(|| "   -     ".into());

        let mem_span = if h.has_memory {
            Span::styled(" ✓mem", Style::new().fg(GREEN))
        } else {
            Span::styled(" ✗mem", Style::new().fg(RED))
        };

        let issues_span = if h.constitution_issues.is_empty() {
            Span::styled(" ✓ const", Style::new().fg(GREEN))
        } else {
            Span::styled(
                format!(" ⚠ {} issues", h.constitution_issues.len()),
                Style::new().fg(AMBER),
            )
        };

        let graph_span = if h.graphify_done {
            Span::styled(" ✓graph ", Style::new().fg(GREEN))
        } else {
            Span::styled(" ✗graph ", Style::new().fg(DIM))
        };

        let sc = project_status_color(&h.status);
        let name_color = if selected { GREEN } else { MID };
        let prefix = if selected { "  ▶ " } else { "    " };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::new().fg(GREEN)),
            Span::styled(format!("{:<28}", h.name), Style::new().fg(name_color).bold()),
            Span::styled(format!(" [{:<10}]", h.status), Style::new().fg(sc)),
            git_span,
            graph_span,
            Span::styled(score_str, Style::new().fg(score_color)),
            mem_span,
            issues_span,
        ]));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Summary footer
    let total = app.health_report.len();
    let dirty = app.health_report.iter().filter(|h| h.git_dirty == Some(true)).count();
    let no_mem = app.health_report.iter().filter(|h| !h.has_memory).count();
    let avg_score = if total > 0 {
        app.health_report.iter()
            .filter_map(|h| h.compliance_score)
            .sum::<u8>() as usize / total.max(1)
    } else { 0 };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {}/{} dirty  ", dirty, total),
            Style::new().fg(if dirty > 0 { AMBER } else { GREEN }),
        ),
        Span::styled(format!("avg score: {}/100  ", avg_score), Style::new().fg(MID)),
        Span::styled(format!("no memory.md: {}  ", no_mem), Style::new().fg(if no_mem > 0 { AMBER } else { DIM })),
        Span::styled("  [↑↓] nav  [Enter] open  [Esc] back", Style::new().fg(DIM)),
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

// ─── Command palette overlay ──────────────────────────────────────────────────

fn render_command_palette(frame: &mut Frame, app: &App) {
    let filtered = filtered_palette(&app.command_buf);
    if filtered.is_empty() {
        return;
    }

    let area = frame.area();
    let popup_h = (filtered.len() as u16 + 2).min(13);
    let popup_w = 68u16.min(area.width.saturating_sub(4));
    let popup = Rect {
        x: area.x + area.width.saturating_sub(popup_w) / 2,
        y: area.y + area.height.saturating_sub(popup_h + 3),
        width: popup_w,
        height: popup_h,
    };

    frame.render_widget(
        Block::new().style(Style::new().bg(Color::Rgb(12, 18, 24))),
        popup,
    );

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(GREEN))
        .title(Span::styled(" Commands — [↑↓] nav  [Tab] fill  [Enter] run ", Style::new().fg(DIM)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<Line> = filtered
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if i == app.palette_cursor {
                Line::from(vec![
                    Span::styled("  ▶ ", Style::new().fg(GREEN).bold()),
                    Span::styled(item.cmd, Style::new().fg(GREEN).bold()),
                    Span::styled(format!("  {}", item.desc), Style::new().fg(AMBER)),
                ])
            } else {
                Line::from(vec![
                    Span::styled("    ", Style::new()),
                    Span::styled(item.cmd, Style::new().fg(CYAN)),
                    Span::styled(format!("  {}", item.desc), Style::new().fg(DIM)),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(items)), inner);
}

// ─── File-changed notification badge ─────────────────────────────────────────

fn render_file_changed_badge(frame: &mut Frame, _app: &App) {
    let area = frame.area();
    let badge_w = 42u16;
    let badge = Rect {
        x: area.x + area.width.saturating_sub(badge_w + 2),
        y: area.y + 1,
        width: badge_w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            "  ⚡ File changed — [R] reload  ",
            Style::new().fg(Color::Black).bg(AMBER).bold(),
        )),
        badge,
    );
}

// ─── Agent launcher modal ─────────────────────────────────────────────────────

fn render_launcher_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = center_rect(36, 8, area);

    frame.render_widget(
        Block::new().style(Style::new().bg(Color::Rgb(12, 18, 24))),
        popup,
    );

    let proj_name = app
        .active_project
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CYAN))
        .title(Span::styled(" Launch Agent ", Style::new().fg(CYAN).bold()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Project: {}", proj_name),
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [C] ", Style::new().fg(GREEN).bold()),
            Span::styled("Claude Code", Style::new().fg(MID)),
        ]),
        Line::from(vec![
            Span::styled("  [G] ", Style::new().fg(CYAN).bold()),
            Span::styled("Gemini CLI", Style::new().fg(MID)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [Esc] Cancel", Style::new().fg(DIM))),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn center_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let x = parent.x + parent.width.saturating_sub(width) / 2;
    let y = parent.y + parent.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(parent.width),
        height: height.min(parent.height),
    }
}

fn render_graph_report(frame: &mut Frame, app: &App) {
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
