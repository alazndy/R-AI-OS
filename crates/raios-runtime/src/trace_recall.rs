use rusqlite::Connection;
use std::path::Path;

const MAX_FIELD_CHARS: usize = 180;

pub fn record_post_run_review_trace(
    conn: &Connection,
    agent: &str,
    project_dir: &str,
    success: bool,
    review: &crate::session_review::PostRunReview,
    related_task_id: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    if success && review.risks.is_empty() && review.learned.is_empty() {
        return Ok(None);
    }

    let project = project_key_from_path(project_dir).unwrap_or_else(|| project_dir.to_string());
    let agent_identity = canonical_trace_agent(agent);
    let command = format!("raios run {agent}");
    let context = review
        .changed
        .as_deref()
        .unwrap_or("post-run session review");
    let outcome = if success {
        format!(
            "session completed; tests_run={}",
            if review.tests_run_during_session {
                "yes"
            } else {
                "no"
            }
        )
    } else {
        "session ended non-zero".to_string()
    };
    let error_summary = if review.risks.is_empty() {
        if success {
            String::new()
        } else {
            "agent run failed".to_string()
        }
    } else {
        review.risks.join("; ")
    };
    let fix_summary = review.learned.join("; ");
    let tags = if success {
        r#"["auto","session_review"]"#
    } else {
        r#"["auto","session_review","failure"]"#
    };

    for value in [
        project.as_str(),
        agent_identity.as_str(),
        command.as_str(),
        context,
        outcome.as_str(),
        error_summary.as_str(),
        fix_summary.as_str(),
    ] {
        if let Some(label) = raios_core::security::looks_like_secret(value) {
            let id = raios_core::db::tool_trace_record_secret_refusal(
                conn,
                &project,
                &agent_identity,
                label,
            )?;
            return Ok(Some(id));
        }
    }

    raios_core::db::tool_trace_insert(
        conn,
        raios_core::db::ToolTraceInsert {
            project: &project,
            agent: &agent_identity,
            command: &command,
            context,
            outcome: &outcome,
            error_summary: &error_summary,
            fix_summary: &fix_summary,
            tags_json: tags,
            success,
            confidence: 0.55,
            related_task_id,
        },
    )
}

pub fn relevant_trace_block(
    conn: &Connection,
    project_path: Option<&str>,
    query: &str,
    limit: usize,
) -> Option<String> {
    let project_key = project_path.and_then(project_key_from_path);
    let query_text = if query.trim().is_empty() {
        project_key.as_deref().unwrap_or_default()
    } else {
        query.trim()
    };
    if query_text.is_empty() {
        return None;
    }

    let mut rows = search(conn, query_text, project_key.as_deref(), limit)?;

    if rows.is_empty() {
        for token in query_text
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|token| token.chars().count() >= 4)
            .take(8)
        {
            rows = search(conn, token, project_key.as_deref(), limit)?;
            if !rows.is_empty() {
                break;
            }
        }
    }

    if rows.is_empty() && project_key.is_some() {
        rows = search(
            conn,
            project_key.as_deref().unwrap_or_default(),
            project_key.as_deref(),
            limit,
        )?;
    }

    if rows.is_empty() && project_key.is_some() {
        rows = search_with_preference(conn, query_text, None, project_key.as_deref(), limit)?;
    }

    if rows.is_empty() {
        return None;
    }

    let mut out = String::from("[Relevant trace memory]");
    for row in rows.into_iter().take(limit) {
        out.push_str("\n- ");
        out.push_str(&format!(
            "{} [{}] {}",
            row.project,
            if row.success { "success" } else { "failure" },
            compact(&row.command)
        ));
        if !row.error_summary.is_empty() {
            out.push_str(&format!("\n  error: {}", compact(&row.error_summary)));
        }
        if !row.fix_summary.is_empty() {
            out.push_str(&format!("\n  fix: {}", compact(&row.fix_summary)));
        }
        if !row.outcome.is_empty() {
            out.push_str(&format!("\n  outcome: {}", compact(&row.outcome)));
        }
    }
    Some(out)
}

