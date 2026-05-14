use crate::app::App;
use crate::filebrowser::FileEntry;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_file_panel(frame: &mut Frame, area: Rect, app: &App, files: &[FileEntry]) {
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::from(vec![
        Span::styled(
            if app.ui.right_panel_focus {
                " FILES "
            } else {
                " FILES "
            },
            Style::new()
                .fg(if app.ui.right_panel_focus { GREEN } else { DIM })
                .bold(),
        ),
        Span::styled(
            if app.ui.right_panel_focus {
                "[↑↓] nav  [Enter] view  [e] edit  [o] ext  [←] menu"
            } else {
                "[→] focus"
            },
            Style::new().fg(DIM),
        ),
    ])];

    for (i, entry) in files.iter().enumerate() {
        let exist_mark = if entry.exists() {
            Span::styled("✓ ", Style::new().fg(GREEN))
        } else {
            Span::styled("✗ ", Style::new().fg(DIM))
        };
        let ro_tag = if entry.read_only {
            Span::styled(" [RO]", Style::new().fg(DIM))
        } else {
            Span::styled("", Style::new())
        };

        if app.ui.right_panel_focus && i == app.ui.right_file_cursor {
            lines.push(Line::from(vec![
                exist_mark,
                Span::styled(format!("▶ {}", entry.name), Style::new().fg(GREEN).bold()),
                ro_tag,
            ]));
        } else {
            lines.push(Line::from(vec![
                exist_mark,
                Span::styled(format!("  {}", entry.name), Style::new().fg(MID)),
                ro_tag,
            ]));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

pub fn render_file_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let Some(ref file) = app.editor.active_file else {
        return;
    };

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let ro_tag = if file.read_only { " [READONLY]" } else { "" };
    let compliance_span = if let Some(ref report) = app.health.compliance {
        let color = match report.score_color() {
            0 => GREEN,
            1 => AMBER,
            _ => RED,
        };
        let lang = report.language();
        let lang_part = if lang.is_empty() {
            String::new()
        } else {
            format!(" [{}]", lang)
        };
        let issues = report.violations.len();
        Span::styled(
            format!(
                "  📊 {}/100 {}{} [{} issues]",
                report.score,
                report.grade(),
                lang_part,
                issues
            ),
            Style::new().fg(color).bold(),
        )
    } else {
        Span::styled("", Style::new())
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ", Style::new()),
            Span::styled(file.name.as_str(), Style::new().fg(GREEN).bold()),
            Span::styled(ro_tag, Style::new().fg(AMBER)),
            compliance_span,
        ]),
        Line::from(Span::styled(
            format!("  {}", file.path.display()),
            Style::new().fg(DIM),
        )),
    ])
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(header, header_area);

    // Content with line numbers + syntax highlighting
    let scroll = app.editor.scroll as usize;
    let visible_h = content_area.height as usize;
    let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut in_block = false;
    for line in app.editor.lines.iter().take(scroll) {
        update_code_block_state(line, ext, &mut in_block);
    }

    let mut lines: Vec<Line> = Vec::new();
    for (i, line) in app
        .editor
        .lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
    {
        let lno = Span::styled(format!("{:>4} │ ", i + 1), Style::new().fg(DIM));
        let mut spans = vec![lno];
        spans.extend(highlight_line(line, &mut in_block, ext));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Footer
    let total = app.editor.lines.len();
    let pct = if total > 0 {
        ((scroll + visible_h).min(total) * 100 / total).min(100)
    } else {
        100
    };
    let edit_hint = if !file.read_only { "  [e] edit" } else { "" };
    let issue_span = app
        .health
        .compliance
        .as_ref()
        .and_then(|r| r.first_issue())
        .map(|txt| Span::styled(format!("   ⚠ {}", txt), Style::new().fg(AMBER)));
    let mut footer_spans = vec![
        Span::styled(
            format!(" Ln {}/{} ({}%)", scroll + 1, total, pct),
            Style::new().fg(DIM),
        ),
        Span::styled(
            format!("   [↑↓/PgUp/PgDn] scroll{}  [Esc/q] back", edit_hint),
            Style::new().fg(DIM),
        ),
    ];
    if let Some(s) = issue_span {
        footer_spans.push(s);
    }
    let footer = Paragraph::new(Line::from(footer_spans)).block(
        Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(footer, footer_area);
}

pub fn render_file_edit(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let Some(ref file) = app.editor.active_file else {
        return;
    };

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // Header
    let edit_compliance_span = if let Some(ref report) = app.health.compliance {
        let color = match report.score_color() {
            0 => GREEN,
            1 => AMBER,
            _ => RED,
        };
        Span::styled(
            format!("  📊 {}/100 {}", report.score, report.grade()),
            Style::new().fg(color),
        )
    } else {
        Span::styled("", Style::new())
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ✏ ", Style::new().fg(AMBER)),
            Span::styled(file.name.as_str(), Style::new().fg(AMBER).bold()),
            Span::styled("  ", Style::new()),
            Span::styled(
                "EDITING",
                Style::new()
                    .fg(Color::White)
                    .bg(Color::Rgb(60, 20, 0))
                    .bold(),
            ),
            edit_compliance_span,
        ]),
        Line::from(Span::styled(
            format!("  {}", file.path.display()),
            Style::new().fg(DIM),
        )),
    ])
    .block(
        Block::new()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(header, header_area);

    // Editor content
    let ed = &app.editor;
    let scroll = ed.scroll;
    let visible_h = content_area.height as usize;

    // Cursor position in the frame
    let cursor_screen_row = ed.editor.cursor_row.saturating_sub(scroll as usize);
    let cursor_screen_col = ed.editor.cursor_col + 7; // 7 = "  NNN │ " prefix width

    let lines: Vec<Line> = ed
        .lines
        .iter()
        .enumerate()
        .skip(scroll as usize)
        .take(visible_h)
        .map(|(i, line)| {
            let lno = Span::styled(format!("  {:>3} │ ", i + 1), Style::new().fg(DIM));
            if i == ed.editor.cursor_row {
                // Highlight cursor line
                Line::from(vec![
                    lno,
                    Span::styled(
                        line.as_str(),
                        Style::new().fg(Color::White).bg(Color::Rgb(15, 25, 35)),
                    ),
                ])
            } else {
                Line::from(vec![lno, Span::styled(line.as_str(), Style::new().fg(MID))])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Place the terminal cursor
    if cursor_screen_row < content_area.height as usize {
        frame.set_cursor_position((
            content_area.x + cursor_screen_col as u16,
            content_area.y + cursor_screen_row as u16,
        ));
    }

    // Footer
    let save_msg = app
        .editor
        .save_msg
        .as_deref()
        .map(|m| {
            let color = if m.starts_with("Error") { RED } else { GREEN };
            Span::styled(format!("  {}", m), Style::new().fg(color).bold())
        })
        .unwrap_or_else(|| Span::styled("", Style::new()));

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" Ln {}/{}", ed.editor.cursor_row + 1, ed.lines.len()),
            Style::new().fg(DIM),
        ),
        Span::styled(
            "   [Ctrl+S] save  [Ctrl+Q/Esc] cancel",
            Style::new().fg(DIM),
        ),
        save_msg,
    ]))
    .block(
        Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().fg(DIM)),
    );
    frame.render_widget(footer, footer_area);
}
