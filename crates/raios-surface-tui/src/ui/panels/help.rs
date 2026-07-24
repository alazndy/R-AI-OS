use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Full-screen help view (AppState::HelpView) — opened with `?` or `/help`.
pub fn render_help_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(frame, header_area, app);
    render_help(frame, body_area, app);

    let footer = Paragraph::new(Span::styled(
        "  press any key to return",
        Style::new().fg(DIM).italic(),
    ));
    frame.render_widget(footer, footer_area);
}

fn key_line(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("    {:<14} ", key), Style::new().fg(AMBER)),
        Span::raw(desc),
    ])
}

fn section(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", title),
        Style::new().fg(GREEN).bold(),
    ))
}

pub fn render_help(frame: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let title = Paragraph::new(Line::from(vec![Span::styled(
        format!(
            "  R-AI-OS v{} — KEYS & COMMANDS  ",
            env!("CARGO_PKG_VERSION")
        ),
        Style::new().fg(CYAN).bold(),
    )]))
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(title, chunks[0]);

    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
            .areas(chunks[1]);

    let keys_text = vec![
        Line::from(""),
        section("GLOBAL"),
        key_line("?", "Open this help screen"),
        key_line("q", "Quit (dashboard) / back (sub-views)"),
        key_line("Ctrl + C", "Hard exit"),
        key_line("Ctrl + P", "Neural fuzzy search (global)"),
        key_line("/", "Open Command Center"),
        key_line(
            "Mouse",
            "Click tabs/items; wheel navigates; click bottom bar for commands",
        ),
        key_line("1–4 / Tab", "Switch NOW, WORK, EXPLORE, and GOVERN routes"),
        key_line("↑↓ / j k", "Move through menu & lists"),
        key_line("→ / l", "Focus right panel"),
        key_line("← / h", "Back to menu"),
        key_line("Enter", "Select / open"),
        key_line("Esc", "Back / cancel"),
        Line::from(""),
        section("FILE PANELS (Rules · Agents · Policies · MemPalace)"),
        key_line("Enter", "View file"),
        key_line("e", "Edit in built-in editor"),
        key_line("o", "Open in VS Code"),
        Line::from(""),
        section("TASKS (Recent panel focused)"),
        key_line("Space / v", "Toggle task done"),
        key_line("c / x / o / a", "Send task → Claude/Codex/OpenCode/Agy"),
        Line::from(""),
        section("ALL PROJECTS"),
        key_line("Enter", "Open project detail"),
        key_line("s", "Cycle sort mode"),
        key_line("L", "Agent launcher for selected project"),
        key_line("C / O / A", "Quick-launch Claude/OpenCode/Agy"),
        Line::from(""),
        section("PROJECT DETAIL"),
        key_line("e", "Edit memory.md"),
        key_line("g / r", "Run graphify / view report"),
        key_line("d", "Git diff"),
        key_line("l", "Agent launcher"),
        Line::from(""),
        section("EXTENSIONS (→ to focus, Esc to leave)"),
        key_line("Tab", "Switch Commands / Config"),
        key_line("← →", "Switch selected extension"),
        key_line("Enter", "Run command / save config field"),
        key_line("e", "Edit config field"),
        Line::from(""),
        section("EDITOR"),
        key_line("Ctrl + S", "Save file"),
        key_line("Esc", "Exit without saving"),
    ];

    let cmds_text = vec![
        Line::from(""),
        section("COMMANDS (type / to open the palette)"),
        key_line("/now", "Approvals, blockers, and active runs"),
        key_line("/work", "Projects, tasks, and artifacts"),
        key_line("/explore", "Search, traces, and daemon logs"),
        key_line("/govern", "Policies, audit ledger, and scheduler"),
        key_line("/refresh", "Refresh the control-plane snapshot"),
        key_line("/sync", "Sync all agents with MASTER.md"),
        key_line("/discover", "Rescan workspace projects"),
        key_line("/health", "Open Health Dashboard"),
        key_line("/reindex", "Rebuild neural search index"),
        key_line("/search <q>", "Neural search"),
        key_line("/open <proj>", "Jump to project detail"),
        key_line("/view <file>", "View any known file"),
        key_line("/edit <file>", "Edit any known file"),
        key_line("/memo <text>", "Append quick session note"),
        key_line("/task add <t>", "Add task (@agent #project)"),
        key_line("/task send <a>", "Dispatch top task to agent"),
        key_line("/timeline", "Activity timeline"),
        key_line("/logs [n]", "Live daemon logs"),
        key_line("/audit", "AI system audit scan"),
        key_line("/mempalace", "MemPalace full-screen view"),
        key_line("/ext", "Extensions panel"),
        key_line("/graphify", "Knowledge graph (needs project)"),
        key_line("/heal", "Sentinel self-correction (project)"),
        key_line("/rules", "Jump to System Rules"),
        key_line("/memory", "Jump to MemPalace files"),
        key_line("/vault-create <p>", "Create Obsidian vault note"),
        key_line("/run <cmd>", "Remote hub: run raios command"),
        key_line("/q", "Quit"),
        Line::from(""),
        section("PANEL-SPECIFIC"),
        key_line("f", "System Core: compliance auto-fix"),
        key_line("i", "Inbox pending: view file-change diff"),
    ];

    frame.render_widget(
        Paragraph::new(keys_text).wrap(Wrap { trim: false }),
        left_area,
    );
    frame.render_widget(
        Paragraph::new(cmds_text).wrap(Wrap { trim: false }),
        right_area,
    );
}