fn search(
    conn: &Connection,
    text: &str,
    project: Option<&str>,
    limit: usize,
) -> Option<Vec<raios_core::db::ToolTraceRow>> {
    search_with_preference(conn, text, project, project, limit)
}

fn search_with_preference(
    conn: &Connection,
    text: &str,
    project: Option<&str>,
    preferred_project: Option<&str>,
    limit: usize,
) -> Option<Vec<raios_core::db::ToolTraceRow>> {
    raios_core::db::tool_trace_search(
        conn,
        raios_core::db::ToolTraceQuery {
            text,
            project,
            preferred_project,
            success_only: true,
            tag: None,
            limit,
        },
    )
    .ok()
}

fn project_key_from_path(path: &str) -> Option<String> {
    let direct = Path::new(path);
    direct
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .or_else(|| {
            let trimmed = path.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

fn canonical_trace_agent(agent: &str) -> String {
    match agent.to_ascii_lowercase().as_str() {
        "claude" | "claude_kaira" => "claude_kaira".to_string(),
        "codex" | "codex_kaira" => "codex_kaira".to_string(),
        "opencode" | "opencode_kaira" => "opencode_kaira".to_string(),
        "antigravity" | "agy" | "antigravity_kaira" => "antigravity_kaira".to_string(),
        other => other.to_string(),
    }
}

fn compact(value: &str) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_FIELD_CHARS {
        return flat;
    }
    let mut out: String = flat
        .chars()
        .take(MAX_FIELD_CHARS.saturating_sub(1))
        .collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn
    }

    #[test]
    fn recall_falls_back_from_full_phrase_to_tokens() {
        let conn = conn();
        raios_core::db::tool_trace_insert(
            &conn,
            raios_core::db::ToolTraceInsert {
                project: "R-AI-OS",
                agent: "codex_kaira",
                command: "cargo test -p raios-core tool_trace",
                context: "trace memory implementation",
                outcome: "tests passed",
                error_summary: "tool trace recall regression",
                fix_summary: "use central schema and existing secret scanner",
                tags_json: r#"["trace"]"#,
                success: true,
                confidence: 0.9,
                related_task_id: None,
            },
        )
        .unwrap();

        let block = relevant_trace_block(
            &conn,
            Some("/home/alaz/dev/core/R-AI-OS"),
            "tool trace recall regression needs next implementation step",
            3,
        )
        .unwrap();
        assert!(block.contains("[Relevant trace memory]"));
        assert!(block.contains("existing secret scanner"));
    }

    #[test]
    fn post_run_review_trace_skips_empty_success() {
        let conn = conn();
        let review = crate::session_review::PostRunReview {
            changed: None,
            tests_run_during_session: true,
            risks: vec![],
            learned: vec![],
        };

        let id = record_post_run_review_trace(&conn, "codex", "/tmp/R-AI-OS", true, &review, None)
            .unwrap();

        assert!(id.is_none());
    }

    #[test]
    fn post_run_review_trace_records_learned_review() {
        let conn = conn();
        let review = crate::session_review::PostRunReview {
            changed: Some("src/lib.rs | 2 +".to_string()),
            tests_run_during_session: true,
            risks: vec![],
            learned: vec!["use trace recall before handoff".to_string()],
        };

        let id = record_post_run_review_trace(&conn, "codex", "/tmp/R-AI-OS", true, &review, None)
            .unwrap()
            .unwrap();
        let row = raios_core::db::tool_trace_get(&conn, &id).unwrap().unwrap();

        assert_eq!(row.project, "R-AI-OS");
        assert_eq!(row.agent, "codex_kaira");
        assert!(row.fix_summary.contains("trace recall"));
        assert!(row.tags_json.contains("session_review"));
    }
}
