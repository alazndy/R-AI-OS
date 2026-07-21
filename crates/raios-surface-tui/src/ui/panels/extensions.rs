use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use raios_surface_tui::app::{
    state::{ExtFocus, ExtensionInfo},
    App,
};

const ACCENT: Color = Color::Rgb(30, 140, 255);
const DIM: Color = Color::DarkGray;
const GREEN: Color = Color::Green;
const RED: Color = Color::Red;
const YELLOW: Color = Color::Yellow;

pub fn render_extensions(frame: &mut Frame, area: Rect, app: &App) {
    if !app.ext.loaded {
        let msg = Paragraph::new("Loading extensions…")
            .style(Style::new().fg(DIM))
            .block(Block::default().borders(Borders::ALL).title(" Extensions "));
        frame.render_widget(msg, area);
        return;
    }
    if app.ext.extensions.is_empty() {
        let msg = Paragraph::new(
            "No extensions found.\n\nAdd a raios-extension.toml to any project under your dev path.",
        )
        .wrap(Wrap { trim: false })
        .style(Style::new().fg(DIM))
        .block(Block::default().borders(Borders::ALL).title(" Extensions "));
        frame.render_widget(msg, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(area);

    render_ext_list(frame, chunks[0], app);
    render_ext_detail(frame, chunks[1], app);
}

fn render_ext_list(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .ext
        .extensions
        .iter()
        .enumerate()
        .map(|(i, ext)| {
            let selected = i == app.ext.ext_cursor;
            let style = if selected {
                Style::new()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::White)
            };
            ListItem::new(format!(" {} ", ext.name)).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Extensions ")
        .border_style(Style::new().fg(DIM));

    let mut state = ListState::default();
    state.select(Some(app.ext.ext_cursor));
    frame.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn render_ext_detail(frame: &mut Frame, area: Rect, app: &App) {
    let ext = match app.ext.extensions.get(app.ext.ext_cursor) {
        Some(e) => e,
        None => return,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    // Header
    render_ext_header(frame, chunks[0], ext, app);

    // Body: commands | config
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    render_commands_panel(frame, body_chunks[0], ext, app);
    render_config_panel(frame, body_chunks[1], ext, app);
}

fn render_ext_header(frame: &mut Frame, area: Rect, ext: &ExtensionInfo, app: &App) {
    let mut spans = vec![
        Span::styled(
            format!(" {} ", ext.name.to_uppercase()),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("v{}  ", ext.version), Style::new().fg(DIM)),
        Span::styled(ext.description.as_str(), Style::new().fg(Color::White)),
    ];

    if let Some(ref status) = app.ext.status {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(status.as_str(), Style::new().fg(YELLOW)));
    }

    // Service badges
    for svc in &ext.service_statuses {
        let (badge_color, badge_text) = if svc.active {
            (GREEN, format!(" {} ● ", svc.name))
        } else {
            (RED, format!(" {} ○ ", svc.name))
        };
        spans.push(Span::styled(badge_text, Style::new().fg(badge_color)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(DIM));
    let p = Paragraph::new(Line::from(spans))
        .block(block)
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

fn render_commands_panel(frame: &mut Frame, area: Rect, ext: &ExtensionInfo, app: &App) {
    let focused = app.ext.focus == ExtFocus::Commands;
    let border_style = if focused {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(DIM)
    };

    let items: Vec<ListItem> = ext
        .commands
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let selected = focused && i == app.ext.cmd_cursor;
            let name_style = if selected {
                Style::new()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
            };
            let desc_style = Style::new().fg(DIM);
            ListItem::new(vec![
                Line::from(Span::styled(format!(" ▶ {} ", cmd.name), name_style)),
                Line::from(Span::styled(format!("   {}", cmd.description), desc_style)),
            ])
        })
        .collect();

    let hint = if focused {
        "Enter: run  Tab: config"
    } else {
        "Tab: focus"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Commands  [{}] ", hint))
        .border_style(border_style);

    let mut state = ListState::default();
    if focused {
        state.select(Some(app.ext.cmd_cursor));
    }
    frame.render_stateful_widget(List::new(items).block(block), area, &mut state);
}

fn render_config_panel(frame: &mut Frame, area: Rect, ext: &ExtensionInfo, app: &App) {
    let focused = app.ext.focus == ExtFocus::Config;
    let border_style = if focused {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(DIM)
    };

    let hint = if app.ext.editing {
        "Enter: save  Esc: cancel"
    } else if focused {
        "e: edit  Tab: commands"
    } else {
        "Tab: focus"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Config  [{}] ", hint))
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in ext.config_schema.iter().enumerate() {
        let selected = focused && i == app.ext.cfg_cursor;

        let label_style = if selected {
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::White)
        };

        let display_val = if app.ext.editing && selected {
            // Show live input with cursor
            format!("{}█", app.ext.input)
        } else if field.masked && !field.value.is_empty() {
            "••••••••".to_string()
        } else if field.value.is_empty() {
            "(not set)".to_string()
        } else {
            field.value.clone()
        };

        let val_style = if field.value.is_empty() {
            Style::new().fg(DIM).add_modifier(Modifier::ITALIC)
        } else if app.ext.editing && selected {
            Style::new().fg(YELLOW)
        } else {
            Style::new().fg(GREEN)
        };

        let prefix = if selected { "▶ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{:<22}", prefix, field.label), label_style),
            Span::styled(display_val, val_style),
        ]));
        if !field.description.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("   {}", field.description),
                Style::new().fg(DIM),
            )));
        }
        lines.push(Line::raw(""));
    }

    let scroll = if focused && app.ext.cfg_cursor > 4 {
        (app.ext.cfg_cursor.saturating_sub(4)) as u16
    } else {
        0
    };
    let p = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(p, inner);
}
