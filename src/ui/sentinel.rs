use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use crate::app::App;
use crate::sentinel::SentinelState;

pub fn render_sentinel_hub(frame: &mut Frame, area: Rect, app: &App) {
    let mut items = Vec::new();

    if app.sentinel_files.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  Sentinel is watching... No files reported yet.",
            Style::new().fg(Color::Rgb(80, 80, 80)).italic(),
        ))));
    } else {
        for file in &app.sentinel_files {
            let (icon, color) = match file.state {
                SentinelState::Clean => ("○", Color::Rgb(100, 100, 100)),
                SentinelState::Dirty => ("🔵", Color::Rgb(0, 150, 255)),
                SentinelState::Compiling => ("🟡", Color::Rgb(255, 200, 0)),
                SentinelState::Failed => ("🔴", Color::Rgb(255, 80, 80)),
                SentinelState::Compiled => ("🟢", Color::Rgb(0, 255, 136)),
                SentinelState::Verified => ("💠", Color::Rgb(0, 220, 220)),
            };

            let file_name = std::path::Path::new(&file.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.path.clone());

            let mut line_spans = vec![
                Span::styled(format!(" {} ", icon), Style::new().fg(color)),
                Span::styled(file_name, Style::new().fg(Color::Rgb(200, 200, 200))),
            ];

            if !file.errors.is_empty() {
                line_spans.push(Span::styled(
                    format!(" ({} errors)", file.errors.len()),
                    Style::new().fg(Color::Rgb(255, 80, 80)),
                ));
            }

            items.push(ListItem::new(Line::from(line_spans)));
            
            // Show errors if failed
            if file.state == SentinelState::Failed {
                for err in &file.errors {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled("      ! ", Style::new().fg(Color::Rgb(255, 80, 80))),
                        Span::styled(format!("Line {}: {}", err.line.unwrap_or(0), err.message), Style::new().fg(Color::Rgb(150, 150, 150))),
                    ])));
                }
            }
        }
    }

    let list = List::new(items).block(
        Block::new()
            .title(" SENTINEL HUB ")
            .title_style(Style::new().fg(Color::Rgb(0, 220, 220)).bold())
            .borders(Borders::NONE),
    );

    frame.render_widget(list, area);
}
