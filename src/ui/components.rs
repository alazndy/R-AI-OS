use crate::app::{filtered_palette, App};
use crate::ui::*;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_boot(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let total = 5u16;
    let done = app.system.boot_results.len() as u16;
    let progress = if total > 0 {
        (done * 100 / total).min(100)
    } else {
        0
    };

    let center = center_rect(60, (total + 10).min(area.height), area);

    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(center);

    let spin = spinner_char(app.tick);
    let heading = Paragraph::new(format!(
        " {}  Initializing R-AI-OS v{} Core...",
        spin,
        env!("CARGO_PKG_VERSION")
    ))
    .style(Style::new().fg(GREEN).add_modifier(Modifier::BOLD));
    frame.render_widget(heading, rows[0]);

    let gauge = Gauge::default()
        .block(Block::new())
        .gauge_style(Style::new().fg(GREEN).bg(DIM))
        .percent(progress)
        .label(format!("{}/{} checks", done, total));
    frame.render_widget(gauge, rows[2]);

    let items: Vec<ListItem> = app
        .system.boot_results
        .iter()
        .map(|(name, pass)| {
            let (mark, color) = if *pass { ("✓", GREEN) } else { ("✗", RED) };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", mark), Style::new().fg(color).bold()),
                Span::styled(name.as_str(), Style::new().fg(MID)),
            ]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, rows[3]);
}

pub fn render_bouncing_alert(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let width = 60;
    let height = 6;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = 1; // Top of the screen
    let rect = Rect::new(x, y, width, height);

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "🚨 HUMAN INTERVENTION REQUIRED 🚨",
            Style::new().fg(Color::White).bold(),
        )),
        Line::from(Span::styled(
            "BOUNCING LIMIT REACHED!",
            Style::new().fg(AMBER).bold(),
        )),
        Line::from(Span::styled(
            format!("(Consecutive Handovers: {})", app.system.handover_count),
            Style::new().fg(Color::White),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(RED))
        .bg(Color::Rgb(120, 0, 0));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);

    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(paragraph, rect);
}

pub fn render_launcher(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = if app.ui.command_mode {
        Line::from(vec![
            Span::styled(" ❯ ", Style::new().fg(GREEN).bold()),
            Span::styled(app.ui.command_buf.as_str(), Style::new().fg(Color::White)),
            Span::styled("█", Style::new().fg(GREEN)),
        ])
    } else if app.ui.right_panel_focus {
        Line::from(Span::styled(
            " [↑↓] navigate  [Enter] view  [e] edit  [o] VS Code  [←] menu  [/] command",
            Style::new().fg(DIM),
        ))
    } else if let Some(act) = app.timeline.activities.last() {
        Line::from(vec![
            Span::styled(format!(" [LOG] {} » ", act.timestamp), Style::new().fg(DIM)),
            Span::styled(act.message.as_str(), Style::new().fg(CYAN).italic()),
        ])
    } else {
        let hint = if !app.current_menu_files().is_empty() {
            "  [→] files"
        } else {
            ""
        };
        Line::from(Span::styled(
            format!(" [↑↓] menu  [/] or [Tab] command{}", hint),
            Style::new().fg(DIM),
        ))
    };

    frame.render_widget(Paragraph::new(content), inner);
}

