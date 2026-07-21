//! Session Canvas — symbolic short-term compression over session_events.
//!
//! Folds a session's event stream into a compact Mermaid flowchart. Long
//! payloads are truncated in the label but stay addressable in the DB via
//! `se:<event_id>` refs, so no compression step is irreversible.

use crate::session::SessionEvent;

pub struct CanvasNode {
    pub label: String,
    pub count: usize,
    pub first_ref: i64,
    pub detail: Option<String>,
}

const DETAIL_MAX: usize = 60;

/// Collapse consecutive runs of the same event_type into single nodes.
pub fn fold_events(events: &[SessionEvent]) -> Vec<CanvasNode> {
    let mut nodes: Vec<CanvasNode> = Vec::new();
    for ev in events {
        match nodes.last_mut() {
            Some(last) if last.label == ev.event_type => {
                last.count += 1;
            }
            _ => {
                let detail = if ev.data.is_empty() {
                    None
                } else {
                    let chars: Vec<char> = ev.data.chars().collect();
                    if chars.len() > DETAIL_MAX {
                        Some(format!(
                            "{}…",
                            chars[..DETAIL_MAX].iter().collect::<String>()
                        ))
                    } else {
                        Some(ev.data.clone())
                    }
                };
                nodes.push(CanvasNode {
                    label: ev.event_type.clone(),
                    count: 1,
                    first_ref: ev.id,
                    detail,
                });
            }
        }
    }
    nodes
}

/// Render folded nodes as a Mermaid flowchart.
pub fn to_mermaid(session_id: &str, nodes: &[CanvasNode]) -> String {
    let mut out = String::from("flowchart TD\n");
    let short_id: String = session_id.chars().take(8).collect();
    out.push_str(&format!("    S((session {short_id}))\n"));
    let mut prev = "S".to_string();
    for (i, n) in nodes.iter().enumerate() {
        let id = format!("N{i}");
        let count = if n.count > 1 {
            format!(" ×{}", n.count)
        } else {
            String::new()
        };
        let detail = n
            .detail
            .as_deref()
            .map(|d| format!(": {}", d.replace('"', "'")))
            .unwrap_or_default();
        out.push_str(&format!(
            "    {prev} --> {id}[\"{}{count}{detail} (se:{})\"]\n",
            n.label, n.first_ref
        ));
        prev = id;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: i64, t: &str, data: &str) -> SessionEvent {
        SessionEvent {
            id,
            session_id: "s1".into(),
            event_type: t.into(),
            data: data.into(),
            timestamp: "2026-07-09 12:00:00".into(),
        }
    }

    #[test]
    fn fold_collapses_consecutive_runs() {
        let events = vec![
            ev(1, "file_read", "a.rs"),
            ev(2, "file_read", "b.rs"),
            ev(3, "tool_call", "cargo test"),
            ev(4, "file_read", "c.rs"),
        ];
        let nodes = fold_events(&events);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].count, 2);
        assert_eq!(nodes[0].first_ref, 1);
        assert_eq!(nodes[1].label, "tool_call");
        assert_eq!(nodes[2].count, 1);
    }

    #[test]
    fn fold_truncates_long_payloads_keeping_ref() {
        let long = "x".repeat(200);
        let nodes = fold_events(&[ev(7, "tool_call", &long)]);
        let detail = nodes[0].detail.as_ref().unwrap();
        assert!(detail.chars().count() <= 61); // 60 + ellipsis
        assert!(detail.ends_with('…'));
        assert_eq!(nodes[0].first_ref, 7); // full payload reachable: se:7
    }

    #[test]
    fn mermaid_output_shape() {
        let nodes = fold_events(&[ev(1, "file_read", "a.rs"), ev(2, "file_read", "b.rs")]);
        let m = to_mermaid("620c3156-abcd", &nodes);
        assert!(m.starts_with("flowchart TD"));
        assert!(m.contains("session 620c3156"));
        assert!(m.contains("file_read ×2"));
        assert!(m.contains("(se:1)"));
    }
}
