use super::*;
pub(super) fn cmd_task_update(id: &str, status: &str, json: bool) {
    let valid = ["pending", "in_progress", "completed", "cancelled"];
    if !valid.contains(&status) {
        if json {
            eprintln!("{{\"status\":\"error\",\"message\":\"invalid status: {status}\"}}");
        } else {
            eprintln!("Invalid status '{status}'. Valid: {}", valid.join(", "));
        }
        std::process::exit(1);
    }
    match raios_core::db::open_db() {
        Ok(conn) => {
            let now = chrono::Local::now().to_rfc3339();
            let res = conn.execute(
                "UPDATE cp_tasks SET status=?1, updated_at=?2 WHERE id=?3",
                rusqlite::params![status, now, id],
            );
            match res {
                Ok(rows) if rows > 0 => {
                    if json {
                        println!(
                            "{{\"status\":\"ok\",\"id\":\"{id}\",\"new_status\":\"{status}\"}}"
                        );
                    } else {
                        println!("Task {id} → {status}");
                    }
                }
                Ok(_) => {
                    if json {
                        eprintln!("{{\"status\":\"error\",\"message\":\"task not found: {id}\"}}");
                    } else {
                        eprintln!("Task not found: {id}");
                    }
                    std::process::exit(1);
                }
                Err(e) => {
                    if json {
                        eprintln!("{{\"status\":\"error\",\"message\":\"{e}\"}}");
                    } else {
                        eprintln!("DB error: {e}");
                    }
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to open DB: {e}");
            std::process::exit(1);
        }
    }
}

pub fn run_refactor_flag(json: bool) {
    let dev_ops_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    refactor::cmd_refactor(
        None, // Target is None to check the current directory
        &dev_ops_path,
        json,
        500,  // high_lines
        300,  // medium_lines
        10,   // high_unwrap
        5,    // medium_unwrap
        10,   // high_nesting
        8,    // medium_nesting
        None, // ext_config
    );
}
