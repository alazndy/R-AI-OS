use raios_surface_cli::cli::CronAction;
use raios_core::db;
use std::process;

fn parse_interval(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.len() < 2 {
        return Err(format!("invalid interval '{s}' — examples: 30s 5m 6h 1d"));
    }
    let (num_str, unit) = s.split_at(s.len() - 1);
    let n: i64 = num_str.parse().map_err(|_| {
        format!("'{num_str}' is not a valid number in interval '{s}'")
    })?;
    if n <= 0 {
        return Err(format!("interval must be > 0, got {n}"));
    }
    match unit {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86400),
        _ => Err(format!("unknown unit '{unit}' — use s (seconds), m (minutes), h (hours), d (days)")),
    }
}

pub fn cmd_cron(action: CronAction, json: bool) {
    let conn = match db::open_db() {
        Ok(c) => c,
        Err(e) => {
            if json {
                println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
            } else {
                eprintln!("Database open failed: {e}");
            }
            process::exit(1);
        }
    };

    match action {
        CronAction::Add { title, every, agent, task } => {
            let interval = match parse_interval(&every) {
                Ok(i) => i,
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e}));
                    } else {
                        eprintln!("Error: {e}");
                    }
                    process::exit(1);
                }
            };

            match db::cp_scheduled_job_create(&conn, &title, &agent, &task, interval) {
                Ok(id) => {
                    let jobs = db::cp_scheduled_jobs_list(&conn).unwrap_or_default();
                    let job = jobs.iter().find(|j| j.id == id);
                    let next_run = job.map(|j| j.next_run_at.as_str()).unwrap_or("unknown");

                    if json {
                        println!("{}", serde_json::json!({
                            "status": "ok",
                            "id": id,
                            "title": title,
                            "agent": agent,
                            "interval_secs": interval,
                            "next_run_at": next_run
                        }));
                    } else {
                        println!("✓ Scheduled job created successfully!");
                        println!("  ID:       {}", id);
                        println!("  Title:    {}", title);
                        println!("  Agent:    {}", agent);
                        println!("  Interval: {}s (every {})", interval, every);
                        println!("  Next Run: {}", next_run);
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
                    } else {
                        eprintln!("Failed to create scheduled job: {e}");
                    }
                    process::exit(1);
                }
            }
        }
        CronAction::List => {
            match db::cp_scheduled_jobs_list(&conn) {
                Ok(jobs) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&jobs).unwrap_or_default());
                    } else {
                        if jobs.is_empty() {
                            println!("No scheduled jobs found.");
                            return;
                        }
                        println!(
                            "{:<8} | {:<28} | {:<10} | {:<8} | {:<20} | {:<8} | {:<5}",
                            "ID", "TITLE", "AGENT", "INTERVAL", "NEXT RUN", "STATUS", "RUNS"
                        );
                        println!("{}", "─".repeat(99));
                        for job in jobs {
                            let truncated_title = if job.title.len() > 28 {
                                format!("{}...", &job.title[..25])
                            } else {
                                job.title.clone()
                            };
                            let interval_str = format!("{}s", job.interval_secs);
                            println!(
                                "{:<8} | {:<28} | {:<10} | {:<8} | {:<20} | {:<8} | {:<5}",
                                &job.id[..8.min(job.id.len())],
                                truncated_title,
                                job.agent,
                                interval_str,
                                job.next_run_at,
                                job.status,
                                job.run_count
                            );
                        }
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
                    } else {
                        eprintln!("Failed to list scheduled jobs: {e}");
                    }
                    process::exit(1);
                }
            }
        }
        CronAction::Remove { id } => {
            let jobs = db::cp_scheduled_jobs_list(&conn).unwrap_or_default();
            let matched_job = jobs.iter().find(|j| j.id == id || j.id.starts_with(&id));

            let target_id = match matched_job {
                Some(job) => job.id.clone(),
                None => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": format!("Job '{id}' not found")}));
                    } else {
                        eprintln!("Error: Job '{id}' not found.");
                    }
                    process::exit(1);
                }
            };

            match db::cp_scheduled_job_delete(&conn, &target_id) {
                Ok(_) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "ok", "id": target_id}));
                    } else {
                        println!("✓ Scheduled job '{target_id}' marked as deleted.");
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
                    } else {
                        eprintln!("Failed to delete job: {e}");
                    }
                    process::exit(1);
                }
            }
        }
        CronAction::Pause { id } => {
            let jobs = db::cp_scheduled_jobs_list(&conn).unwrap_or_default();
            let matched_job = jobs.iter().find(|j| j.id == id || j.id.starts_with(&id));

            let target_id = match matched_job {
                Some(job) => job.id.clone(),
                None => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": format!("Job '{id}' not found")}));
                    } else {
                        eprintln!("Error: Job '{id}' not found.");
                    }
                    process::exit(1);
                }
            };

            match db::cp_scheduled_job_set_status(&conn, &target_id, "paused") {
                Ok(_) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "ok", "id": target_id, "status_value": "paused"}));
                    } else {
                        println!("✓ Scheduled job '{target_id}' paused.");
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
                    } else {
                        eprintln!("Failed to pause job: {e}");
                    }
                    process::exit(1);
                }
            }
        }
        CronAction::Resume { id } => {
            let jobs = db::cp_scheduled_jobs_list(&conn).unwrap_or_default();
            let matched_job = jobs.iter().find(|j| j.id == id || j.id.starts_with(&id));

            let target_id = match matched_job {
                Some(job) => job.id.clone(),
                None => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": format!("Job '{id}' not found")}));
                    } else {
                        eprintln!("Error: Job '{id}' not found.");
                    }
                    process::exit(1);
                }
            };

            match db::cp_scheduled_job_set_status(&conn, &target_id, "active") {
                Ok(_) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "ok", "id": target_id, "status_value": "active"}));
                    } else {
                        println!("✓ Scheduled job '{target_id}' resumed.");
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e.to_string()}));
                    } else {
                        eprintln!("Failed to resume job: {e}");
                    }
                    process::exit(1);
                }
            }
        }
        CronAction::Run { id } => {
            let jobs = db::cp_scheduled_jobs_list(&conn).unwrap_or_default();
            let matched_job = jobs.iter().find(|j| j.id == id || j.id.starts_with(&id));

            let job = match matched_job {
                Some(j) => j,
                None => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": format!("Job '{id}' not found")}));
                    } else {
                        eprintln!("Error: Job '{id}' not found.");
                    }
                    process::exit(1);
                }
            };

            // 1. Mark to run now in DB (updates next_run_at to now)
            if let Err(e) = db::cp_scheduled_job_trigger_now(&conn, &job.id) {
                eprintln!("Warning: failed to update next_run_at in DB: {e}");
            }

            // 2. Synchronous spawn
            let prompt = format!(
                "[SCHEDULED TASK]\nTitle: {}\n\n{}",
                job.title, job.task_description
            );

            println!("Firing job '{}' ({}) synchronously...", job.title, job.id);
            match raios_runtime::agent_runner::spawn_agent_detached(&job.agent, &prompt, None) {
                Ok(pid) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "ok", "pid": pid}));
                    } else {
                        println!("✓ Job fired successfully. Spawned process PID: {}", pid);
                    }
                }
                Err(e) => {
                    if json {
                        println!("{}", serde_json::json!({"status": "error", "message": e}));
                    } else {
                        eprintln!("Error spawning agent: {e}");
                    }
                    process::exit(1);
                }
            }
        }
    }
}
