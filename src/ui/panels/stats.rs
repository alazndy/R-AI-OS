use crate::app::App;
use crate::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_quick_stats(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(" PORTFOLIO ", Style::new().fg(DIM)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rf_high_total: usize = app
        .health.report
        .iter()
        .map(|h| h.refactor_high_count)
        .sum();
    let rf_projects_with_issues: Vec<&str> = app
        .health.report
        .iter()
        .filter(|h| h.refactor_high_count > 0)
        .map(|h| h.name.as_str())
        .take(3)
        .collect();
    let rf_projects_total = app
        .health.report
        .iter()
        .filter(|h| h.refactor_high_count > 0)
        .count();

    let lines = if app.system.is_computing_stats {
        vec![Line::from(Span::styled(
            "  computing...",
            Style::new().fg(DIM).italic(),
        ))]
    } else if let Some(ref s) = app.system.stats_cache {
        let total = s.total;
        let grade_pct = |n: usize| if total > 0 { n * 100 / total } else { 0 };

        let mut result = vec![
            Line::from(vec![
                Span::styled(format!("  {:>3} projects", total), Style::new().fg(MID)),
                Span::styled(
                    format!("  {} dirty", s.dirty),
                    if s.dirty > 0 {
                        Style::new().fg(AMBER)
                    } else {
                        Style::new().fg(GREEN)
                    },
                ),
                Span::styled(
                    format!("  {} no-mem", s.no_memory),
                    Style::new().fg(if s.no_memory > 0 { RED } else { DIM }),
                ),
                Span::styled(
                    format!("  {} no-sig", s.no_sigmap),
                    Style::new().fg(if s.no_sigmap > 0 { RED } else { DIM }),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("  A:{:>3}% ", grade_pct(s.grade_a)),
                    Style::new().fg(GREEN),
                ),
                Span::styled(
                    format!("B:{:>3}% ", grade_pct(s.grade_b)),
                    Style::new().fg(CYAN),
                ),
                Span::styled(
                    format!("C:{:>3}% ", grade_pct(s.grade_c)),
                    Style::new().fg(AMBER),
                ),
                Span::styled(
                    format!("D:{:>3}%", grade_pct(s.grade_d)),
                    Style::new().fg(RED),
                ),
            ]),
            Line::from(vec![Span::styled(
                format!("  local-only:{}", s.no_github),
                Style::new().fg(DIM),
            )]),
        ];

        if rf_high_total > 0 {
            result.push(Line::from(""));
            result.push(Line::from(Span::styled(
                format!(
                    "  ⚠ REFACTOR: {} HIGH files across {} projects",
                    rf_high_total, rf_projects_total
                ),
                Style::new().fg(AMBER).bold(),
            )));
            for name in &rf_projects_with_issues {
                result.push(Line::from(Span::styled(
                    format!("    • {}", name),
                    Style::new().fg(AMBER),
                )));
            }
            if rf_projects_total > 3 {
                result.push(Line::from(Span::styled(
                    format!("    +{} more → [h] Health view", rf_projects_total - 3),
                    Style::new().fg(DIM).italic(),
                )));
            }
        }

        result
    } else {
        vec![Line::from(Span::styled(
            "  — no data —",
            Style::new().fg(DIM).italic(),
        ))]
    };

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}
