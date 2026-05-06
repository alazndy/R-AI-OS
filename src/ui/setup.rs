use ratatui::{
    Frame,
    layout::{Constraint, Layout, Direction},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use crate::app::App;
use crate::ui::*;


pub fn render_setup(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let center = center_rect(72, 22, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(AMBER))
        .title(Span::styled(" R-AI-OS FIRST RUN SETUP ", Style::new().fg(AMBER).bold()))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(center);
    frame.render_widget(block, center);

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(inner);

    let mut left_lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "  Configure your workspace paths:",
            Style::new().fg(DIM),
        )),
        Line::from(""),
    ];

    for (i, field) in app.setup_fields.iter().enumerate() {
        let is_selected = i == app.setup_cursor;
        let is_editing  = is_selected && app.setup_editing;

        let label_style = if is_selected {
            Style::new().fg(AMBER).bold()
        } else {
            Style::new().fg(MID)
        };

        left_lines.push(Line::from(vec![
            Span::styled(format!(" {} ", if is_selected { "▶" } else { " " }), label_style),
            Span::styled(field.label, label_style),
        ]));
        left_lines.push(Line::from(Span::styled(
            format!("     {}", field.hint),
            Style::new().fg(DIM).italic(),
        )));

        let (display_val, val_style) = if is_editing {
            (format!("  {}█", app.setup_input), Style::new().fg(Color::White).bg(Color::Rgb(20, 30, 20)))
        } else if field.value.is_empty() {
            ("  (not set — press Enter to edit)".into(), Style::new().fg(RED))
        } else if field.auto_detected {
            (format!("  ✓ {}", field.value), Style::new().fg(GREEN))
        } else {
            (format!("  ✏ {}", field.value), Style::new().fg(CYAN))
        };

        left_lines.push(Line::from(Span::styled(display_val, val_style)));
        left_lines.push(Line::from(""));
    }

    if let Some(ref status) = app.setup_status {
        let color = if status.starts_with("Save") { RED } else { AMBER };
        left_lines.push(Line::from(Span::styled(format!(" ⚠ {}", status), Style::new().fg(color).bold())));
    }

    left_lines.push(Line::from(""));
    left_lines.push(Line::from(Span::styled(
        " [↑↓] navigate  [Enter] edit  [s] save  [q] quit",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(left_lines)), layout[0]);

    // Right side: Requirements
    let mut right_lines = vec![
        Line::from(Span::styled(" System Requirements:", Style::new().fg(DIM))),
        Line::from(""),
    ];

    for req in &app.requirements {
        let (icon, color) = if req.installed { ("✓", GREEN) } else { ("✗", RED) };
        let crit_tag = if req.critical && !req.installed {
            Span::styled(" [CRITICAL]", Style::new().fg(RED).bold())
        } else {
            Span::styled("", Style::new())
        };
        right_lines.push(Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::new().fg(color).bold()),
            Span::styled(req.name, Style::new().fg(MID)),
            crit_tag,
        ]));
        if req.installed {
            right_lines.push(Line::from(Span::styled(
                format!("    {}", req.version.split('\n').next().unwrap_or("")),
                Style::new().fg(DIM).italic(),
            )));
        } else {
            right_lines.push(Line::from(Span::styled(
                format!("    missing binary: {}", req.command),
                Style::new().fg(Color::Rgb(100, 50, 50)).italic(),
            )));
        }
        right_lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(Text::from(right_lines)), layout[1]);
}


