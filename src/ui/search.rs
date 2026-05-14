use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_search(frame: &mut Frame, app: &App) {
    // Render dashboard in background
    render_dashboard(frame, app);

    // Dim the background
    let area = frame.area();
    // let dim = Block::new().style(Style::new().bg(Color::Rgb(0, 0, 0)));
    // frame.render_widget(dim, area);

    let popup_area = center_rect(80, 70, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CYAN))
        .title(vec![
            Span::styled(" FUZZY FINDER ", Style::new().fg(CYAN).bold()),
            Span::styled(" (indexer active) ", Style::new().fg(DIM).italic()),
        ])
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    // Search Input
    let input = Paragraph::new(format!(" > {}", app.search.query))
        .style(Style::new().fg(Color::White))
        .block(
            Block::new()
                .borders(Borders::BOTTOM)
                .border_style(Style::new().fg(DIM)),
        );
    frame.render_widget(input, chunks[0]);

    // Results
    let mut items = Vec::new();
    for (i, res) in app.search.results.iter().enumerate() {
        let is_selected = i == app.search.cursor;
        let style = if is_selected {
            Style::new().bg(Color::Rgb(30, 40, 50)).fg(AMBER).bold()
        } else {
            Style::new().fg(MID)
        };

        let file_name = res.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let mut line_content = vec![
            Span::styled(format!(" {:<20} ", res.project), Style::new().fg(DIM)),
            Span::styled(format!("{:<30} ", file_name), style),
            Span::styled(format!("Ln {:<4} ", res.line), Style::new().fg(DIM)),
        ];

        if is_selected {
            line_content.push(Span::styled(
                format!("| {}", res.snippet),
                Style::new().fg(AMBER).italic(),
            ));
        } else {
            line_content.push(Span::styled(
                format!("| {}", res.snippet),
                Style::new().fg(DIM),
            ));
        }

        items.push(ListItem::new(Line::from(line_content)));
    }

    if items.is_empty() && !app.search.query.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "   No results found.",
            Style::new().fg(RED),
        ))));
    }

    let results_list = List::new(items)
        .highlight_style(Style::new().add_modifier(Modifier::BOLD))
        .block(Block::new());

    frame.render_widget(results_list, chunks[1]);
}

pub fn render_search_panel(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(
            " NEURAL SEARCH — Content-Aware Project Index",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    // Index status line
    let (status_text, status_color) = if app.search.is_indexing {
        ("  Building index...".to_string(), AMBER)
    } else if let Some(ref s) = app.search.status {
        (format!("  {}", s), GREEN)
    } else {
        ("  No index — use /search <query> to build".to_string(), DIM)
    };
    lines.push(Line::from(Span::styled(
        status_text,
        Style::new().fg(status_color),
    )));
    lines.push(Line::from(""));

    if app.search.results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Use /search <query> to search across all projects",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Example: /search hata yönetimi",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Example: /search async error handling",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  /reindex  — force rebuild the index",
            Style::new().fg(DIM),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} results", app.search.results.len()),
                Style::new().fg(CYAN).bold(),
            ),
            Span::styled(
                "  [→] focus  [↑↓] navigate  [Enter] open",
                Style::new().fg(DIM),
            ),
        ]));
        lines.push(Line::from(""));

        for (i, result) in app.search.results.iter().enumerate() {
            let is_selected = app.ui.right_panel_focus && i == app.search.cursor;

            let file_name = result
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled("  ▶ ", Style::new().fg(GREEN).bold()),
                    Span::styled(file_name.to_string(), Style::new().fg(GREEN).bold()),
                    Span::styled(format!(":{}", result.line), Style::new().fg(AMBER)),
                    Span::styled(format!("  [{}]", result.project), Style::new().fg(DIM)),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("      {}", result.snippet),
                    Style::new().fg(MID),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::new()),
                    Span::styled(file_name.to_string(), Style::new().fg(CYAN)),
                    Span::styled(format!(":{}", result.line), Style::new().fg(DIM)),
                    Span::styled(format!("  [{}]", result.project), Style::new().fg(DIM)),
                ]));
                let snippet: String = result.snippet.chars().take(70).collect();
                lines.push(Line::from(Span::styled(
                    format!("      {}", snippet),
                    Style::new().fg(DIM),
                )));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}
