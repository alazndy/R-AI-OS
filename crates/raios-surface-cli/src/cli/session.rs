pub(super) fn cmd_sessions(
    agent: Option<&str>,
    top: usize,
    canvas: Option<&str>,
    active: bool,
    project: Option<&str>,
    search: Option<&str>,
    json: bool,
) {
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
    let canonical = agent.map(|filter| match filter {
        "claude" => "claude_kaira",
        "codex" => "codex_kaira",
        "opencode" => "opencode_kaira",
        "agy" | "antigravity" => "antigravity_kaira",
        other => other,
    });
    let project_path = project
        .and_then(|p| std::path::Path::new(p).canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned());
    let rows = match raios_core::db::cp_sessions_list_filtered(
        &conn,
        top,
        canonical,
        active,
        project_path.as_deref().or(project),
        search,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Query error: {e}");
            std::process::exit(1);
        }
    };

    if json {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "run_id": r.run_id,
                    "native_session_id":r.native_session_id,
                    "workspace_root":r.workspace_root,
                    "task_id": r.task_id,
                    "name": r.name,
                    "work": r.work,
                    "task_status": r.task_status,
                    "project": r.project_name,
                    "project_path": r.project_path,
                    "directory": r.workspace_root,
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
        println!("  No sessions match these filters.\n");
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
            r.run_id.chars().take(8).collect::<String>(),
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
        println!("           name: {}", r.name);
        println!(
            "           project: {}  task: {}",
            r.project_name.as_deref().unwrap_or("unregistered"),
            r.task_status
        );
        println!(
            "           directory: {}",
            r.workspace_root
                .as_deref()
                .or(r.project_path.as_deref())
                .unwrap_or("unknown")
        );
        println!("           started: {}", r.started_at);
        println!("           work: {}", r.work);
        println!("           id: {}", r.run_id);
        if let Some(id) = &r.native_session_id {
            println!("           saved conversation: {id}");
        }
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
        let payload = format!("{}\n", serde_json::json!({"run_id": run_id, "note": note}));
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

pub(super) fn cmd_session_label(
    run_id: &str,
    name: Option<&str>,
    work: Option<&str>,
    json: bool,
) -> Result<(), String> {
    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    raios_core::db::cp_label_wrapper_session(&conn, run_id, name, work)
        .map_err(|e| e.to_string())?;
    if json {
        println!("{}", serde_json::json!({"updated":true,"run_id":run_id}));
    } else {
        println!("Session labels updated: {run_id}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cmd_resume(
    id: Option<&str>,
    list: bool,
    agent: Option<&str>,
    project: Option<&str>,
    search: Option<&str>,
    top: usize,
    dry_run: bool,
    json: bool,
) -> Result<(), String> {
    use raios_runtime::session_recovery::{catalog, plan, RecoveryRoots};
    use std::io::{IsTerminal, Write};
    let roots = RecoveryRoots::live().map_err(|e| e.to_string())?;
    let mut inventory = catalog(&roots, &raios_core::db::db_path()).map_err(|e| e.to_string())?;
    let project_path = project
        .and_then(|p| std::path::Path::new(p).canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned());
    let project = project_path.as_deref().or(project);
    inventory.sessions.retain(|r| {
        agent.map(|a| r.agent == a).unwrap_or(true)
            && project.map(|p| r.project_path == p).unwrap_or(true)
            && search
                .map(|s| {
                    let query = s.to_lowercase();
                    r.name.to_lowercase().contains(&query)
                        || r.key.contains(s)
                        || r.work
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query)
                })
                .unwrap_or(true)
    });
    if list || (id.is_none() && json) {
        inventory.sessions.truncate(top);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&inventory).map_err(|e| e.to_string())?
            );
        } else {
            print_recovery_inventory(&inventory);
        }
        return Ok(());
    }
    let selected = if let Some(id) = id {
        id.to_string()
    } else {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Err("use raios resume --list or provide a full saved session key in non-interactive mode".into());
        }
        inventory.sessions.truncate(top);
        print_recovery_inventory(&inventory);
        if inventory.sessions.is_empty() {
            return Err("no saved conversations match these filters".into());
        }
        print!("Select a session number (Enter cancels): ");
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?;
        if input.trim().is_empty() {
            return Ok(());
        }
        let n = input
            .trim()
            .parse::<usize>()
            .map_err(|_| "invalid selection".to_string())?;
        inventory
            .sessions
            .get(n.checked_sub(1).ok_or("invalid selection")?)
            .ok_or("invalid selection")?
            .key
            .clone()
    };
    let selected_plan = plan(&inventory, &selected).map_err(|e| e.to_string())?;
    // Rescan after selection: the conversation may have disappeared or become active.
    let fresh = catalog(&roots, &raios_core::db::db_path()).map_err(|e| e.to_string())?;
    let plan = plan(&fresh, &selected).map_err(|e| e.to_string())?;
    if plan.project_path != selected_plan.project_path
        || plan.agent != selected_plan.agent
        || plan.native_session_id != selected_plan.native_session_id
    {
        return Err("saved session changed during selection; refresh the inventory".into());
    }
    if dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if json {
        return Err("use --dry-run with JSON for a non-launching continuation plan".into());
    }
    // Only process-witnessed dead runs can be reconciled here; legacy rows are excluded.
    raios_runtime::wrapper_liveness::reconcile_sessions(&raios_core::db::db_path(), true, false)
        .map_err(|e| e.to_string())?;
    raios_runtime::agent_runner::run_agent_with_session_labels(
        &plan.agent,
        Some(plan.project_path),
        None,
        plan.args,
        raios_runtime::agent_runner::HandoffAction::Defer,
        Some(plan.name),
        plan.work,
    )
}
fn print_recovery_inventory(inventory: &raios_runtime::session_recovery::RecoveryCatalog) {
    use std::io::IsTerminal;
    let color = std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb");
    let (bold, dim, cyan, green, yellow, reset) = if color {
        (
            "\x1b[1m", "\x1b[90m", "\x1b[36m", "\x1b[32m", "\x1b[33m", "\x1b[0m",
        )
    } else {
        ("", "", "", "", "", "")
    };
    let available = inventory
        .sessions
        .iter()
        .filter(|row| row.resumable)
        .count();
    println!("\n{bold}SAVED CONVERSATIONS{reset}  {dim}all projects{reset}");
    println!(
        "{dim}{} shown · {available} available · times are local{reset}\n",
        inventory.sessions.len()
    );
    if inventory.sessions.is_empty() {
        println!("No saved conversations match these filters.\n");
    }
    for (n, row) in inventory.sessions.iter().enumerate() {
        let (status_color, status) = if row.resumable {
            (green, "available")
        } else {
            (
                yellow,
                match row.availability.as_str() {
                    "project_missing" => "project directory missing",
                    "running_or_unverified" => "running or awaiting verification",
                    "native_conversation_unavailable" => "saved conversation unavailable",
                    other => other,
                },
            )
        };
        println!("{cyan}{:>2}.{reset} {bold}{}{reset}", n + 1, row.name);
        println!("    {} · {status_color}{status}{reset}", row.agent);
        let project = std::path::Path::new(&row.project_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        let timestamp = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S %:z")
                    .to_string()
            })
            .unwrap_or_else(|_| {
                if row.updated_at.is_empty() {
                    "unknown".into()
                } else {
                    row.updated_at.clone()
                }
            });
        println!("    {dim}Project{reset}  {project}");
        println!("    {dim}Directory{reset} {}", row.project_path);
        println!("    {dim}Time{reset}      {timestamp}");
        if let Some(work) = &row.work {
            println!("    {dim}Work{reset}      {work}");
        }
        println!("    {dim}ID        {}{reset}\n", row.key);
    }
    for warning in &inventory.warnings {
        eprintln!("Inventory note: {warning}");
    }
}
