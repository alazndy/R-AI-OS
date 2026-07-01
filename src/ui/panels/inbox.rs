use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Control-plane inbox: pending approvals (including agent handoffs), active runs,
/// and blocked tasks. Mirrors the MCP `get_inbox` tool, but rendered for humans.
pub fn render_inbox(frame: &mut Frame, area: Rect, app: &App) {
    let _ = app;

    let block = Block::new()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" INBOX ", Style::new().fg(GREEN).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Ok(data) = crate::app::load_inbox_panel_data() else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Could not load inbox snapshot.",
                Style::new().fg(RED),
            )),
            inner,
        );
        return;
    };
    let approvals = data.approvals;
    let runs = data.runs;
    let blocked = data.blocked;

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " PENDING APPROVALS",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    if approvals.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Nothing waiting.",
            Style::new().fg(DIM).italic(),
        )));
    } else {
        for scored in &approvals {
            let ap = &scored.approval;
            let (icon, color) = if ap.approval_type == "handover" {
                ("📨", CYAN)
            } else {
                ("⏳", AMBER)
            };
            let risk_color = match scored.risk_label {
                "low" => GREEN,
                "medium" => AMBER,
                _ => RED,
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::new().fg(color)),
                Span::styled(
                    format!("[{}] ", ap.approval_type),
                    Style::new().fg(color).bold(),
                ),
                Span::styled(
                    ap.task_title.clone().unwrap_or_else(|| "(no task)".into()),
                    Style::new().fg(MID),
                ),
                Span::styled(
                    format!(" [{} risk:{}]", scored.risk_label.to_uppercase(), scored.risk_score),
                    Style::new().fg(risk_color).bold(),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("      {}", truncate(&ap.reason, 90)),
                Style::new().fg(DIM),
            )));
            let impact = scored
                .file_impact
                .as_ref()
                .map(|fi| {
                    format!(
                        "files={} lines={}",
                        fi.files_changed.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                        fi.lines_changed.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                    )
                })
                .unwrap_or_else(|| "no file impact data".into());
            let history = scored
                .agent_success_rate
                .map(|r| format!("agent success rate: {:.0}%", r * 100.0))
                .unwrap_or_else(|| "no agent history".into());
            lines.push(Line::from(Span::styled(
                format!("      {impact}  ·  {history}  ·  suggested: {}", scored.suggested_action),
                Style::new().fg(DIM).italic(),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ACTIVE AGENT RUNS",
        Style::new().fg(MID).bold(),
    )));
    lines.push(Line::from(""));

    if runs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No agent runs in progress.",
            Style::new().fg(DIM).italic(),
        )));
    } else {
        for run in &runs {
            let agent_color = match run.agent_name.as_str() {
                "claude_kaira" | "claude" => GREEN,
                "codex_kaira" | "codex" => MAGENTA,
                "opencode_kaira" | "opencode" => CYAN,
                "antigravity_kaira" | "antigravity" | "agy" => AMBER,
                _ => DIM,
            };
            lines.push(Line::from(vec![
                Span::styled("  ● ", Style::new().fg(agent_color)),
                Span::styled(format!("{} ", run.agent_name), Style::new().fg(agent_color).bold()),
                Span::styled(
                    format!("({}) ", run.status),
                    Style::new().fg(DIM),
                ),
                Span::styled(
                    run.task_title.clone().unwrap_or_else(|| run.task_id.clone()),
                    Style::new().fg(MID),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " BLOCKED TASKS",
        Style::new().fg(MID).bold(),
    )));
    lines.push(Line::from(""));

    if blocked.is_empty() {
        lines.push(Line::from(Span::styled(
            "  None blocked.",
            Style::new().fg(DIM).italic(),
        )));
    } else {
        for task in &blocked {
            lines.push(Line::from(vec![
                Span::styled("  🚫 ", Style::new().fg(RED)),
                Span::styled(task.title.as_str(), Style::new().fg(MID)),
                Span::styled(
                    task.assignee_id
                        .as_deref()
                        .map(|a| format!(" @{a}"))
                        .unwrap_or_default(),
                    Style::new().fg(DIM),
                ),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn truncate(s: &str, max: usize) -> String {
    let oneline = s.replace('\n', " ");
    if oneline.chars().count() <= max {
        oneline
    } else {
        let mut t: String = oneline.chars().take(max).collect();
        t.push('…');
        t
    }
}
