use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect, Alignment},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, AppState, MENU_ITEMS};
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
        AppState::Booting => render_boot(frame, app),
        AppState::Dashboard => render_dashboard(frame, app),
        AppState::FileView => render_file_view(frame, app),
        AppState::FileEdit => render_file_edit(frame, app),
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
        " {}  Initializing R-AI-OS Core...",
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
    let version_tag = Span::styled(" v0.1 (Rust) ", Style::new().fg(DIM));
    let title = Span::styled(
        "  R-AI-OS — CORE SYSTEM  ",
        Style::new()
            .fg(GREEN)
            .bg(HEADER_BG)
            .add_modifier(Modifier::BOLD),
    );
    let sync_tag = if app.is_syncing {
        Span::styled(" ⚡ SYNCING... ", Style::new().fg(AMBER).add_modifier(Modifier::BOLD))
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
        _ => {}
    }
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
        lines.push(Line::from(vec![
            Span::styled(" 📁 ", Style::new().fg(CYAN)),
            Span::styled(proj.name.as_str(), Style::new().fg(CYAN).bold()),
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

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_agents(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(" DISCOVERED AGENTS", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    if app.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No agents detected",
            Style::new().fg(DIM),
        )));
    } else {
        for agent in &app.agents {
            let (mark, color) = if agent.exists() { ("●", CYAN) } else { ("○", DIM) };
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", mark), Style::new().fg(color).bold()),
                Span::styled(agent.name, Style::new().fg(CYAN).bold()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("    ", Style::new()),
                Span::styled(agent.path.to_string_lossy().to_string(), Style::new().fg(DIM)),
            ]));
            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(Span::styled(
        " INSTALLED SKILLS",
        Style::new().fg(MID).bold(),
    )));
    lines.push(Line::from(""));

    if app.skills.is_empty() {
        lines.push(Line::from(Span::styled("  None found", Style::new().fg(DIM))));
    } else {
        let skill_line: Vec<Span> = app
            .skills
            .iter()
            .flat_map(|s| {
                let color = if s.category == "Global" { Color::Yellow } else { Color::Magenta };
                vec![
                    Span::styled(format!("◈ {} ", s.name), Style::new().fg(color)),
                ]
            })
            .collect();
        lines.push(Line::from(skill_line));
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
    let _ = app;
    let lines = vec![
        Line::from(Span::styled(" MEMPALACE & PROJECT MEMORY", Style::new().fg(MID).bold())),
        Line::from(""),
        Line::from(Span::styled(
            "  Base: C:\\Users\\turha\\Desktop\\Dev Ops",
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Select a file below →",
            Style::new().fg(DIM).italic(),
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
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
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(file.name.as_str(), Style::new().fg(GREEN).bold()),
            Span::styled(ro_tag, Style::new().fg(AMBER)),
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

    // Content with line numbers
    let scroll = app.file_scroll as usize;
    let visible_h = content_area.height as usize;
    let lines: Vec<Line> = app
        .file_lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
        .map(|(i, line)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>4} │ ", i + 1),
                    Style::new().fg(DIM),
                ),
                Span::styled(syntax_highlight(line), Style::new().fg(MID)),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Footer
    let total = app.file_lines.len();
    let pct = if total > 0 {
        ((scroll + visible_h).min(total) * 100 / total).min(100)
    } else {
        100
    };
    let edit_hint = if !file.read_only { "  [e] edit" } else { "" };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" Ln {}/{} ({}%)", scroll + 1, total, pct),
            Style::new().fg(DIM),
        ),
        Span::styled(
            format!("   [↑↓/PgUp/PgDn] scroll{}  [Esc/q] back", edit_hint),
            Style::new().fg(DIM),
        ),
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

// Minimal syntax highlight: color markdown headers, bullets, code blocks
fn syntax_highlight(line: &str) -> &str {
    line // passthrough — colour applied uniformly per-line above
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
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ✏ ", Style::new().fg(AMBER)),
            Span::styled(file.name.as_str(), Style::new().fg(AMBER).bold()),
            Span::styled("  ", Style::new()),
            Span::styled("EDITING", Style::new().fg(Color::White).bg(Color::Rgb(60, 20, 0)).bold()),
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
