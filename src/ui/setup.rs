use crate::app::App;
use crate::setup_wizard::WizardStep;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

const ACCENT: Color = Color::Rgb(0, 220, 130);
const PANEL: Color = Color::Rgb(8, 14, 10);
const DIM_B: Color = Color::Rgb(30, 50, 35);

pub fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(area);

    render_header(frame, header, app);
    render_body(frame, body, app);
    render_footer(frame, footer, app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let step = app.wizard.step.index();
    let total = WizardStep::total();
    let pct = (step * 100 / total.max(1)) as u16;

    let [title_a, bar_a] =
        Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).areas(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  R-AI-OS ", Style::new().fg(ACCENT).bold()),
            Span::styled("SETUP WIZARD  ", Style::new().fg(DIM)),
            Span::styled(format!("[{}/{}] ", step, total), Style::new().fg(DIM)),
            Span::styled(app.wizard.step.title(), Style::new().fg(AMBER).bold()),
        ])),
        title_a,
    );

    frame.render_widget(
        Gauge::default()
            .block(Block::new().borders(Borders::NONE))
            .gauge_style(Style::new().fg(ACCENT).bg(Color::Rgb(20, 35, 25)))
            .percent(pct)
            .label(format!("{}%", pct)),
        bar_a,
    );
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    match app.wizard.step {
        WizardStep::Welcome => render_welcome(frame, area, app),
        WizardStep::Workspace => render_workspace(frame, area, app),
        WizardStep::Constitution => render_master(frame, area, app),
        WizardStep::Claude => render_agent(
            frame,
            area,
            app,
            "CLAUDE CODE",
            app.wizard.skip_claude,
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.claude_installed)
                .unwrap_or(false),
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.claude_version.as_str())
                .unwrap_or(""),
            "https://claude.ai/code",
            &[
                "~/.claude/CLAUDE.md",
                "~/.claude/settings.json (MCP)",
                "~/.claude/rules/",
                ".agents/skills/",
            ],
        ),

        WizardStep::Codex => render_agent(
            frame,
            area,
            app,
            "CODEX KAIRA",
            app.wizard.skip_antigravity,
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.codex_installed)
                .unwrap_or(false),
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.codex_version.as_str())
                .unwrap_or(""),
            "https://openai.com/codex",
            &["~/.codex/AGENTS.md", "~/AGENTS.md (symlink)"],
        ),
        WizardStep::OpenCode => render_agent(
            frame,
            area,
            app,
            "OPENCODE",
            app.wizard.skip_opencode,
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.opencode_installed)
                .unwrap_or(false),
            app.wizard
                .agent_status
                .as_ref()
                .map(|s| s.opencode_version.as_str())
                .unwrap_or(""),
            "https://opencode.ai",
            &[
                "~/.config/opencode/opencode.jsonc (MCP)",
            ],
        ),
        WizardStep::Skills => render_skills(frame, area, app),
        WizardStep::Initialize => render_initialize(frame, area, app),
        WizardStep::Done => render_done(frame, area, app),
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hint = match &app.wizard.step {
        WizardStep::Welcome => " [Enter] Başla  [q] Çık ",
        WizardStep::Done => " [Enter] Dashboard'ı Aç ",
        WizardStep::Initialize if app.wizard.running => " Kurulum çalışıyor... ",
        WizardStep::Initialize => " [Enter] Kurulumu Başlat  [q] Çık ",
        _ if app.wizard.editing => " [Enter] Onayla  [Esc] İptal ",
        _ => " [Enter] Düzenle  [s] İleri  [Tab] Ajanı Atla  [↑↓] Alan  [q] Çık ",
    };
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM_B));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::new().fg(DIM)))),
        inner,
    );
}

// ─── Steps ───────────────────────────────────────────────────────────────────

