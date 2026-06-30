use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use super::ACCENT;
use super::DIM_B;

#[allow(clippy::too_many_arguments)]
pub fn render_agent(
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

pub fn render_skills(frame: &mut Frame, area: Rect, app: &App) {
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
        (".agents/skills/prompt-master.md", "Prompt optimizasyon"),
        (".agents/skills/graphify.md", "Mimari haritalama"),
        (".agents/skills/search-first.md", "Koddan önce araştır"),
        (".agents/skills/ki-snapshot.md", "Session özeti"),
        (".agents/skills/continuous-learning.md", "Instinct kaydı"),
        (".agents/skills/verify-ai-os.md", "Sistem sağlığı"),
        (".agents/hooks/README.md", "Hook dokümantasyonu"),
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

pub fn render_agent_wrapper(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let choice = app.wizard.field_cursor;

    let choices: &[(&str, &str)] = &[
        (
            "Evet — tümü  (claude, codex, opencode, agy)",
            "Önerilen",
        ),
        ("Hayır — atla", ""),
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "  AGENT WRAPPER",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Ajanlar herzaman raios üzerinden çalışsın mı?",
            Style::new().fg(MID),
        )),
        Line::from(Span::styled(
            "  (UMAI shield + handoff inject + session capture)",
            Style::new().fg(DIM),
        )),
        Line::from(""),
    ];

    for (i, (label, badge)) in choices.iter().enumerate() {
        let selected = i == choice;
        let radio = if selected { "◉" } else { "○" };
        let (fg, bg) = if selected {
            (ACCENT, Style::new().fg(ACCENT).bold())
        } else {
            (DIM, Style::new().fg(DIM))
        };

        let mut spans = vec![
            Span::styled(format!("  {} ", radio), Style::new().fg(fg)),
            Span::styled(*label, bg),
        ];
        if !badge.is_empty() {
            spans.push(Span::styled(
                format!("  [{}]", badge),
                Style::new().fg(GREEN),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Shell function olarak ~/.zshrc / ~/.bashrc'a eklenir.",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "  Sonradan: raios agent-wrapper status / remove",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), left);

    let mut r = vec![
        Line::from(Span::styled(
            "  NE YAZILIR",
            Style::new().fg(DIM).bold(),
        )),
        Line::from(""),
    ];
    if choice == 0 {
        for agent in crate::agent_wrapper::ALL_AGENTS {
            r.push(Line::from(vec![
                Span::styled("  + ", Style::new().fg(ACCENT)),
                Span::styled(
                    format!("{}() {{ raios run {} \"$@\"; }}", agent, agent),
                    Style::new().fg(Color::Rgb(100, 130, 110)),
                ),
            ]));
        }
        r.push(Line::from(""));
        r.push(Line::from(Span::styled(
            "  → ~/.zshrc'a eklenir",
            Style::new().fg(DIM),
        )));
        r.push(Line::from(Span::styled(
            "  Terminal yeniden başlatılınca aktif olur.",
            Style::new().fg(DIM),
        )));
    } else {
        r.push(Line::from(Span::styled(
            "  Hiçbir şey yazılmaz.",
            Style::new().fg(DIM),
        )));
        r.push(Line::from(""));
        r.push(Line::from(Span::styled(
            "  Sonradan aktifleştirmek için:",
            Style::new().fg(DIM),
        )));
        r.push(Line::from(Span::styled(
            "  raios agent-wrapper install",
            Style::new().fg(CYAN),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(r)), right);
}

pub fn render_initialize(frame: &mut Frame, area: Rect, app: &App) {
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
        let wrapper_active = app.wizard.agent_wrapper_choice == 0;
        lines.push(Line::from(vec![
            Span::styled("  Agent Wrapper ", Style::new().fg(DIM)),
            if wrapper_active {
                Span::styled("✓ tümü", Style::new().fg(GREEN))
            } else {
                Span::styled("⊘ atlandı", Style::new().fg(DIM))
            },
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  [Enter] → Kurulumu Başlat",
            Style::new().fg(ACCENT).bold(),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), left);
    render_log(frame, right, app);
}

pub fn render_done(frame: &mut Frame, area: Rect, app: &App) {
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
            Style::new()
                .fg(ACCENT)
                .bold()
                .add_modifier(Modifier::BOLD),
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
    let wrapper_active = app.wizard.agent_wrapper_choice == 0;
    let step1_text = if wrapper_active {
        "Terminali yeniden başlat (MCP + wrapper aktifleşir)"
    } else {
        "Claude Code'u yeniden başlat (MCP aktifleşir)"
    };
    for (n, text) in [
        ("1.", step1_text),
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
