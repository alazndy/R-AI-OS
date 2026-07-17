use raios_surface_tui::app::state::OutlineRow;
use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

pub fn render_constitution(frame: &mut Frame, area: Rect, app: &App) {
    if app.constitution.creator.active {
        render_creator(frame, area, app);
        return;
    }

    let mut lines = vec![render_tab_bar(app), Line::from("")];

    if app.constitution.sections.is_empty() {
        let target_path = app
            .constitution
            .tabs
            .get(app.constitution.active_tab)
            .map(|t| t.path().to_path_buf())
            .unwrap_or_default();
        let content = raios_runtime::filebrowser::load_file_content(&target_path);
        if raios_runtime::constitution::is_include_only(&content) {
            lines.push(Line::from(vec![Span::styled(
                " ↳ includes: AGENT_CONSTITUTION.md — press [1] to edit the real content",
                Style::new().fg(DIM),
            )]));
        } else {
            lines.push(Line::from(Span::styled(
                " (empty or unparsed — press [r] to raw-edit)",
                Style::new().fg(DIM),
            )));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
        return;
    }

    for (row_idx, row) in app.constitution.rows.iter().enumerate() {
        let selected = row_idx == app.constitution.outline_cursor;
        lines.push(render_row(app, row, selected));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_tab_bar(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, tab) in app.constitution.tabs.iter().enumerate() {
        let marker = if i == app.constitution.active_tab { GREEN } else { DIM };
        spans.push(Span::styled(
            format!(" [{}] {} ", i + 1, tab.label()),
            Style::new().fg(marker).bold(),
        ));
    }
    Line::from(spans)
}

fn render_row(app: &App, row: &OutlineRow, selected: bool) -> Line<'static> {
    let prefix = if selected { "▶ " } else { "  " };
    let base_style = if selected {
        Style::new().fg(GREEN).bold()
    } else {
        Style::new().fg(MID)
    };
    match *row {
        OutlineRow::Section { idx } => {
            let title = app.constitution.sections[idx].title.clone();
            Line::from(Span::styled(format!("{}◈ {}", prefix, title), base_style))
        }
        OutlineRow::Child { idx, child_idx } => {
            let title = app.constitution.sections[idx].children[child_idx].title.clone();
            Line::from(Span::styled(format!("{}  ◦ {}", prefix, title), base_style))
        }
        OutlineRow::Item { idx, child_idx, item_idx } => {
            let text = if let Some(c) = child_idx {
                if selected && app.constitution.item_editing {
                    app.constitution.item_input.clone()
                } else {
                    app.constitution.sections[idx].children[c].items[item_idx].clone()
                }
            } else if selected && app.constitution.item_editing {
                app.constitution.item_input.clone()
            } else {
                app.constitution.sections[idx].items[item_idx].clone()
            };
            let indent = if child_idx.is_some() { "      " } else { "    " };
            Line::from(Span::styled(format!("{}{}• {}", prefix, indent, text), base_style))
        }
    }
}

fn render_creator(frame: &mut Frame, area: Rect, app: &App) {
    use raios_surface_tui::app::state::CreatorStep;
    let c = &app.constitution.creator;
    let mut lines = vec![
        Line::from(Span::styled(" CONSTITUTION CREATOR", Style::new().fg(MID).bold())),
        Line::from(""),
    ];
    match c.step {
        CreatorStep::ChooseTarget => {
            lines.push(Line::from(" [p] Project-specific file   [g] Append to Global constitution (requires confirm)"));
        }
        CreatorStep::Notes => {
            lines.push(Line::from(" Notes to append (as a new \"## Project-Specific Rules\" section):"));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(format!(" {}█", c.notes_input), Style::new().fg(GREEN))));
        }
        CreatorStep::Preview => {
            lines.push(Line::from(if c.target_is_global {
                " Preview — [Enter] to continue to confirmation, [Esc] to cancel:"
            } else {
                " Preview — [Enter] to save, [Esc] to cancel:"
            }));
            lines.push(Line::from(""));
            if c.target_is_global {
                lines.push(Line::from(Span::styled(
                    " Appended to the end of the existing global constitution (nothing is removed):",
                    Style::new().fg(AMBER),
                )));
            } else {
                lines.push(Line::from("@/home/alaz/AGENT_CONSTITUTION.md"));
            }
            lines.push(Line::from(""));
            lines.push(Line::from("## Project-Specific Rules"));
            for line in c.notes_input.lines() {
                lines.push(Line::from(line.to_string()));
            }
        }
        CreatorStep::ConfirmGlobal => {
            lines.push(Line::from(Span::styled(
                " WARNING: This appends a new section to AGENT_CONSTITUTION.md — the single file every agent reads.",
                Style::new().fg(AMBER).bold(),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(" [y] Confirm and continue to save   [n / Esc] Back / cancel"));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
