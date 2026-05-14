use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_logs(frame: &mut Frame, area: Rect, app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(area);

    // Left Panel: Active Agents
    let mut agent_items = Vec::new();
    for (i, agent) in app.system.active_agents.iter().enumerate() {
        let style = if i == app.system.selected_agent_idx {
            Style::new().bg(Color::Rgb(0, 40, 20)).fg(GREEN).bold()
        } else {
            Style::new().fg(MID)
        };

        let status_color = match agent.status.as_str() {
            "Running" => GREEN,
            s if s.contains("Completed") => CYAN,
            _ => RED,
        };

        agent_items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    if i == app.system.selected_agent_idx {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::new().fg(GREEN),
                ),
                Span::styled(&agent.name, style),
            ]),
            Line::from(vec![
                Span::styled("    ", Style::new()),
                Span::styled(&agent.status, Style::new().fg(status_color).italic()),
            ]),
        ]));
    }

    if agent_items.is_empty() {
        agent_items.push(ListItem::new(Line::from(Span::styled(
            "  No active agents.",
            Style::new().fg(DIM).italic(),
        ))));
    }

    let agent_list = List::new(agent_items).block(
        Block::new()
            .borders(Borders::RIGHT)
            .border_style(Style::new().fg(DIM))
            .title(" AGENTS "),
    );
    frame.render_widget(agent_list, left);

    // Right Panel: Agent Logs
    let mut log_lines = Vec::new();
    if let Some(agent) = app.system.active_agents.get(app.system.selected_agent_idx) {
        for log in &agent.logs {
            let style = if log.contains("[stderr]") {
                Style::new().fg(RED)
            } else {
                Style::new().fg(MID)
            };
            log_lines.push(Line::from(Span::styled(log, style)));
        }
    }

    if log_lines.is_empty() {
        log_lines.push(Line::from(Span::styled(
            "  No logs for selected agent.",
            Style::new().fg(DIM).italic(),
        )));
    }

    let log_para = Paragraph::new(log_lines)
        .block(Block::new().title(" AGENT LOGS "))
        .wrap(Wrap { trim: false });
    frame.render_widget(log_para, right);
}
