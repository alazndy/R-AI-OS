use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_scheduler(frame: &mut Frame, area: Rect, app: &App) {
    let _ = app;

    let block = Block::new()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" SCHEDULER ", Style::new().fg(CYAN).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Ok(data) = raios_surface_tui::app::load_scheduler_panel_data() else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Could not load scheduler snapshot.",
                Style::new().fg(RED),
            )),
            inner,
        );
        return;
    };
    let jobs = data.jobs;

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " AUTONOMOUS SCHEDULER",
            Style::new().fg(MID).bold(),
        )),
        Line::from(""),
    ];

    let active: Vec<_> = jobs.iter().filter(|j| j.status == "active").collect();
    let paused: Vec<_> = jobs.iter().filter(|j| j.status == "paused").collect();

    if active.is_empty() && paused.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No scheduled jobs.",
            Style::new().fg(DIM).italic(),
        )));
        lines.push(Line::from(Span::styled(
            "  Add one: raios cron add \"Title\" --every 24h --agent claude --task \"...\"",
            Style::new().fg(DIM),
        )));
    } else {
        if !active.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  ACTIVE  ({})", active.len()),
                Style::new().fg(GREEN).bold(),
            )));
            lines.push(Line::from(""));
            for job in &active {
                let id8 = &job.id[..8.min(job.id.len())];
                let title: String = job.title.chars().take(32).collect();
                let interval = fmt_interval(job.interval_secs);
                lines.push(Line::from(vec![
                    Span::styled(format!("  [{id8}] "), Style::new().fg(DIM)),
                    Span::styled(format!("{title:<32} "), Style::new().fg(MID)),
                    Span::styled(format!("{:<8} ", job.agent), Style::new().fg(CYAN)),
                    Span::styled(format!("/{interval}"), Style::new().fg(AMBER)),
                    Span::styled(format!("  runs:{}", job.run_count), Style::new().fg(DIM)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("             next: ", Style::new().fg(DIM)),
                    Span::styled(job.next_run_at.clone(), Style::new().fg(MID)),
                ]));
                if let Some(last) = &job.last_run_at {
                    lines.push(Line::from(vec![
                        Span::styled("             last: ", Style::new().fg(DIM)),
                        Span::styled(last.clone(), Style::new().fg(DIM)),
                    ]));
                }
                lines.push(Line::from(""));
            }
        }

        if !paused.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  PAUSED  ({})", paused.len()),
                Style::new().fg(AMBER).bold(),
            )));
            lines.push(Line::from(""));
            for job in &paused {
                let id8 = &job.id[..8.min(job.id.len())];
                let title: String = job.title.chars().take(32).collect();
                lines.push(Line::from(vec![
                    Span::styled(format!("  [{id8}] "), Style::new().fg(DIM)),
                    Span::styled(format!("{title:<32} "), Style::new().fg(DIM).italic()),
                    Span::styled(format!("{:<8}", job.agent), Style::new().fg(DIM)),
                    Span::styled(format!("  runs:{}", job.run_count), Style::new().fg(DIM)),
                ]));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn fmt_interval(secs: i64) -> String {
    if secs >= 86400 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}
