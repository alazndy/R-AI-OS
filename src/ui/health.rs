use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Table, Row, Cell, TableState},
};
use crate::app::App;
use crate::ui::*;

pub fn render_diagnostics(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(" SYSTEM DIAGNOSTICS", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    for (name, pass) in &app.boot_results {
        let (mark, color) = if *pass { ("✓", GREEN) } else { ("✗", RED) };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", mark), Style::new().fg(color)),
            Span::styled(name.as_str(), Style::new().fg(MID)),
        ]));
    }

    if let Some(ref report) = app.compliance {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(" PROJECT COMPLIANCE REPORT", Style::new().fg(MID).bold())));
        lines.push(Line::from(format!("  Score: {}/100", report.score)));
        lines.push(Line::from(""));

        if report.violations.is_empty() {
            lines.push(Line::from(Span::styled("  ✓ No compliance issues found. Excellent work!", Style::new().fg(GREEN))));
        } else {
            for v in &report.violations {
                lines.push(Line::from(vec![
                    Span::styled("  ⚠ ", Style::new().fg(AMBER)),
                    Span::styled(format!("Line {}: ", v.line), Style::new().fg(DIM)),
                    Span::styled(v.rule, Style::new().fg(MID)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  Press 'f' to attempt Auto-Fix with Claude", Style::new().fg(CYAN).italic())));
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

pub fn render_health_view(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(PANEL_BG)), area);

    let [header_area, content_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .areas(area);

    // --- Header ---
    let status_text = if app.is_checking_health {
        Span::styled(" Checking projects...", Style::new().fg(AMBER).bold())
    } else {
        Span::styled(
            format!(" {} projects checked", app.health_report.len()),
            Style::new().fg(GREEN),
        )
    };
    let header_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  HEALTH DASHBOARD", Style::new().fg(MID).bold()),
            status_text,
        ]),
        Line::from(Span::styled(
            "  Constitution compliance + git status across all projects",
            Style::new().fg(DIM),
        )),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(header_widget, header_area);

    // --- Table Content ---
    let header_style = Style::new().fg(MID).bold();
    let selected_style = Style::new().bg(Color::Rgb(0, 40, 30)).fg(GREEN).bold();

    let rows: Vec<Row> = app.health_report.iter().enumerate().map(|(i, h)| {
        let selected = i == app.health_cursor;
        
        let git_icon = match h.git_dirty {
            Some(true)  => "● dirty",
            Some(false) => "○ clean",
            None        => "-      ",
        };
        let git_color = match h.git_dirty {
            Some(true)  => AMBER,
            Some(false) => GREEN,
            _           => DIM,
        };

        let score_color = match h.compliance_score {
            Some(s) if s >= 80 => GREEN,
            Some(s) if s >= 60 => AMBER,
            _                  => RED,
        };
        let comp_text = h.compliance_score
            .map(|s| format!("{} {}", s, h.compliance_grade))
            .unwrap_or_else(|| "—".into());

        let sec_text = match h.security_score {
            Some(s) => format!("🔒 {} {}", s, h.security_grade.as_deref().unwrap_or("-")),
            None    => "—".into(),
        };

        let mem_text = if h.has_memory { "✓ mem" } else { "✗ mem" };
        let mem_color = if h.has_memory { GREEN } else { RED };

        let sig_text = if h.has_sigmap { "✓ sig" } else { "✗ sig" };
        let sig_color = if h.has_sigmap { GREEN } else { RED };

        let type_text = if h.path.join("Cargo.toml").exists() { "Rust" }
                        else if h.path.join("package.json").exists() { "Node" }
                        else { "Other" };

        let rf_color = match h.refactor_grade.as_str() {
            "A" | "B" => GREEN,
            "C"       => AMBER,
            _         => RED,
        };
        let rf_text = if h.refactor_high_count > 0 {
            format!("{} ⚠{}", h.refactor_grade, h.refactor_high_count)
        } else {
            h.refactor_grade.clone()
        };

        Row::new(vec![
            Cell::from(if selected { "▶" } else { " " }),
            Cell::from(Span::styled(git_icon, Style::new().fg(git_color))),
            Cell::from(Span::styled(h.name.as_str(), Style::new().bold())),
            Cell::from(Span::styled(comp_text, Style::new().fg(score_color))),
            Cell::from(Span::styled(sec_text, Style::new().fg(MID))),
            Cell::from(Span::styled(rf_text, Style::new().fg(rf_color))),
            Cell::from(Span::styled(mem_text, Style::new().fg(mem_color))),
            Cell::from(Span::styled(sig_text, Style::new().fg(sig_color))),
            Cell::from(Span::styled(type_text, Style::new().fg(DIM))),
        ]).style(if selected { selected_style } else { Style::new().fg(MID) })
    }).collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(2),   // Selector
            Constraint::Length(10),  // Git
            Constraint::Min(20),     // Name
            Constraint::Length(12),  // Compliance
            Constraint::Length(15),  // Security
            Constraint::Length(10),  // Refactor
            Constraint::Length(8),   // Memory
            Constraint::Length(8),   // Sigmap
            Constraint::Length(8),   // Type
        ],
    )
    .header(
        Row::new(vec!["", "GIT", "PROJECT NAME", "COMPLIANCE", "SECURITY", "REFACTOR", "MEM", "SIG", "TYPE"])
            .style(header_style)
            .bottom_margin(1)
    )
    .column_spacing(2);

    let mut state = TableState::default();
    state.select(Some(app.health_cursor));
    frame.render_stateful_widget(table, content_area, &mut state);

    // --- Footer ---
    let total = app.health_report.len();
    let dirty = app.health_report.iter().filter(|h| h.git_dirty == Some(true)).count();
    let avg_score = if total > 0 {
        app.health_report.iter()
            .filter_map(|h| h.compliance_score)
            .map(|s| s as usize)
            .sum::<usize>() / total.max(1)
    } else { 0 };

    let sec_scanned = app.health_report.iter().filter(|h| h.security_score.is_some()).count();
    let sec_critical = app.health_report.iter().map(|h| h.security_critical).sum::<usize>();
    let rf_high_total = app.health_report.iter().map(|h| h.refactor_high_count).sum::<usize>();

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {}/{} dirty ", dirty, total), Style::new().fg(if dirty > 0 { AMBER } else { GREEN })),
        Span::styled(format!(" comp:{}/100 ", avg_score), Style::new().fg(MID)),
        if sec_scanned > 0 {
            Span::styled(format!(" 🔒crit:{} ", sec_critical), Style::new().fg(if sec_critical > 0 { RED } else { GREEN }))
        } else {
            Span::styled(" 🔒— ", Style::new().fg(DIM))
        },
        Span::styled(
            format!(" ♻ high:{} ", rf_high_total),
            Style::new().fg(if rf_high_total > 0 { AMBER } else { GREEN }),
        ),
        Span::styled("  [↑↓] nav  [r] refresh  [Esc] back", Style::new().fg(DIM)),
    ]))
    .block(Block::new().borders(Borders::TOP).border_style(Style::new().fg(DIM)));
    frame.render_widget(footer, footer_area);
}

