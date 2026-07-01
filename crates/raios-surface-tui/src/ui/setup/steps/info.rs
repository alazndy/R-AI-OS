use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};
use super::{ACCENT, MASTER_PREVIEW};

pub fn render_welcome(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ██████╗      █████╗  ██╗      ██████╗  ███████╗",
            Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ██╔══██╗    ██╔══██╗ ██║     ██╔═══██╗ ██╔════╝",
            Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ██████╔╝    ███████║ ██║     ██║   ██║ ███████╗",
            Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ██╔══██╗    ██╔══██║ ██║     ██║   ██║ ╚════██║",
            Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ██║  ██║    ██║  ██║ ███████╗╚██████╔╝ ███████║",
            Style::new().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  ╚═╝  ╚═╝    ╚═╝  ╚═╝ ╚══════╝ ╚═════╝  ╚══════╝",
            Style::new().fg(ACCENT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  v{}  —  İlk Kurulum", env!("CARGO_PKG_VERSION")),
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Bu sihirbaz sıfırdan tam kurulum yapar:",
            Style::new().fg(MID),
        )),
        Line::from(""),
    ];
    for item in &[
        "AGENT_CONSTITUTION.md — K-AI-RA unified constitution",
        "Workspace symlinks: CLAUDE.md, AGENTS.md",
        "Claude Kaira:       ~/.claude/CLAUDE.md + MCP",
        "Codex Kaira:        ~/.codex/AGENTS.md",
        "Skills & Hooks:     6 K-AI-RA skill stubs",
        "İlk proje keşfi → entities.json",
    ] {
        lines.push(Line::from(vec![
            Span::styled("  ◈ ", Style::new().fg(ACCENT)),
            Span::styled(*item, Style::new().fg(MID)),
        ]));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let mut r = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  SİSTEM TARAMASI",
            Style::new().fg(DIM).bold(),
        )),
        Line::from(""),
    ];
    if let Some(s) = &app.wizard.agent_status {
        for (name, ok, ver) in [
            ("git", s.git_installed, s.git_version.as_str()),
            ("gh (GitHub)", s.gh_installed, s.gh_version.as_str()),
            ("Claude Code", s.claude_installed, s.claude_version.as_str()),
            ("Codex", s.codex_installed, s.codex_version.as_str()),
            ("OpenCode", s.opencode_installed, s.opencode_version.as_str()),
            ("AGY (Antigravity)", s.agy_installed, s.agy_version.as_str()),
        ] {
            let (icon, col) = if ok {
                ("✓", GREEN)
            } else {
                ("✗", Color::Rgb(180, 60, 60))
            };
            r.push(Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::new().fg(col).bold()),
                Span::styled(
                    format!("{:<20}", name),
                    Style::new().fg(if ok { MID } else { DIM }),
                ),
                Span::styled(
                    ver.chars().take(24).collect::<String>(),
                    Style::new().fg(DIM),
                ),
            ]));
        }
    } else {
        r.push(Line::from(Span::styled(
            "  taranıyor...",
            Style::new().fg(DIM).italic(),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

pub fn render_workspace(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let fields: &[(&str, &str, &str)] = &[
        (
            "Dev_Ops Path",
            "Tüm projelerin kök dizini (zorunlu)",
            &app.wizard.dev_ops,
        ),
        (
            "GitHub Username",
            "GitHub kullanıcı adı (opsiyonel)",
            &app.wizard.github,
        ),
        (
            "Vault Projects",
            "Obsidian Vault Projeler klasörü (opsiyonel)",
            &app.wizard.vault,
        ),
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "  WORKSPACE KURULUMU",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    for (i, (label, hint, val)) in fields.iter().enumerate() {
        let sel = i == app.wizard.field_cursor;
        let edit = sel && app.wizard.editing;
        let disp = if edit {
            format!("  {}█", app.wizard.input)
        } else if val.is_empty() {
            format!(
                "  (boş{} — Enter ile düzenle)",
                if i == 0 { ", zorunlu" } else { "" }
            )
        } else {
            format!("  ✓ {}", val)
        };

        lines.push(Line::from(vec![
            Span::styled(
                if sel { "  ▶ " } else { "    " },
                Style::new().fg(ACCENT).bold(),
            ),
            Span::styled(
                *label,
                Style::new().fg(if sel { ACCENT } else { MID }).bold(),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("      {}", hint),
            Style::new().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            disp,
            if edit {
                Style::new().fg(GREEN).bg(Color::Rgb(0, 25, 45))
            } else if val.is_empty() {
                Style::new().fg(Color::Rgb(200, 80, 50)).italic()
            } else {
                Style::new().fg(GREEN)
            },
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let dev_ops_path = std::path::Path::new(&app.wizard.dev_ops);
    let path_exists = !app.wizard.dev_ops.is_empty() && dev_ops_path.is_dir();
    let base = if app.wizard.dev_ops.is_empty() {
        "~/dev".to_string()
    } else {
        app.wizard.dev_ops.clone()
    };

    let header_label = if path_exists {
        "  MEVCUT YAPI"
    } else {
        "  OLUŞTURULACAK YAPI (K-AI-RA)"
    };

    let mut r = vec![
        Line::from(Span::styled(header_label, Style::new().fg(DIM).bold())),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}/", base),
            Style::new().fg(ACCENT).bold(),
        )),
    ];

    if path_exists {
        let mut entries: Vec<String> = std::fs::read_dir(dev_ops_path)
            .map(|rd| {
                let mut v: Vec<String> = rd
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') {
                            None
                        } else {
                            Some(name)
                        }
                    })
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
        entries.truncate(12);
        let total = entries.len();
        for (i, name) in entries.iter().enumerate() {
            let is_last = i == total - 1;
            let prefix = if is_last { "└── " } else { "├── " };
            let is_file = dev_ops_path.join(name).is_file();
            r.push(Line::from(Span::styled(
                format!("  {}{}", prefix, name),
                Style::new().fg(if is_file { CYAN } else { MID }),
            )));
        }
        r.push(Line::from(""));
        r.push(Line::from(Span::styled(
            "  + entities.json  tasks.md  mempalace.yaml",
            Style::new().fg(CYAN),
        )));
    } else {
        for (item, file) in [
            ("├── ai/", false),
            ("├── embedded/", false),
            ("├── web/", false),
            ("├── tools/", false),
            ("├── entities.json", true),
            ("├── tasks.md", true),
            ("└── mempalace.yaml", true),
        ] {
            r.push(Line::from(Span::styled(
                format!("  {}", item),
                Style::new().fg(if file { CYAN } else { MID }),
            )));
        }
        r.push(Line::from(""));
        r.push(Line::from(Span::styled(
            "  Kategoriler AGENT_CONSTITUTION'dan",
            Style::new().fg(DIM).italic(),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

pub fn render_master(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let val = &app.wizard.master;
    let edit = app.wizard.editing;
    let exists = !val.is_empty() && std::path::Path::new(val).exists();

    let disp = if edit {
        format!("  {}█", app.wizard.input)
    } else if val.is_empty() {
        "  (opsiyonel — [s] ile atla)".into()
    } else if exists {
        format!("  ✓ {} (mevcut)", val)
    } else {
        format!("  + {} (oluşturulacak)", val)
    };

    let lines = vec![
        Line::from(Span::styled(
            "  K-AI-RA — AGENT CONSTITUTION",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Tüm AI ajanların tek kaynağı (Claude, Codex).",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "  CLAUDE.md, AGENTS.md bu dosyaya symlink olur.",
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ▶ ", Style::new().fg(ACCENT)),
            Span::styled("AGENT_CONSTITUTION.md Path", Style::new().fg(ACCENT).bold()),
        ]),
        Line::from(Span::styled(
            "      Mevcut yol veya oluşturulacak konum",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            disp,
            if edit {
                Style::new().fg(GREEN).bg(Color::Rgb(0, 25, 10))
            } else if val.is_empty() {
                Style::new().fg(DIM)
            } else if exists {
                Style::new().fg(GREEN)
            } else {
                Style::new().fg(AMBER)
            },
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let mut r = vec![
        Line::from(Span::styled(
            "  TEMPLATE ÖNİZLEME",
            Style::new().fg(DIM).bold(),
        )),
        Line::from(""),
    ];
    for l in MASTER_PREVIEW.lines() {
        r.push(Line::from(Span::styled(
            format!("  {}", l),
            Style::new().fg(Color::Rgb(100, 120, 140)),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}
