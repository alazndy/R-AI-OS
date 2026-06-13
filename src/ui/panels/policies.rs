use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_policies(frame: &mut Frame, area: Rect, app: &App) {
    let _ = app;

    let block = Block::new()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" SECURITY ", Style::new().fg(DIM)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // ─── Load live policy data ────────────────────────────────────────────────
    let policy = crate::security::PolicyConfig::try_load_default();
    let audit_count = load_audit_count();

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " SECURITY KERNEL",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    // ── Sandbox ──────────────────────────────────────────────────────────────
    if let Some(ref p) = policy {
        let (label, color) = if p.filesystem.enforce_sandbox {
            ("ACTIVE ✓", GREEN)
        } else {
            ("DISABLED ✗", RED)
        };
        lines.push(Line::from(vec![
            Span::styled("  Filesystem Jail  ", Style::new().fg(MID)),
            Span::styled(label, Style::new().fg(color).bold()),
        ]));
        let blocked_count = p.filesystem.blocked_paths.len();
        lines.push(Line::from(Span::styled(
            format!("    {} explicit blocked paths", blocked_count),
            Style::new().fg(DIM),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Filesystem Jail  ", Style::new().fg(MID)),
            Span::styled("NO POLICY FILE", Style::new().fg(AMBER).bold()),
        ]));
    }

    lines.push(Line::from(""));

    // ── Tool Policy ───────────────────────────────────────────────────────────
    if let Some(ref p) = policy {
        let default_color = match p.tools.default_action {
            crate::security::policy::PolicyAction::Allow => GREEN,
            crate::security::policy::PolicyAction::Confirm => AMBER,
            crate::security::policy::PolicyAction::Deny => RED,
        };
        lines.push(Line::from(vec![
            Span::styled("  Tool Policy      ", Style::new().fg(MID)),
            Span::styled(
                format!(
                    "default: {}",
                    p.tools.default_action.as_str().to_uppercase()
                ),
                Style::new().fg(default_color).bold(),
            ),
        ]));

        // Show deny/confirm rules (skip allow — those are boring)
        let notable: Vec<_> = p
            .tools
            .rules
            .iter()
            .filter(|r| r.action != crate::security::policy::PolicyAction::Allow)
            .take(4)
            .collect();
        for rule in &notable {
            let (tag, col) = match rule.action {
                crate::security::policy::PolicyAction::Deny => ("DENY   ", RED),
                crate::security::policy::PolicyAction::Confirm => ("CONFIRM", AMBER),
                crate::security::policy::PolicyAction::Allow => ("ALLOW  ", GREEN),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("    [{}] ", tag), Style::new().fg(col)),
                Span::styled(rule.name.as_str(), Style::new().fg(MID)),
            ]));
        }
        let total_rules = p.tools.rules.len();
        if total_rules > 4 {
            lines.push(Line::from(Span::styled(
                format!("    +{} more rules…", total_rules - 4),
                Style::new().fg(DIM).italic(),
            )));
        }
    }

    lines.push(Line::from(""));

    // ── Egress Filter ─────────────────────────────────────────────────────────
    if let Some(ref p) = policy {
        if let Some(ref e) = p.egress {
            let (label, color) = if !e.enabled {
                ("OFF", DIM)
            } else if e.deny_all.unwrap_or(false) {
                ("DENY ALL ✗", RED)
            } else {
                ("ACTIVE ✓", GREEN)
            };
            lines.push(Line::from(vec![
                Span::styled("  Egress Filter    ", Style::new().fg(MID)),
                Span::styled(label, Style::new().fg(color).bold()),
            ]));
            if e.enabled && !e.deny_all.unwrap_or(false) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "    {} allowed  {} blocked domains",
                        e.allowed_domains.len(),
                        e.blocked_domains.len()
                    ),
                    Style::new().fg(DIM),
                )));
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled("  Egress Filter    ", Style::new().fg(MID)),
                Span::styled("NOT CONFIGURED", Style::new().fg(DIM)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // ── Audit Chain ───────────────────────────────────────────────────────────
    let (chain_label, chain_color) = match audit_count {
        Some(n) => (format!("{} events logged", n), GREEN),
        None => ("DB unavailable".to_string(), AMBER),
    };
    lines.push(Line::from(vec![
        Span::styled("  Audit Chain      ", Style::new().fg(MID)),
        Span::styled(chain_label, Style::new().fg(chain_color).bold()),
    ]));
    lines.push(Line::from(Span::styled(
        "    raios verify-chain to check integrity",
        Style::new().fg(DIM).italic(),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn load_audit_count() -> Option<i64> {
    let conn = crate::db::open_db().ok()?;
    conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .ok()
}
