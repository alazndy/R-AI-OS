use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, Paragraph},
};
use crate::app::App;
use crate::setup_wizard::WizardStep;
use crate::ui::*;

const ACCENT: Color = Color::Rgb(0, 220, 130);
const PANEL: Color  = Color::Rgb(8, 14, 10);
const DIM_B: Color  = Color::Rgb(30, 50, 35);

pub fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL)), area);

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(3),
    ]).areas(area);

    render_header(frame, header, app);
    render_body(frame, body, app);
    render_footer(frame, footer, app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let step  = app.wizard_step.index();
    let total = WizardStep::total();
    let pct   = (step * 100 / total.max(1)) as u16;

    let [title_a, bar_a] = Layout::vertical([
        Constraint::Length(2), Constraint::Length(2),
    ]).areas(area);

    frame.render_widget(Paragraph::new(Line::from(vec![
        Span::styled("  R-AI-OS ", Style::new().fg(ACCENT).bold()),
        Span::styled("SETUP WIZARD  ", Style::new().fg(DIM)),
        Span::styled(format!("[{}/{}] ", step, total), Style::new().fg(DIM)),
        Span::styled(app.wizard_step.title(), Style::new().fg(AMBER).bold()),
    ])), title_a);

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
    match app.wizard_step {
        WizardStep::Welcome     => render_welcome(frame, area, app),
        WizardStep::Workspace   => render_workspace(frame, area, app),
        WizardStep::Master      => render_master(frame, area, app),
        WizardStep::Claude      => render_agent(frame, area, app, "CLAUDE CODE",
            app.wizard_skip_claude,
            app.wizard_agent_status.as_ref().map(|s| s.claude_installed).unwrap_or(false),
            app.wizard_agent_status.as_ref().map(|s| s.claude_version.as_str()).unwrap_or(""),
            "https://claude.ai/code",
            &["~/.claude/CLAUDE.md", "~/.claude/settings.json (MCP)", "~/.claude/rules/", ".agents/skills/"],
        ),
        WizardStep::Gemini      => render_agent(frame, area, app, "GEMINI CLI",
            app.wizard_skip_gemini,
            app.wizard_agent_status.as_ref().map(|s| s.gemini_installed).unwrap_or(false),
            app.wizard_agent_status.as_ref().map(|s| s.gemini_version.as_str()).unwrap_or(""),
            "https://ai.google.dev/gemini-api/docs/gemini-cli",
            &["~/.gemini/GEMINI.md", "~/.gemini/settings.json (MCP)"],
        ),
        WizardStep::Antigravity => render_agent(frame, area, app, "ANTIGRAVITY",
            app.wizard_skip_antigravity,
            app.wizard_agent_status.as_ref().map(|s| s.antigravity_installed).unwrap_or(false),
            app.wizard_agent_status.as_ref().map(|s| s.antigravity_version.as_str()).unwrap_or(""),
            "https://antigravity.dev",
            &[".agents/ANTIGRAVITY.md"],
        ),
        WizardStep::Skills      => render_skills(frame, area, app),
        WizardStep::Initialize  => render_initialize(frame, area, app),
        WizardStep::Done        => render_done(frame, area, app),
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hint = match &app.wizard_step {
        WizardStep::Welcome                          => " [Enter] Başla  [q] Çık ",
        WizardStep::Done                             => " [Enter] Dashboard'ı Aç ",
        WizardStep::Initialize if app.wizard_running => " Kurulum çalışıyor... ",
        WizardStep::Initialize                       => " [Enter] Kurulumu Başlat  [q] Çık ",
        _ if app.wizard_editing                      => " [Enter] Onayla  [Esc] İptal ",
        _                                            => " [Enter] Düzenle  [s] İleri  [Tab] Ajanı Atla  [↑↓] Alan  [q] Çık ",
    };
    let block = Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM_B));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Line::from(Span::styled(hint, Style::new().fg(DIM)))), inner);
}

// ─── Steps ───────────────────────────────────────────────────────────────────

