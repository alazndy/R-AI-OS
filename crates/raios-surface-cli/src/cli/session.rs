pub(super) fn cmd_sessions(agent: Option<&str>, top: usize, canvas: Option<&str>, json: bool) {
    if let Some(session_id) = canvas {
        let store = raios_runtime::session::SessionStore::new(
            raios_runtime::session::SessionStore::default_path(),
        );
        let events = store.events(session_id);
        if events.is_empty() {
            eprintln!("  No events for session {}", session_id);
            return;
        }
        let nodes = raios_runtime::session_canvas::fold_events(&events);
        println!(
            "{}",
            raios_runtime::session_canvas::to_mermaid(session_id, &nodes)
        );
        return;
    }
    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };
    let rows = match raios_core::db::cp_sessions_list(&conn, top) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Query error: {e}");
            return;
        }
    };
    let rows: Vec<_> = match agent {
        Some(filter) => {
            let canonical = match filter {
                "claude" => "claude_kaira",
                "codex" => "codex_kaira",
                "opencode" => "opencode_kaira",
                "agy" | "antigravity" => "antigravity_kaira",
                other => other,
            };
            rows.into_iter()
                .filter(|r| r.agent_name == canonical)
                .collect()
        }
        None => rows,
    };

    if json {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "run_id": r.run_id,
                    "agent": r.agent_name,
                    "status": r.status,
                    "started_at": r.started_at,
                    "ended_at": r.ended_at,
                    "exit_reason": r.exit_reason,
                    "summary": r.summary,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        return;
    }

    println!("\n  WRAPPER SESSIONS  (most recent first)\n");
    if rows.is_empty() {
        println!("  No sessions yet. Start an agent with: claude / codex / opencode / agy");
        println!("  (wrapper must be installed: raios agent-wrapper install)\n");
        return;
    }

    let col_w = [8usize, 16, 12, 19, 19, 12];
    println!(
        "  \x1b[90m{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}\x1b[0m",
        "RUN",
        "AGENT",
        "STATUS",
        "STARTED",
        "ENDED",
        "EXIT",
        w0 = col_w[0],
        w1 = col_w[1],
        w2 = col_w[2],
        w3 = col_w[3],
        w4 = col_w[4],
        w5 = col_w[5],
    );
    println!(
        "  {}",
        "─".repeat(col_w.iter().sum::<usize>() + col_w.len() * 2)
    );

    for r in &rows {
        let status_col = match r.status.as_str() {
            "running" => "\x1b[33mrunning \x1b[0m",
            "succeeded" => "\x1b[32msucceeded   \x1b[0m",
            "failed" => "\x1b[31mfailed  \x1b[0m",
            other => other,
        };
        println!(
            "  {:<w0$}  {:<w1$}  {}  {:<w3$}  {:<w4$}  {:<w5$}",
            &r.run_id[..8],
            r.agent_name,
            status_col,
            r.started_at,
            r.ended_at.as_deref().unwrap_or("—"),
            r.exit_reason.as_deref().unwrap_or("—"),
            w0 = col_w[0],
            w1 = col_w[1],
            w3 = col_w[3],
            w4 = col_w[4],
            w5 = col_w[5],
        );
        if let Some(summary) = &r.summary {
            println!("  \x1b[90m{:>8}  review: {}\x1b[0m", "", summary);
        }
    }
    println!();
}

pub(super) fn cmd_wrapper_note(note: &str, json: bool) -> Result<(), String> {
    let run_id = std::env::var("RAIOS_WRAPPER_RUN_ID")
        .map_err(|_| "missing RAIOS_WRAPPER_RUN_ID; run this only inside raios run".to_string())?;
    let project_path = std::env::current_dir()
        .map_err(|e| format!("cannot resolve current project: {e}"))?
        .to_string_lossy()
        .into_owned();
    #[cfg(unix)]
    if let Ok(addr) = std::env::var("RAIOS_WRAPPER_NOTE_SOCKET") {
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| format!("cannot reach wrapper note channel: {e}"))?;
        let payload = serde_json::json!({"run_id": run_id, "note": note}).to_string();
        stream
            .write_all(payload.as_bytes())
            .map_err(|e| format!("cannot send wrapper note: {e}"))?;
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .map_err(|e| format!("cannot read wrapper note response: {e}"))?;
        let response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|_| "invalid wrapper note response".to_string())?;
        if response["recorded"] != true {
            return Err(response["error"]
                .as_str()
                .unwrap_or("wrapper note rejected")
                .to_string());
        }
        if json {
            println!("{}", response);
        } else {
            println!("✓ Wrapper note recorded for run {}", &run_id[..8]);
        }
        return Ok(());
    }
    let conn = raios_core::db::open_db().map_err(|e| format!("DB error: {e}"))?;
    let event = raios_core::db::cp_record_wrapper_memory_note(&conn, &run_id, &project_path, note)
        .map_err(|e| e.to_string())?;

    raios_runtime::session_memory::sync_wrapper_session_note(
        &event.agent_name,
        &project_path,
        &run_id,
        note,
    );
    if json {
        println!(
            "{}",
            serde_json::json!({"recorded": true, "event_id": event.event_id, "run_id": run_id})
        );
    } else {
        println!("✓ Wrapper note recorded for run {}", &run_id[..8]);
    }
    Ok(())
}
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::TcpStream;
