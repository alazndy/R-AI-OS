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

#[cfg(test)]
mod tests {
    use super::cmd_task_update;
    use std::sync::Mutex;

    // `RAIOS_DB_PATH` is process-global; serialize any test in this binary
    // that reads or writes it so parallel `cargo test` threads never race.
    static DB_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_db<R>(f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
        let _lock = DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("RAIOS_DB_PATH").ok();
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        std::env::set_var("RAIOS_DB_PATH", tmp_db.path());

        let conn = raios_core::db::open_db().unwrap();
        let result = f(&conn);
        drop(conn);

        match original {
            Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
            None => std::env::remove_var("RAIOS_DB_PATH"),
        }
        result
    }

    fn insert_task(conn: &rusqlite::Connection, id: &str, status: &str) {
        conn.execute(
            "INSERT INTO cp_tasks (id, title, description, status, created_at, updated_at)
             VALUES (?1, 'Title', 'Description', ?2, datetime('now'), datetime('now'))",
            rusqlite::params![id, status],
        )
        .unwrap();
    }

    #[test]
    fn cmd_task_update_persists_the_new_status_for_an_existing_task() {
        with_temp_db(|conn| {
            insert_task(conn, "task-1", "pending");

            cmd_task_update("task-1", "in_progress", false);

            let status: String = conn
                .query_row("SELECT status FROM cp_tasks WHERE id = 'task-1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(status, "in_progress");
        });
    }

    #[test]
    fn cmd_task_update_leaves_other_tasks_untouched() {
        with_temp_db(|conn| {
            insert_task(conn, "task-1", "pending");
            insert_task(conn, "task-2", "pending");

            cmd_task_update("task-1", "completed", false);

            let other: String = conn
                .query_row("SELECT status FROM cp_tasks WHERE id = 'task-2'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(other, "pending");
        });
    }
}
