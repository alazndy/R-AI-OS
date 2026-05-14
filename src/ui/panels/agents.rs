use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

pub fn render_agents(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(
            " AI AGENT RULE FILES",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    if app.inventory.agent_rule_groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Scanning agent configs...",
            Style::new().fg(DIM),
        )));
    } else {
        for group in &app.inventory.agent_rule_groups {
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
                Span::styled(format!("  {}", group.config_dir), Style::new().fg(DIM)),
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
    if !app.inventory.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            " RUNTIME AGENTS (PATH)",
            Style::new().fg(MID).bold(),
        )));
        lines.push(Line::from(""));
        for agent in &app.inventory.agents {
            let (mark, color) = if agent.exists() {
                ("●", GREEN)
            } else {
                ("○", DIM)
            };
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
    if !app.inventory.skills.is_empty() {
        lines.push(Line::from(Span::styled(
            " INSTALLED SKILLS & AI EXTENSIONS",
            Style::new().fg(MID).bold(),
        )));
        lines.push(Line::from(""));

        for s in &app.inventory.skills {
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

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}
