use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn tool_create_swarm_task(&self, args: &Value) -> Result<Value, String> {
        let project_name = args["project_name"]
            .as_str()
            .ok_or("missing project_name")?;
        let project_path = args["project_path"]
            .as_str()
            .ok_or("missing project_path")?;
        let description = args["description"].as_str().ok_or("missing description")?;
        let agent = args["agent"].as_str().unwrap_or("claude");
        let store =
            raios_runtime::swarm::store::SwarmStore::new(raios_runtime::swarm::store::SwarmStore::default_path());
        match store.create(
            project_name,
            std::path::Path::new(project_path),
            description,
            agent,
        ) {
            Ok(task) => Ok(
                json!({ "content": [{ "type": "text", "text": format!("Swarm task created: {}\nbranch: {}", task.id, task.branch_name) }], "task_id": task.id.to_string(), "branch": task.branch_name }),
            ),
            Err(e) => Err(format!("Failed to create swarm task: {e}")),
        }
    }

    pub(super) fn tool_list_swarm_tasks(&self) -> Result<Value, String> {
        let store =
            raios_runtime::swarm::store::SwarmStore::new(raios_runtime::swarm::store::SwarmStore::default_path());
        let tasks = store.list_active();
        let text = if tasks.is_empty() {
            "No active swarm tasks.".to_string()
        } else {
            tasks
                .iter()
                .map(|t| {
                    format!(
                        "{} [{}] {} — {}",
                        t.id,
                        format!("{:?}", t.status).to_lowercase(),
                        t.agent,
                        t.task_description
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "tasks": tasks }))
    }

    pub(super) fn tool_approve_swarm_task(&self, args: &Value) -> Result<Value, String> {
        let task_id = args["task_id"].as_str().ok_or("missing task_id")?;
        let store =
            raios_runtime::swarm::store::SwarmStore::new(raios_runtime::swarm::store::SwarmStore::default_path());
        match store.get(task_id) {
            Some(task) => {
                let msg = format!("swarm merge: {}", task.task_description);
                match raios_runtime::swarm::merge::merge_branch(&task.project_path, &task.branch_name, &msg)
                {
                    Ok(_) => {
                        let _ = raios_runtime::swarm::worktree::remove_worktree(
                            &task.project_path,
                            &task.worktree_path,
                        );
                        store.set_status(task_id, raios_runtime::swarm::SwarmStatus::Merged);
                        Ok(
                            json!({ "content": [{ "type": "text", "text": format!("✓ Merged: {task_id}") }], "merged": true }),
                        )
                    }
                    Err(e) => Err(format!("Merge failed: {e}")),
                }
            }
            None => Err(format!("Task not found: {task_id}")),
        }
    }

    pub(super) fn tool_route_capability(&self, args: &Value) -> Result<Value, String> {
        let query = args["query"].as_str().ok_or("missing query")?;
        let descriptions = vec![
            (
                "health_check".to_string(),
                "Run health scan and grade on a project".to_string(),
            ),
            (
                "search_memory".to_string(),
                "Semantic search across all project memory files".to_string(),
            ),
            (
                "run_sentinel".to_string(),
                "Compile and lint check a project".to_string(),
            ),
            (
                "list_projects".to_string(),
                "List all known projects in workspace".to_string(),
            ),
            (
                "git_status".to_string(),
                "Show git status for a project".to_string(),
            ),
            (
                "git_commit".to_string(),
                "Create a git commit with a message".to_string(),
            ),
            ("git_push".to_string(), "Push commits to remote".to_string()),
            (
                "run_build".to_string(),
                "Run the project build command".to_string(),
            ),
            (
                "run_tests".to_string(),
                "Run the project test suite".to_string(),
            ),
            (
                "check_deps".to_string(),
                "Check for outdated dependencies and CVEs".to_string(),
            ),
            (
                "bump_version".to_string(),
                "Bump semver version and update CHANGELOG".to_string(),
            ),
            (
                "create_swarm_task".to_string(),
                "Start a parallel agent task in an isolated git worktree".to_string(),
            ),
            (
                "list_swarm_tasks".to_string(),
                "List all active swarm tasks".to_string(),
            ),
            (
                "create_task_graph".to_string(),
                "Submit a DAG of dependent tasks for execution".to_string(),
            ),
            (
                "list_evolution_candidates".to_string(),
                "List pending instinct candidates from job outcomes".to_string(),
            ),
        ];
        let router = raios_runtime::intelligence::edge::EdgeRouter::new(descriptions);
        let capability = router.route(query);
        let text = match capability {
            Some(c) => format!("→ {c}"),
            None => format!("No matching capability for: {query}"),
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "capability": capability }))
    }

    pub(super) fn tool_list_evolution_candidates(&self, args: &Value) -> Result<Value, String> {
        let limit = args["limit"].as_u64().unwrap_or(20) as usize;
        let store = raios_runtime::intelligence::evolution::CandidateStore::new(
            raios_runtime::intelligence::evolution::CandidateStore::default_path(),
        );
        let candidates = store.list_pending(limit);
        let text = if candidates.is_empty() {
            "No pending instinct candidates.".to_string()
        } else {
            candidates
                .iter()
                .enumerate()
                .map(|(i, r)| format!("{}. {}", i + 1, r))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "candidates": candidates }))
    }

    pub(super) fn tool_promote_evolution_candidate(&self, args: &Value) -> Result<Value, String> {
        let rule = args["rule"].as_str().ok_or("missing rule")?;
        let store = raios_runtime::intelligence::evolution::CandidateStore::new(
            raios_runtime::intelligence::evolution::CandidateStore::default_path(),
        );
        store.promote(rule);
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = raios_runtime::intelligence::instinct::append_to_memory_md(&cwd, rule);
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("✓ Promoted: {rule}") }], "promoted": rule }),
        )
    }
}