pub fn render_system_audit(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled(" AI SYSTEM AUDIT & INVENTORY", Style::new().fg(MID).bold())),
        Line::from(""),
    ];

    if app.is_scanning_system {
        lines.push(Line::from(Span::styled("  ⚡ Scanning entire system... This may take a moment.", Style::new().fg(AMBER).bold())));
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
        return;
    }

    if let Some(ref report) = app.system_report {
        lines.push(Line::from(Span::styled("  ◈ AI TOOLS & SERVICES", Style::new().fg(GREEN).bold())));
        for tool in &report.tools {
            let status_span = match tool.status {
                crate::system_scan::ToolStatus::Running => Span::styled(" [RUNNING]", Style::new().fg(GREEN).bold()),
                crate::system_scan::ToolStatus::Installed => Span::styled(" [INSTALLED]", Style::new().fg(CYAN)),
                crate::system_scan::ToolStatus::Missing => Span::styled(" [MISSING]", Style::new().fg(DIM)),
                crate::system_scan::ToolStatus::Error(ref e) => Span::styled(format!(" [ERROR: {}]", e), Style::new().fg(RED)),
            };
            
            lines.push(Line::from(vec![
                Span::styled(format!("    • {:<20}", tool.name), Style::new().fg(MID)),
                status_span,
            ]));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
