use super::TraceAction;

pub(super) fn cmd_trace(action: TraceAction, json: bool) {
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Trace failed: could not open DB: {e}");
            std::process::exit(1);
        }
    };

    match action {
        TraceAction::Record {
            project,
            command,
            context,
            outcome,
            error,
            fix,
            tag,
            success,
            confidence,
            task_id,
        } => {
            let agent =
                std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());
            if let Some(label) =
                secret_label(&[&project, &command, &context, &outcome, &error, &fix])
            {
                let refusal_id = raios_core::db::tool_trace_record_secret_refusal(
                    &conn, &project, &agent, label,
                )
                .ok();
                eprintln!(
                    "Trace refused: input looks like it contains a {label}. \
                     Remove it and retry; traces are stored in plain text."
                );
                if json {
                    let out = serde_json::json!({
                        "ok": false,
                        "error": "secret_like_input",
                        "label": label,
                        "refusal_trace_id": refusal_id,
                    });
                    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                }
                std::process::exit(1);
            }

            let tags_json = serde_json::to_string(&tag).unwrap_or_else(|_| "[]".into());
            let confidence = confidence.clamp(0.0, 1.0);
            let trace = raios_core::db::ToolTraceInsert {
                project: &project,
                agent: &agent,
                command: &command,
                context: &context,
                outcome: &outcome,
                error_summary: &error,
                fix_summary: &fix,
                tags_json: &tags_json,
                success,
                confidence,
                related_task_id: task_id.as_deref(),
            };
            match raios_core::db::tool_trace_insert(&conn, trace) {
                Ok(Some(id)) => {
                    if json {
                        println!("{}", serde_json::json!({"ok": true, "id": id}));
                    } else {
                        println!("Trace recorded: {id}");
                    }
                }
                Ok(None) => {
                    if json {
                        println!("{}", serde_json::json!({"ok": true, "deduped": true}));
                    } else {
                        println!("Trace already exists; skipped duplicate.");
                    }
                }
                Err(e) => {
                    eprintln!("Trace failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        TraceAction::Search {
            query,
            project,
            success_only,
            tag,
            limit,
        } => match raios_core::db::tool_trace_search(
            &conn,
            raios_core::db::ToolTraceQuery {
                text: &query,
                project: project.as_deref(),
                preferred_project: project.as_deref(),
                success_only,
                tag: tag.as_deref(),
                limit,
            },
        ) {
            Ok(rows) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&rows).unwrap_or_default()
                    );
                    return;
                }
                if rows.is_empty() {
                    println!("No matching traces.");
                    return;
                }
                for row in rows {
                    println!(
                        "{}  [{}] {}  {}",
                        row.id, row.project, row.agent, row.created_at
                    );
                    println!("  command: {}", row.command);
                    if !row.error_summary.is_empty() {
                        println!("  error:   {}", row.error_summary);
                    }
                    if !row.fix_summary.is_empty() {
                        println!("  fix:     {}", row.fix_summary);
                    }
                    if !row.outcome.is_empty() {
                        println!("  outcome: {}", row.outcome);
                    }
                    println!();
                }
            }
            Err(e) => {
                eprintln!("Trace search failed: {e}");
                std::process::exit(1);
            }
        },
        TraceAction::KgExport {
            query,
            project,
            success_only,
            limit,
        } => match raios_core::db::tool_trace_search(
            &conn,
            raios_core::db::ToolTraceQuery {
                text: query.as_deref().unwrap_or(""),
                project: project.as_deref(),
                preferred_project: project.as_deref(),
                success_only,
                tag: None,
                limit,
            },
        ) {
            Ok(rows) => {
                let facts = rows.iter().flat_map(trace_to_kg_facts).collect::<Vec<_>>();
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&facts).unwrap_or_default()
                    );
                } else {
                    println!(
                        "Exported {} KG fact(s) from {} trace row(s).",
                        facts.len(),
                        rows.len()
                    );
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&facts).unwrap_or_default()
                    );
                }
            }
            Err(e) => {
                eprintln!("Trace KG export failed: {e}");
                std::process::exit(1);
            }
        },
        TraceAction::Forget { id } => match raios_core::db::tool_trace_forget(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{}", serde_json::json!({"ok": true, "deleted": id}));
                } else {
                    println!("Trace deleted: {id}");
                }
            }
            Ok(false) => {
                eprintln!("Trace not found: {id}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Trace delete failed: {e}");
                std::process::exit(1);
            }
        },
    }
}

fn secret_label(fields: &[&str]) -> Option<&'static str> {
    fields
        .iter()
        .find_map(|field| raios_core::security::looks_like_secret(field))
}

fn trace_to_kg_facts(row: &raios_core::db::ToolTraceRow) -> Vec<serde_json::Value> {
    let subject = format!("trace:{}", row.id);
    let mut facts = vec![
        kg_fact(&subject, "project", &row.project, &row.id, &row.created_at),
        kg_fact(&subject, "agent", &row.agent, &row.id, &row.created_at),
        kg_fact(&subject, "command", &row.command, &row.id, &row.created_at),
        kg_fact(
            &subject,
            "success",
            if row.success { "true" } else { "false" },
            &row.id,
            &row.created_at,
        ),
    ];
    if !row.error_summary.is_empty() {
        facts.push(kg_fact(
            &subject,
            "observed_error",
            &row.error_summary,
            &row.id,
            &row.created_at,
        ));
    }
    if !row.fix_summary.is_empty() {
        facts.push(kg_fact(
            &subject,
            "resolved_by",
            &row.fix_summary,
            &row.id,
            &row.created_at,
        ));
    }
    if !row.outcome.is_empty() {
        facts.push(kg_fact(
            &subject,
            "outcome",
            &row.outcome,
            &row.id,
            &row.created_at,
        ));
    }
    facts
}

fn kg_fact(
    subject: &str,
    predicate: &str,
    object: &str,
    trace_id: &str,
    valid_from: &str,
) -> serde_json::Value {
    serde_json::json!({
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "valid_from": valid_from,
        "source": {
            "kind": "raios_tool_trace",
            "trace_id": trace_id
        }
    })
}