pub fn render_command_palette(frame: &mut Frame, app: &App) {
    let filtered = filtered_palette(&app.ui.command_buf);
    if filtered.is_empty() {
        return;
    }

    let area = frame.area();
    let popup_h = (filtered.len() as u16 + 2).min(13);
    let popup_w = 68u16.min(area.width.saturating_sub(4));
    let popup = Rect {
        x: area.x + area.width.saturating_sub(popup_w) / 2,
        y: area.y + area.height.saturating_sub(popup_h + 3),
        width: popup_w,
        height: popup_h,
    };

    frame.render_widget(
        Block::new().style(Style::new().bg(Color::Rgb(12, 18, 24))),
        popup,
    );

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(GREEN))
        .title(Span::styled(
            " Commands — [↑↓] nav  [Tab] fill  [Enter] run ",
            Style::new().fg(DIM),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<Line> = filtered
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if i == app.ui.palette_cursor {
                Line::from(vec![
                    Span::styled("  ▶ ", Style::new().fg(GREEN).bold()),
                    Span::styled(item.cmd, Style::new().fg(GREEN).bold()),
                    Span::styled(format!("  {}", item.desc), Style::new().fg(AMBER)),
                ])
            } else {
                Line::from(vec![
                    Span::styled("    ", Style::new()),
                    Span::styled(item.cmd, Style::new().fg(CYAN)),
                    Span::styled(format!("  {}", item.desc), Style::new().fg(DIM)),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(items)), inner);
}

pub fn render_file_changed_badge(frame: &mut Frame, _app: &App) {
    let area = frame.area();
    let badge_w = 42u16;
    let badge = Rect {
        x: area.x + area.width.saturating_sub(badge_w + 2),
        y: area.y + 1,
        width: badge_w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            "  ⚡ File changed — [R] reload  ",
            Style::new().fg(Color::Black).bg(AMBER).bold(),
        )),
        badge,
    );
}

pub fn render_launcher_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup = center_rect(36, 8, area);

    frame.render_widget(
        Block::new().style(Style::new().bg(Color::Rgb(12, 18, 24))),
        popup,
    );

    let proj_name = app
        .projects
        .active
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CYAN))
        .title(Span::styled(" Launch Agent ", Style::new().fg(CYAN).bold()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Project: {}", proj_name),
            Style::new().fg(DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [C] ", Style::new().fg(GREEN).bold()),
            Span::styled("Claude Code", Style::new().fg(MID)),
        ]),
        Line::from(vec![
            Span::styled("  [G] ", Style::new().fg(CYAN).bold()),
            Span::styled("Gemini CLI", Style::new().fg(MID)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [Esc] Cancel", Style::new().fg(DIM))),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

pub fn center_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let x = parent.x + parent.width.saturating_sub(width) / 2;
    let y = parent.y + parent.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(parent.width),
        height: height.min(parent.height),
    }
}

pub fn render_handover_modal(frame: &mut Frame, app: &App) {
    if let Some((target, instruction)) = &app.system.handover_modal {
        let area = center_rect(60, 40, frame.area());

        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(" ⚠️ Human Approval Required: Bouncing Limit ")
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(255, 170, 0))) // AMBER
            .style(Style::default().bg(Color::Rgb(8, 12, 16)));

        let mut lines = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            " The agents are bouncing tasks too much.",
            Style::default().fg(Color::Rgb(255, 170, 0)).bold(),
        )]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                " Target Agent: ",
                Style::default().fg(Color::Rgb(170, 170, 170)),
            ),
            Span::styled(
                target.clone(),
                Style::default().fg(Color::Rgb(0, 255, 136)).bold(),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Instruction:",
            Style::default().fg(Color::Rgb(170, 170, 170)),
        )));

        let instruction_words: Vec<&str> = instruction.split_whitespace().collect();
        let mut current_line = String::from("   ");
        for word in instruction_words {
            if current_line.len() + word.len() > 50 {
                lines.push(Line::from(Span::styled(
                    current_line.clone(),
                    Style::default().fg(Color::Rgb(170, 170, 170)),
                )));
                current_line = format!("   {}", word);
            } else {
                current_line.push_str(word);
                current_line.push(' ');
            }
        }
        if !current_line.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                current_line,
                Style::default().fg(Color::Rgb(170, 170, 170)),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                " [Y] Approve",
                Style::default().fg(Color::Rgb(0, 255, 136)).bold(),
            ),
            Span::styled(
                "    [N] Reject",
                Style::default().fg(Color::Rgb(255, 80, 80)).bold(),
            ),
        ]));

        let p = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(p, area);
    }
}
