pub(super) fn cmd_agent_stats(agent: Option<String>, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };

    let stats = match &agent {
        Some(a) => match crate::db::cp_agent_stats_for(&conn, a) {
            Ok(Some(s)) => vec![s],
            Ok(None) => vec![],
            Err(e) => {
                eprintln!("Failed to read agent stats: {e}");
                return;
            }
        },
        None => match crate::db::cp_agent_stats(&conn) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read agent stats: {e}");
                return;
            }
        },
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&stats).unwrap_or_default());
        return;
    }

    if stats.is_empty() {
        match &agent {
            Some(a) => println!("  No runs recorded for '{a}' yet."),
            None => println!("  No agent runs recorded yet."),
        }
        return;
    }

    println!();
    for s in &stats {
        let rate = s.success_rate.map(|r| format!("{:.0}%", r * 100.0)).unwrap_or_else(|| "n/a".into());
        let dur = s
            .avg_duration_secs
            .map(|d| format!("{:.0}s avg", d))
            .unwrap_or_else(|| "n/a".into());
        let open_note = if s.still_open > 0 { format!("  ({} still open)", s.still_open) } else { String::new() };
        println!(
            "  {:<20} runs={:<4} success={:<5} {dur}{open_note}",
            s.agent_name, s.total_runs, rate
        );
        if !s.top_exit_reasons.is_empty() {
            let breakdown: Vec<String> =
                s.top_exit_reasons.iter().map(|(r, c)| format!("{r}={c}")).collect();
            println!("      exit_reasons: {}", breakdown.join(", "));
        }
    }
    println!();
    println!("  \x1b[90mnote: token usage and repetition are not tracked yet — this covers run count, success rate, duration, and exit_reason only.\x1b[0m");
    println!();
}
