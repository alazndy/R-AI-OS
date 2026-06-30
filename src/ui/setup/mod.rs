use crate::app::App;
use crate::setup_wizard::WizardStep;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

mod steps;

pub(super) const ACCENT: Color = Color::Rgb(30, 140, 255);
pub(super) const PANEL: Color = Color::Rgb(8, 12, 18);
pub(super) const DIM_B: Color = Color::Rgb(20, 35, 50);

pub(super) const MASTER_PREVIEW: &str = "# AGENT CONSTITUTION (v5.0)\n# K-AI-RA — Single source of truth\n\n## Identity\n- Claude Kaira  |  Codex Kaira\n\n## RIPER-5\n1. Requirement  2. Investigation\n3. Planning     4. Execution\n5. Review & Refactor\n\n## AgentShield (OWASP)\n- No client-side secrets\n- Parameterized queries only\n- pnpm audit on every commit\n\n## Skills\nraios · search-first · graphify\nprompt-master · ki-snapshot";

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
        WizardStep::Welcome => steps::render_welcome(frame, area, app),
        WizardStep::Workspace => steps::render_workspace(frame, area, app),
        WizardStep::Constitution => steps::render_master(frame, area, app),
        WizardStep::Claude => steps::render_agent(
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
        WizardStep::Codex => steps::render_agent(
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
        WizardStep::OpenCode => steps::render_agent(
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
            &["~/.config/opencode/opencode.jsonc (MCP)"],
        ),
        WizardStep::Skills => steps::render_skills(frame, area, app),
        WizardStep::AgentWrapper => steps::render_agent_wrapper(frame, area, app),
        WizardStep::Initialize => steps::render_initialize(frame, area, app),
        WizardStep::Done => steps::render_done(frame, area, app),
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hint = match &app.wizard.step {
        WizardStep::Welcome => " [Enter] Başla  [q] Çık ",
        WizardStep::Done => " [Enter] Dashboard'ı Aç ",
        WizardStep::Initialize if app.wizard.running => " Kurulum çalışıyor... ",
        WizardStep::Initialize => " [Enter] Kurulumu Başlat  [q] Çık ",
        WizardStep::AgentWrapper => " [↑↓] Seç  [s] Devam  [q] Çık ",
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
