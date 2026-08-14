//! Explore route rendering (read-only workspace search, traces, and daemon logs).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::store::Store;

/// Shared Explore-route rectangles used by rendering and mouse hit testing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExploreRouteLayout {
    pub(crate) search: Rect,
    pub(crate) results: Rect,
    pub(crate) traces: Rect,
    pub(crate) logs: Rect,
}

/// Calculates stable Explore panels for one dashboard content area.
pub(crate) fn explore_route_layout(area: Rect) -> ExploreRouteLayout {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(6)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(columns[1]);

    ExploreRouteLayout {
        search: left[0],
        results: left[1],
        traces: right[0],
        logs: right[1],
    }
}

/// Renders the Explore route panel view.
pub fn render_explore_route(f: &mut Frame, area: Rect, store: &Store) {
    let layout = explore_route_layout(area);

    // 1. Search input bar
    let search_block = Block::default()
        .borders(Borders::ALL)
        .title(" Workspace Search [/] edit · Enter run · Esc cancel ")
        .border_style(Style::default().fg(if store.explore_search.is_editing {
            Color::Yellow
        } else {
            Color::Cyan
        }));

    let cursor = if store.explore_search.is_editing {
        "_"
    } else {
        ""
    };
    let status = store
        .explore_search
        .status
        .as_deref()
        .map(|status| format!("  ·  {status}"))
        .unwrap_or_default();
    let search_p = Paragraph::new(format!(
        "  {}{}{}",
        store.explore_search.query, cursor, status
    ))
    .block(search_block);
    f.render_widget(search_p, layout.search);

    let result_items: Vec<ListItem> = if store.explore_results().is_empty() {
        let hint = if store.explore_search.query.is_empty() {
            "  Type / to search the indexed workspace."
        } else {
            "  No matching indexed files. Refine the query with /."
        };
        vec![ListItem::new(hint)]
    } else {
        store
            .explore_results()
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let bg = if !store.right_panel_focus && store.cursor == index {
                    Color::DarkGray
                } else {
                    Color::Reset
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{}:{} ", result.file_path, result.line_number),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&result.snippet, Style::default().fg(Color::Gray)),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let results_block = Block::default()
        .borders(Borders::ALL)
        .title(" Search Results · Enter opens selected local file ")
        .border_style(Style::default().fg(if store.right_panel_focus {
            Color::DarkGray
        } else {
            Color::Green
        }));
    f.render_widget(List::new(result_items).block(results_block), layout.results);

    // 2. Tool Traces Timeline & Results
    let trace_items: Vec<ListItem> = if store.snapshot.explore.recent_traces.is_empty() {
        vec![ListItem::new("  • No tool trace history recorded yet.")]
    } else {
        store
            .snapshot
            .explore
            .recent_traces
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let status_color = if t.status == "SUCCESS" {
                    Color::Green
                } else {
                    Color::Red
                };
                let bg = if !store.right_panel_focus && store.cursor == i {
                    Color::DarkGray
                } else {
                    Color::Reset
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", t.status),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        &t.tool_name,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" ({}ms)", t.duration_ms),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let traces_block = Block::default()
        .borders(Borders::ALL)
        .title(" Tool Execution Traces Timeline ")
        .border_style(Style::default().fg(if !store.right_panel_focus {
            Color::Green
        } else {
            Color::Cyan
        }));

    let traces_list = List::new(trace_items).block(traces_block);
    f.render_widget(traces_list, layout.traces);

    // 3. Live Logs Replay
    let log_items: Vec<ListItem> = if store.snapshot.explore.recent_logs.is_empty() {
        vec![ListItem::new("  • No logs available.")]
    } else {
        store
            .snapshot
            .explore
            .recent_logs
            .iter()
            .rev()
            .take(20)
            .enumerate()
            .map(|(i, l)| {
                let bg = if store.right_panel_focus && store.cursor == i {
                    Color::DarkGray
                } else {
                    Color::Reset
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", l.category),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(&l.message, Style::default().fg(Color::Gray)),
                ]))
                .style(Style::default().bg(bg))
            })
            .collect()
    };

    let logs_block = Block::default()
        .borders(Borders::ALL)
        .title(" Daemon Log Stream Replay ")
        .border_style(Style::default().fg(if store.right_panel_focus {
            Color::Green
        } else {
            Color::DarkGray
        }));

    let logs_list = List::new(log_items).block(logs_block);
    f.render_widget(logs_list, layout.logs);
}

#[cfg(test)]
mod tests {
    use super::explore_route_layout;
    use ratatui::layout::Rect;

    #[test]
    fn layout_keeps_search_and_results_in_the_left_column() {
        let layout = explore_route_layout(Rect::new(0, 0, 120, 36));
        assert!(layout.search.width < 120);
        assert_eq!(layout.search.x, layout.results.x);
        assert!(layout.results.y > layout.search.y);
        assert!(layout.traces.x > layout.results.x);
        assert!(layout.logs.y >= layout.traces.y);
    }
}
