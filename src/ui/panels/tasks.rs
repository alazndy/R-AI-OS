use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_tasks_panel(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.ui.right_panel_focus && app.ui.menu_cursor == 0;
    let border_color = if focused { GREEN } else { DIM };

    let hint = if focused {
        " [Space] done  [c] Claude  [g] Gemini  [a] Antigravity "
    } else {
        " [→] focus "
    };

    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(border_color))
        .title(Span::styled(" TASKS ", Style::new().fg(GREEN).bold()))
        .title_bottom(Span::styled(hint, Style::new().fg(DIM)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    if app.tasks.list.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No tasks — /task add <text> [@agent] [#project]",
            Style::new().fg(DIM).italic(),
        )));
    } else {
        let pending: Vec<_> = app
            .tasks
            .list
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.completed)
            .collect();
        let done: Vec<_> = app
            .tasks
            .list
            .iter()
            .enumerate()
            .filter(|(_, t)| t.completed)
            .collect();

        for (i, task) in pending.iter().chain(done.iter()) {
            let selected = *i == app.tasks.cursor && focused;

            let (mark, text_style) = if task.completed {
                ("✓", Style::new().fg(DIM))
            } else {
                ("☐", Style::new().fg(MID))
            };

            let cursor_span = if selected {
                Span::styled(" ❯ ", Style::new().fg(GREEN).bold())
            } else {
                Span::styled("   ", Style::new())
            };

            let text_color = if selected {
                GREEN
            } else if task.completed {
                DIM
            } else {
                MID
            };

            // Agent badge
            let agent_span = task
                .agent_label()
                .map(|label| {
                    let color = match task.agent.as_deref() {
                        Some("claude") => GREEN,
                        Some("gemini") => CYAN,
                        Some("antigravity") => AMBER,
                        _ => DIM,
                    };
                    Span::styled(format!(" {}", label), Style::new().fg(color).bold())
                })
                .unwrap_or_else(|| Span::styled("", Style::new()));

            // Project badge
            let proj_span = task
                .project
                .as_deref()
                .map(|p| Span::styled(format!(" #{}", p), Style::new().fg(DIM)))
                .unwrap_or_else(|| Span::styled("", Style::new()));

            lines.push(Line::from(vec![
                cursor_span,
                Span::styled(format!("{} ", mark), text_style),
                Span::styled(task.display().to_string(), Style::new().fg(text_color)),
                agent_span,
                proj_span,
            ]));
        }

        // Summary footer
        let total = app.tasks.list.len();
        let done_count = app.tasks.list.iter().filter(|t| t.completed).count();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}/{} done", done_count, total),
            Style::new().fg(DIM),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}
