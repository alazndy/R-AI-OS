use super::{EvolveAction, SwarmAction};

pub(super) fn cmd_swarm(action: SwarmAction, json: bool) {
    use raios_runtime::swarm::store::SwarmStore;
    use raios_runtime::swarm::SwarmStatus;

    let store = SwarmStore::new(SwarmStore::default_path());

    match action {
        SwarmAction::Start {
            project,
            path,
            description,
            agent,
        } => match store.create(&project, std::path::Path::new(&path), &description, &agent) {
            Ok(task) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"status":"ok","task_id":task.id.to_string()})
                    );
                } else {
                    println!("✓ Swarm task created");
                    println!("  id:     {}", task.id);
                    println!("  branch: {}", task.branch_name);
                    println!("  agent:  {}", task.agent);
                }
            }
            Err(e) => eprintln!("✗ Failed to create swarm task: {e}"),
        },
        SwarmAction::List => {
            let tasks = store.list_active();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&tasks).unwrap_or_default()
                );
                return;
            }
            if tasks.is_empty() {
                println!("No active swarm tasks.");
                return;
            }
            println!(
                "{:<36}  {:<12}  {:<8}  DESCRIPTION",
                "ID", "STATUS", "AGENT"
            );
            println!("{}", "-".repeat(80_usize));
            for t in &tasks {
                let status = match &t.status {
                    SwarmStatus::Initializing => "init",
                    SwarmStatus::Running => "running",
                    SwarmStatus::AwaitingReview => "review",
                    SwarmStatus::Merged => "merged",
                    SwarmStatus::Rejected => "rejected",
                    SwarmStatus::Failed(_) => "failed",
                };
                println!(
                    "{:<36}  {:<12}  {:<8}  {}",
                    t.id, status, t.agent, t.task_description
                );
            }
        }
        SwarmAction::Approve { task_id } => match store.get(&task_id) {
            Some(task) => {
                let msg = format!("swarm merge: {}", task.task_description);
                match raios_runtime::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg)
                {
                    Ok(_) => {
                        let _ = raios_runtime::swarm::worktree::remove_worktree(
                            &task.project_path,
                            &task.worktree_path,
                        );
                        store.set_status(&task_id, SwarmStatus::Merged);
                        if json {
                            println!("{}", serde_json::json!({"status":"ok","merged":task_id}));
                        } else {
                            println!("✓ Swarm task merged: {task_id}");
                        }
                    }
                    Err(e) => eprintln!("✗ Merge failed: {e}"),
                }
            }
            None => eprintln!("✗ Task not found: {task_id}"),
        },
        SwarmAction::Reject { task_id } => match store.get(&task_id) {
            Some(task) => {
                let _ = raios_runtime::swarm::worktree::remove_worktree(
                    &task.project_path,
                    &task.worktree_path,
                );
                let _ = raios_runtime::swarm::merge::delete_branch(&task.project_path, &task.branch_name);
                store.set_status(&task_id, SwarmStatus::Rejected);
                if json {
                    println!("{}", serde_json::json!({"status":"ok","rejected":task_id}));
                } else {
                    println!("✓ Swarm task rejected: {task_id}");
                }
            }
            None => eprintln!("✗ Task not found: {task_id}"),
        },
    }
}

pub(super) fn cmd_route(query: &str, json: bool) {
    match raios_runtime::intelligence::router::route_capability(query) {
        Some(capability) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"capability": capability, "query": query})
                );
            } else {
                println!("→ {capability}");
            }
        }
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"capability": null, "query": query})
                );
            } else {
                println!("No matching capability found for: {query}");
            }
        }
    }
}

pub(super) fn cmd_evolve(action: EvolveAction, json: bool) {
    use raios_runtime::evolution::CandidateStore;

    let store = CandidateStore::new(CandidateStore::default_path());
    match action {
        EvolveAction::List { limit } => {
            let candidates = store.list_pending(limit as usize);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&candidates).unwrap_or_default()
                );
            } else if candidates.is_empty() {
                println!("No pending instinct candidates.");
            } else {
                println!("Pending instinct candidates ({}):", candidates.len());
                for (i, rule) in candidates.iter().enumerate() {
                    println!("  {}. {}", i + 1, rule);
                }
            }
        }
        EvolveAction::FromTraces { project, limit } => {
            let conn = match raios_core::db::open_db() {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("Failed to open workspace DB: {e}");
                    std::process::exit(1);
                }
            };
            match raios_runtime::evolution::import_trace_candidates(
                &conn,
                &store,
                project.as_deref(),
                limit,
            ) {
                Ok(inserted) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": "ok",
                                "inserted": inserted,
                                "project": project,
                                "limit": limit
                            })
                        );
                    } else {
                        println!("✓ Generated {inserted} instinct candidate(s) from trace memory.");
                        if inserted > 0 {
                            println!("  Review with `raios evolve list`; promote with `raios evolve promote \"...\"`.");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to import trace candidates: {e}");
                    std::process::exit(1);
                }
            }
        }
        EvolveAction::Promote { rule } => {
            store.promote(&rule);
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let _ = raios_runtime::instinct::append_to_memory_md(&cwd, &rule);
            if json {
                println!("{}", serde_json::json!({"status":"ok","promoted":rule}));
            } else {
                println!("✓ Promoted: {rule}");
            }
        }
        EvolveAction::Prune => {
            let removed = store.sweep_expired();
            if json {
                println!("{}", serde_json::json!({"status":"ok","removed":removed}));
            } else {
                println!("✓ Pruned {removed} expired candidate(s).");
            }
        }
    }
}