fn render_welcome(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  ██████╗      █████╗  ██╗      ██████╗  ███████╗", Style::new().fg(ACCENT))),
        Line::from(Span::styled("  ██╔══██╗    ██╔══██╗ ██║     ██╔═══██╗ ██╔════╝", Style::new().fg(ACCENT))),
        Line::from(Span::styled("  ██████╔╝    ███████║ ██║     ██║   ██║ ███████╗", Style::new().fg(ACCENT))),
        Line::from(Span::styled("  ██╔══██╗    ██╔══██║ ██║     ██║   ██║ ╚════██║", Style::new().fg(ACCENT))),
        Line::from(Span::styled("  ██║  ██║    ██║  ██║ ███████╗╚██████╔╝ ███████║", Style::new().fg(ACCENT))),
        Line::from(Span::styled("  ╚═╝  ╚═╝    ╚═╝  ╚═╝ ╚══════╝ ╚═════╝  ╚══════╝", Style::new().fg(ACCENT))),
        Line::from(""),
        Line::from(Span::styled(format!("  v{}  —  İlk Kurulum", env!("CARGO_PKG_VERSION")), Style::new().fg(DIM))),
        Line::from(""),
        Line::from(Span::styled("  Bu sihirbaz sıfırdan tam kurulum yapar:", Style::new().fg(MID))),
        Line::from(""),
    ];
    for item in &[
        "Workspace dizin yapısı (Dev_Ops + kategoriler)",
        "MASTER.md — agent constitution template",
        "Claude Code: CLAUDE.md + MCP kaydı + rules/",
        "Gemini CLI:  GEMINI.md + MCP kaydı",
        "Antigravity: ANTIGRAVITY.md",
        "Skills & Hooks dizinleri + starter dosyalar",
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
        Line::from(Span::styled("  SİSTEM TARAMASI", Style::new().fg(DIM).bold())),
        Line::from(""),
    ];
    if let Some(s) = &app.wizard_agent_status {
        for (name, ok, ver) in [
            ("git",           s.git_installed,          s.git_version.as_str()),
            ("gh (GitHub)",   s.gh_installed,            s.gh_version.as_str()),
            ("Claude Code",   s.claude_installed,        s.claude_version.as_str()),
            ("Gemini CLI",    s.gemini_installed,        s.gemini_version.as_str()),
            ("Antigravity",   s.antigravity_installed,   s.antigravity_version.as_str()),
        ] {
            let (icon, col) = if ok { ("✓", GREEN) } else { ("✗", Color::Rgb(180,60,60)) };
            r.push(Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::new().fg(col).bold()),
                Span::styled(format!("{:<20}", name), Style::new().fg(if ok { MID } else { DIM })),
                Span::styled(ver.chars().take(24).collect::<String>(), Style::new().fg(DIM)),
            ]));
        }
    } else {
        r.push(Line::from(Span::styled("  taranıyor...", Style::new().fg(DIM).italic())));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

fn render_workspace(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let fields: &[(&str, &str, &str)] = &[
        ("Dev_Ops Path",    "Tüm projelerin kök dizini (zorunlu)",          &app.wizard_dev_ops),
        ("GitHub Username", "GitHub kullanıcı adı (opsiyonel)",             &app.wizard_github),
        ("Vault Projects",  "Obsidian Vault Projeler klasörü (opsiyonel)",  &app.wizard_vault),
    ];

    let mut lines = vec![
        Line::from(Span::styled("  WORKSPACE KURULUMU", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    for (i, (label, hint, val)) in fields.iter().enumerate() {
        let sel  = i == app.wizard_field_cursor;
        let edit = sel && app.wizard_editing;
        let disp = if edit {
            format!("  {}█", app.wizard_input)
        } else if val.is_empty() {
            format!("  (boş{} — Enter ile düzenle)", if i == 0 { ", zorunlu" } else { "" })
        } else {
            format!("  ✓ {}", val)
        };

        lines.push(Line::from(vec![
            Span::styled(if sel { "  ▶ " } else { "    " }, Style::new().fg(ACCENT).bold()),
            Span::styled(*label, Style::new().fg(if sel { ACCENT } else { MID }).bold()),
        ]));
        lines.push(Line::from(Span::styled(format!("      {}", hint), Style::new().fg(DIM))));
        lines.push(Line::from(Span::styled(
            disp,
            if edit { Style::new().fg(GREEN).bg(Color::Rgb(0,25,10)) }
            else if val.is_empty() { Style::new().fg(Color::Rgb(120,60,60)).italic() }
            else { Style::new().fg(GREEN) },
        )));
        lines.push(Line::from(""));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let base = if app.wizard_dev_ops.is_empty() { "Dev_Ops" }
               else { app.wizard_dev_ops.split(['/', '\\']).last().unwrap_or("Dev_Ops") };
    let mut r = vec![
        Line::from(Span::styled("  OLUŞTURULACAK YAPI", Style::new().fg(DIM).bold())),
        Line::from(""),
        Line::from(Span::styled(format!("  {}/", base), Style::new().fg(ACCENT).bold())),
    ];
    for (item, file) in [
        ("├── 00_System/", false), ("├── 01_Hardware_&_Embedded/", false),
        ("├── 02_AI_&_Data/", false), ("├── 03_Core_Libraries/", false),
        ("├── 04_Web_Platforms/", false), ("├── 05_Mobile_&_Gaming/", false),
        ("├── 06_Media_&_Audio/", false), ("├── 07_DevTools_&_Productivity/", false),
        ("├── 08_External/", false), ("├── 09_Archive/", false),
        ("├── 10_ADC/", false), ("├── 11_Personal/", false),
        ("├── entities.json", true), ("├── tasks.md", true), ("└── mempalace.yaml", true),
    ] {
        r.push(Line::from(Span::styled(format!("  {}", item), Style::new().fg(if file { CYAN } else { MID }))));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

fn render_master(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let val    = &app.wizard_master;
    let edit   = app.wizard_editing;
    let exists = !val.is_empty() && std::path::Path::new(val).exists();

    let disp = if edit { format!("  {}█", app.wizard_input) }
               else if val.is_empty() { "  (opsiyonel — [s] ile atla)".into() }
               else if exists          { format!("  ✓ {} (mevcut)", val) }
               else                    { format!("  + {} (oluşturulacak)", val) };

    let lines = vec![
        Line::from(Span::styled("  MASTER.md — AGENT CONSTITUTION", Style::new().fg(MID).bold())),
        Line::from(""),
        Line::from(Span::styled("  Tüm AI ajanların uyduğu kural dosyası.", Style::new().fg(DIM))),
        Line::from(Span::styled("  Yoksa minimal bir template oluşturulur.", Style::new().fg(DIM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ▶ ", Style::new().fg(ACCENT)),
            Span::styled("MASTER.md Path", Style::new().fg(ACCENT).bold()),
        ]),
        Line::from(Span::styled("      Mevcut yol veya oluşturulacak konum", Style::new().fg(DIM))),
        Line::from(Span::styled(
            disp,
            if edit    { Style::new().fg(GREEN).bg(Color::Rgb(0,25,10)) }
            else if val.is_empty() { Style::new().fg(DIM) }
            else if exists         { Style::new().fg(GREEN) }
            else                   { Style::new().fg(AMBER) },
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let mut r = vec![
        Line::from(Span::styled("  TEMPLATE ÖNİZLEME", Style::new().fg(DIM).bold())),
        Line::from(""),
    ];
    for l in MASTER_PREVIEW.lines() {
        r.push(Line::from(Span::styled(format!("  {}", l), Style::new().fg(Color::Rgb(100,130,110)))));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

fn render_agent(
    frame: &mut Frame, area: Rect, app: &App,
    name: &str, skipped: bool, installed: bool, version: &str,
    url: &str, will_create: &[&str],
) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let (s_text, s_col) = if skipped { ("ATLANDI", DIM) }
                          else if installed { ("KURULU", GREEN) }
                          else { ("KURULU DEĞİL", Color::Rgb(200,60,60)) };

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
        lines.push(Line::from(Span::styled(format!("  → {}", url), Style::new().fg(CYAN))));
    }

    lines.push(Line::from(""));
    if skipped {
        lines.push(Line::from(Span::styled("  Bu adım atlandı. [Tab] ile geri al.", Style::new().fg(DIM))));
    } else {
        lines.push(Line::from(Span::styled("  Oluşturulacaklar:", Style::new().fg(DIM))));
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
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let mut lines = vec![
        Line::from(Span::styled("  SKILLS & HOOKS", Style::new().fg(MID).bold())),
        Line::from(""),
        Line::from(Span::styled("  Tüm ajanlar tarafından paylaşılan skill ve hook dizinleri.", Style::new().fg(DIM))),
        Line::from(""),
        Line::from(Span::styled("  Oluşturulacaklar:", Style::new().fg(DIM))),
        Line::from(""),
    ];
    for (path, desc) in [
        (".agents/skills/prompt-master.md", "Prompt optimizasyon skill"),
        (".agents/skills/graphify.md",       "Knowledge graph skill"),
        (".agents/skills/verify-ai-os.md",   "System health skill"),
        (".agents/skills/ki-snapshot.md",    "Session snapshot skill"),
        (".agents/hooks/README.md",          "Hook sistemi dokümantasyonu"),
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
    let [left, right] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    let mut lines = vec![
        Line::from(Span::styled("  HAZIR — KURULUM ÖZETİ", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    if app.wizard_running {
        lines.push(Line::from(Span::styled("  ⚡ Kurulum çalışıyor...", Style::new().fg(AMBER).bold())));
    } else {
        for (label, val) in [
            ("Dev_Ops",  app.wizard_dev_ops.as_str()),
            ("MASTER",   app.wizard_master.as_str()),
            ("GitHub",   app.wizard_github.as_str()),
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
            ("Claude Code",  !app.wizard_skip_claude),
            ("Gemini CLI",   !app.wizard_skip_gemini),
            ("Antigravity",  !app.wizard_skip_antigravity),
        ] {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", name), Style::new().fg(DIM)),
                if active { Span::styled("✓ aktif",   Style::new().fg(GREEN)) }
                else      { Span::styled("⊘ atlandı", Style::new().fg(DIM))  },
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  [Enter] → Kurulumu Başlat", Style::new().fg(ACCENT).bold())));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

fn render_done(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let ok   = app.wizard_action_log.iter().filter(|a| a.ok && !a.skipped).count();
    let skip = app.wizard_action_log.iter().filter(|a| a.skipped).count();
    let fail = app.wizard_action_log.iter().filter(|a| !a.ok).count();

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  ✓ KURULUM TAMAMLANDI", Style::new().fg(ACCENT).bold().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![Span::styled(format!("  {} ", ok),   Style::new().fg(GREEN)), Span::styled("işlem başarılı", Style::new().fg(MID))]),
        Line::from(vec![Span::styled(format!("  {} ", skip), Style::new().fg(DIM)),   Span::styled("adım atlandı (zaten vardı)", Style::new().fg(DIM))]),
    ];
    if fail > 0 {
        lines.push(Line::from(vec![Span::styled(format!("  {} ", fail), Style::new().fg(RED)), Span::styled("hata — sağda detay", Style::new().fg(DIM))]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Sıradakiler:", Style::new().fg(DIM))));
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
    lines.push(Line::from(Span::styled("  [Enter] → Dashboard", Style::new().fg(ACCENT).bold())));
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
    let log = &app.wizard_action_log;
    let scroll = log.len().saturating_sub(visible);
    let max_w = inner.width.saturating_sub(4) as usize;

    let lines: Vec<Line> = log.iter().skip(scroll).map(|a| {
        let (icon, col) = if a.skipped { ("·", DIM) } else if a.ok { ("✓", GREEN) } else { ("✗", RED) };
        Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::new().fg(col)),
            Span::styled(a.desc.chars().take(max_w).collect::<String>(), Style::new().fg(if a.ok { MID } else if a.skipped { DIM } else { RED })),
        ])
    }).collect();

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

const MASTER_PREVIEW: &str = "# MASTER — [Kullanıcı]\n\n## 1. Kimlik & Davranış\nNet, direkt. Kod: EN, İletişim: TR.\n\n## 2. Kodlama\npnpm > npm. TypeScript strict.\nFonksiyonel. Hata yönetimi zorunlu.\n\n## 3. Güvenlik\nAPI key asla client-side.\nRLS day 0.\n\n## 4. Agent İş Bölümü\nClaude: Geliştirme\nGemini: Araştırma\nAntigravity: Görsel/Perf";