fn render_welcome(frame: &mut Frame, area: Rect, app: &App) {
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

    // System scan
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

fn render_workspace(frame: &mut Frame, area: Rect, app: &App) {
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
                Style::new().fg(GREEN).bg(Color::Rgb(0, 25, 10))
            } else if val.is_empty() {
                Style::new().fg(Color::Rgb(120, 60, 60)).italic()
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
        // Scan actual directory — show up to 12 entries
        let mut entries: Vec<String> = std::fs::read_dir(dev_ops_path)
            .map(|rd| {
                let mut v: Vec<String> = rd
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') { None } else { Some(name) }
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
        // Show K-AI-RA constitution-defined structure
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

fn render_master(frame: &mut Frame, area: Rect, app: &App) {
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
            Style::new().fg(Color::Rgb(100, 130, 110)),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

#[allow(clippy::too_many_arguments)]
fn render_agent(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    name: &str,
    skipped: bool,
    installed: bool,
    version: &str,
    url: &str,
    will_create: &[&str],
) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let (s_text, s_col) = if skipped {
        ("ATLANDI", DIM)
    } else if installed {
        ("KURULU", GREEN)
    } else {
        ("KURULU DEĞİL", Color::Rgb(200, 60, 60))
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("  {} ", name), Style::new().fg(ACCENT).bold()),
            Span::styled(format!(" {} ", s_text), Style::new().fg(s_col).bold()),
        ]),
        Line::from(""),
    ];

    if installed {
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", Style::new().fg(GREEN)),
            Span::styled(version, Style::new().fg(DIM)),
        ]));
    } else {
        lines.push(Line::from(Span::styled("  Kurulum:", Style::new().fg(DIM))));
        lines.push(Line::from(Span::styled(
            format!("  → {}", url),
            Style::new().fg(CYAN),
        )));
    }

    lines.push(Line::from(""));
    if skipped {
        lines.push(Line::from(Span::styled(
            "  Bu adım atlandı. [Tab] ile geri al.",
            Style::new().fg(DIM),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Oluşturulacaklar:",
            Style::new().fg(DIM),
        )));
        lines.push(Line::from(""));
        for item in will_create {
            lines.push(Line::from(vec![
                Span::styled("  + ", Style::new().fg(ACCENT)),
                Span::styled(*item, Style::new().fg(MID)),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

fn render_skills(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let mut lines = vec![
        Line::from(Span::styled(
            "  SKILLS & HOOKS",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Tüm ajanlar tarafından paylaşılan skill ve hook dizinleri.",
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled("  Oluşturulacaklar:", Style::new().fg(DIM))),
        Line::from(""),
    ];
    for (path, desc) in [
        (".agents/skills/prompt-master.md",      "Prompt optimizasyon"),
        (".agents/skills/graphify.md",           "Mimari haritalama"),
        (".agents/skills/search-first.md",       "Koddan önce araştır"),
        (".agents/skills/ki-snapshot.md",        "Session özeti"),
        (".agents/skills/continuous-learning.md","Instinct kaydı"),
        (".agents/skills/verify-ai-os.md",       "Sistem sağlığı"),
        (".agents/hooks/README.md",              "Hook dokümantasyonu"),
    ] {
        lines.push(Line::from(vec![
            Span::styled("  + ", Style::new().fg(ACCENT)),
            Span::styled(format!("{:<36}", path), Style::new().fg(MID)),
            Span::styled(desc, Style::new().fg(DIM)),
        ]));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

fn render_initialize(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    let mut lines = vec![
        Line::from(Span::styled(
            "  HAZIR — KURULUM ÖZETİ",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    if app.wizard.running {
        lines.push(Line::from(Span::styled(
            "  ⚡ Kurulum çalışıyor...",
            Style::new().fg(AMBER).bold(),
        )));
    } else {
        for (label, val) in [
            ("Dev_Ops", app.wizard.dev_ops.as_str()),
            ("CONSTITUTION", app.wizard.master.as_str()),
            ("GitHub", app.wizard.github.as_str()),
        ] {
            let (disp, col) = if val.is_empty() {
                ("(atlandı)".to_string(), DIM)
            } else {
                (val.chars().take(38).collect(), GREEN)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<12}", label), Style::new().fg(DIM)),
                Span::styled(disp, Style::new().fg(col)),
            ]));
        }
        lines.push(Line::from(""));
        for (name, active) in [
            ("Claude Code", !app.wizard.skip_claude),
            ("Antigravity", !app.wizard.skip_antigravity),
            ("OpenCode", !app.wizard.skip_opencode),
        ] {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", name), Style::new().fg(DIM)),
                if active {
                    Span::styled("✓ aktif", Style::new().fg(GREEN))
                } else {
                    Span::styled("⊘ atlandı", Style::new().fg(DIM))
                },
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  [Enter] → Kurulumu Başlat",
            Style::new().fg(ACCENT).bold(),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

fn render_done(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let ok = app
        .wizard
        .action_log
        .iter()
        .filter(|a| a.ok && !a.skipped)
        .count();
    let skip = app.wizard.action_log.iter().filter(|a| a.skipped).count();
    let fail = app.wizard.action_log.iter().filter(|a| !a.ok).count();

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ✓ KURULUM TAMAMLANDI",
            Style::new().fg(ACCENT).bold().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} ", ok), Style::new().fg(GREEN)),
            Span::styled("işlem başarılı", Style::new().fg(MID)),
        ]),
        Line::from(vec![
            Span::styled(format!("  {} ", skip), Style::new().fg(DIM)),
            Span::styled("adım atlandı (zaten vardı)", Style::new().fg(DIM)),
        ]),
    ];
    if fail > 0 {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", fail), Style::new().fg(RED)),
            Span::styled("hata — sağda detay", Style::new().fg(DIM)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Sıradakiler:",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(""));
    for (n, text) in [
        ("1.", "Claude Code'u yeniden başlat (MCP aktifleşir)"),
        ("2.", "raios health   — proje sağlık raporu"),
        ("3.", "raios new <ad> — ilk projeyi oluştur"),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", n), Style::new().fg(ACCENT)),
            Span::styled(text, Style::new().fg(MID)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [Enter] → Dashboard",
        Style::new().fg(ACCENT).bold(),
    )));
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

fn render_log(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(DIM_B))
        .title(Span::styled(" LOG ", Style::new().fg(DIM)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible = inner.height as usize;
    let log = &app.wizard.action_log;
    let scroll = log.len().saturating_sub(visible);
    let max_w = inner.width.saturating_sub(4) as usize;

    let lines: Vec<Line> = log
        .iter()
        .skip(scroll)
        .map(|a| {
            let (icon, col) = if a.skipped {
                ("·", DIM)
            } else if a.ok {
                ("✓", GREEN)
            } else {
                ("✗", RED)
            };
            Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::new().fg(col)),
                Span::styled(
                    a.desc.chars().take(max_w).collect::<String>(),
                    Style::new().fg(if a.ok {
                        MID
                    } else if a.skipped {
                        DIM
                    } else {
                        RED
                    }),
                ),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

const MASTER_PREVIEW: &str = "# AGENT CONSTITUTION (v5.0)\n# K-AI-RA — Single source of truth\n\n## Identity\n- Claude Kaira  |  Codex Kaira\n\n## RIPER-5\n1. Requirement  2. Investigation\n3. Planning     4. Execution\n5. Review & Refactor\n\n## AgentShield (OWASP)\n- No client-side secrets\n- Parameterized queries only\n- pnpm audit on every commit\n\n## Skills\nraios · search-first · graphify\nprompt-master · ki-snapshot";
