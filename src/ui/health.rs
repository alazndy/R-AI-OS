use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
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

    // Header
    let status = if app.is_checking_health {
        Span::styled(" Checking projects...", Style::new().fg(AMBER).bold())
    } else {
        Span::styled(
            format!(" {} projects checked", app.health_report.len()),
            Style::new().fg(GREEN),
        )
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  HEALTH DASHBOARD", Style::new().fg(MID).bold()),
            status,
        ]),
        Line::from(Span::styled(
            "  Constitution compliance + git status across all projects",
            Style::new().fg(DIM),
        )),
    ])
    .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(DIM)));
    frame.render_widget(header, header_area);

    // Content
    if app.is_checking_health && app.health_report.is_empty() {
        let msg = Paragraph::new(Span::styled(
            "  Running checks...",
            Style::new().fg(DIM).italic(),
        ));
        frame.render_widget(msg, content_area);
        return;
    }

    let visible = content_area.height as usize;
    let scroll = app.health_cursor.saturating_sub(visible / 2);
    let mut lines: Vec<Line> = Vec::new();

    for (i, h) in app.health_report.iter().enumerate().skip(scroll).take(visible) {
        let selected = i == app.health_cursor;

        let git_span = match h.git_dirty {
            Some(true)  => Span::styled(" ●dirty ", Style::new().fg(AMBER)),
            Some(false) => Span::styled(" ○clean ", Style::new().fg(GREEN)),
            None        => Span::styled(" -      ", Style::new().fg(DIM)),
        };

        let score_color = match h.compliance_score {
            Some(s) if s >= 80 => GREEN,
            Some(s) if s >= 60 => AMBER,
            Some(_) => RED,
            None => DIM,
        };
        let score_str = h.compliance_score
            .map(|s| format!("{:3}/100 {}", s, h.compliance_grade))
            .unwrap_or_else(|| "   -     ".into());

        let mem_span = if h.has_memory {
            Span::styled(" ✓mem", Style::new().fg(GREEN))
        } else {
            Span::styled(" ✗mem", Style::new().fg(RED))
        };

        let issues_span = if h.constitution_issues.is_empty() {
            Span::styled(" ✓ const", Style::new().fg(GREEN))
        } else {
            Span::styled(
                format!(" ⚠ {} issues", h.constitution_issues.len()),
                Style::new().fg(AMBER),
            )
        };

        let graph_span = if h.graphify_done {
            Span::styled(" ✓graph ", Style::new().fg(GREEN))
        } else {
            Span::styled(" ✗graph ", Style::new().fg(DIM))
        };

        // Security score
        let sec_span = match h.security_score {
            Some(s) => {
                let grade = h.security_grade.as_deref().unwrap_or("-");
                let col = match s {
                    90..=100 => GREEN,
                    75..=89  => CYAN,
                    50..=74  => AMBER,
                    _        => RED,
                };
                let crit_tag = if h.security_critical > 0 {
                    format!(" ⚠{}", h.security_critical)
                } else {
                    String::new()
                };
                Span::styled(
                    format!(" 🔒{}/100{}{} ", s, grade, crit_tag),
                    Style::new().fg(col).bold(),
                )
            }
            None => Span::styled(" 🔒— ", Style::new().fg(DIM)),
        };

        let sc = project_status_color(&h.status);
        let name_color = if selected { GREEN } else { MID };
        let prefix = if selected { "  ▶ " } else { "    " };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::new().fg(GREEN)),
            Span::styled(format!("{:<24}", h.name), Style::new().fg(name_color).bold()),
            git_span,
            Span::styled(score_str, Style::new().fg(score_color)),
            sec_span,
            mem_span,
        ]));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), content_area);

    // Summary footer
    let total = app.health_report.len();
    let dirty = app.health_report.iter().filter(|h| h.git_dirty == Some(true)).count();
    let no_mem = app.health_report.iter().filter(|h| !h.has_memory).count();
    let avg_score = if total > 0 {
        app.health_report.iter()
            .filter_map(|h| h.compliance_score)
            .map(|s| s as usize)
            .sum::<usize>() / total.max(1)
    } else { 0 };

    let sec_scanned = app.health_report.iter().filter(|h| h.security_score.is_some()).count();
    let sec_critical = app.health_report.iter().map(|h| h.security_critical).sum::<usize>();
    let avg_sec = if sec_scanned > 0 {
        app.health_report.iter().filter_map(|h| h.security_score).map(|s| s as usize).sum::<usize>() / sec_scanned
    } else { 0 };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {}/{} dirty ", dirty, total), Style::new().fg(if dirty > 0 { AMBER } else { GREEN })),
        Span::styled(format!(" comp:{}/100 ", avg_score), Style::new().fg(MID)),
        if sec_scanned > 0 {
            Span::styled(
                format!(" 🔒{}/100 ⚠crit:{} ", avg_sec, sec_critical),
                Style::new().fg(if sec_critical > 0 { RED } else { GREEN }),
            )
        } else {
            Span::styled(" 🔒— (raios security çalıştır)", Style::new().fg(DIM))
        },
        Span::styled("  [↑↓] nav  [Enter] open  [Esc] back", Style::new().fg(DIM)),
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
            if let Some(ref path) = tool.path {
                lines.push(Line::from(vec![
                    Span::styled("      Path: ", Style::new().fg(DIM)),
                    Span::styled(path.to_string_lossy(), Style::new().fg(DIM).italic()),
                ]));
            }
        }
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("  ◈ ACTIVE API KEYS (ENV)", Style::new().fg(GREEN).bold())));
        if report.env_keys.is_empty() {
            lines.push(Line::from(Span::styled("    No global API keys detected in environment variables.", Style::new().fg(DIM))));
        } else {
            for key in &report.env_keys {
                lines.push(Line::from(vec![
                    Span::styled("    ✓ ", Style::new().fg(GREEN)),
                    Span::styled(key, Style::new().fg(MID)),
                ]));
            }
        }
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("  ◈ MODEL LOCATIONS & CACHE", Style::new().fg(GREEN).bold())));
        for model in &report.local_models {
            lines.push(Line::from(vec![
                Span::styled("    📂 ", Style::new().fg(AMBER)),
                Span::styled(model, Style::new().fg(MID)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled("  No system scan performed yet.", Style::new().fg(DIM))));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  Run /scan-system or /audit to start a deep scan.", Style::new().fg(CYAN).italic())));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}


