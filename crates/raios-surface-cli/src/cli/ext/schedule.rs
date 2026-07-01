use std::path::{Path, PathBuf};
use super::ExtSchedule;

fn cron_to_interval_secs(cron: &str) -> u64 {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return 86400;
    }
    let dow = fields[4];
    let hour = fields[1];
    let minute = fields[0];

    if dow != "*" {
        return 604800;
    }
    if hour != "*" {
        return 86400;
    }
    if minute != "*" {
        return 3600;
    }
    60
}

fn cron_next_run(cron: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let fields: Vec<&str> = cron.split_whitespace().collect();
    let hour: u64 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let minute: u64 = fields.first().and_then(|s| s.parse().ok()).unwrap_or(0);

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let day_secs = now_secs % 86400;
    let target_day_secs = hour * 3600 + minute * 60;

    let next_secs = if target_day_secs > day_secs {
        now_secs - day_secs + target_day_secs
    } else {
        now_secs - day_secs + 86400 + target_day_secs
    };

    chrono_next_run_str(next_secs)
}

fn chrono_next_run_str(unix_secs: u64) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(unix_secs as i64, 0)
        .unwrap_or_else(chrono::Utc::now);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn register_extension_schedules(
    ext_name: &str,
    _ext_path: &Path,
    schedules: &[ExtSchedule],
) {
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
        .join("workspace.db");

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("    Could not open raios DB to register schedules: {}", e);
            return;
        }
    };

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    for sched in schedules {
        let id = format!("ext-{}-{}", ext_name, sched.name);
        let interval = cron_to_interval_secs(&sched.cron);
        let next_run = cron_next_run(&sched.cron);
        let task_desc = format!("raios ext {} {}", ext_name, sched.command);
        let title = if sched.description.is_empty() {
            format!("[ext:{}] {}", ext_name, sched.command)
        } else {
            sched.description.clone()
        };

        let result = conn.execute(
            "INSERT INTO cp_scheduled_jobs
             (id, title, agent, task_description, interval_secs, status, next_run_at, created_at)
             VALUES (?1,?2,?3,?4,?5,'active',?6,?7)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title,
               task_description=excluded.task_description,
               interval_secs=excluded.interval_secs,
               next_run_at=excluded.next_run_at",
            rusqlite::params![id, title, ext_name, task_desc, interval as i64, next_run, now],
        );
        match result {
            Ok(_) => println!("    ✓ Schedule registered: {} ({}s)", sched.name, interval),
            Err(e) => eprintln!("    ✗ Failed to register schedule {}: {}", sched.name, e),
        }
    }
}
