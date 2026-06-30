pub(super) fn cmd_sessions(agent: Option<&str>, top: usize, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };
    let rows = match crate::db::cp_sessions_list(&conn, top) {
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
    println!("  {}", "─".repeat(col_w.iter().sum::<usize>() + col_w.len() * 2));

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
    }
    println!();
}

