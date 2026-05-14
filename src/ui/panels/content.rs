use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    let menu_files = app.current_menu_files();

    if menu_files.is_empty() {
        render_content_body(frame, area, app);
        return;
    }

    let file_count = menu_files.len() as u16;
    let files_height = (file_count + 3).min(area.height / 2);

    let [body_area, files_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(files_height)]).areas(area);

    render_content_body(frame, body_area, app);
    render_file_panel(frame, files_area, app, &menu_files);
}
pub fn render_content_body(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::NONE)
        .style(Style::new().bg(PANEL_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.system.is_syncing {
        let text = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ⚡ UNIVERSAL SYNC IN PROGRESS...",
                Style::new().fg(GREEN).bold(),
            )),
            Line::from(Span::styled(
                "  Aligning all agent constitutions with MASTER.md",
                Style::new().fg(DIM),
            )),
        ]);
        frame.render_widget(text, inner);
        return;
    }

    if let Some(ref status) = app.system.sync_status {
        let msg = format!("  ✓ {}", status);
        let badge = Paragraph::new(Span::styled(msg, Style::new().fg(GREEN)));
        let badge_area = Rect {
            height: 1,
            y: inner.y,
            ..inner
        };
        frame.render_widget(badge, badge_area);
    }

    match app.ui.menu_cursor {
        0 => render_recent(frame, inner, app),
        1 => render_rules(frame, inner, app),
        2 => render_diagnostics(frame, inner, app),
        3 => render_agents(frame, inner, app),
        4 => render_policies(frame, inner, app),
        5 => render_mempalace_info(frame, inner, app),
        6 => render_search_panel(frame, inner, app),
        7 => render_projects(frame, inner, app),
        8 => render_timeline(frame, inner, app),
        9 => render_logs(frame, inner, app),
        10 => render_sentinel_hub(frame, inner, app),
        11 => render_help(frame, inner, app),
        12 => render_system_audit(frame, inner, app),
        _ => {}
    }
}
