use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn handle_tools_list(&self) -> Result<Value, String> {
        Ok(json!({ "tools": [
            { "name": "update_state",    "description": "Update the shared memory.md with agent progress. Call this after completing any significant action.", "inputSchema": { "type": "object", "properties": { "agent": {"type":"string","description":"Agent name (claude, gemini, antigravity)"}, "action": {"type":"string","description":"What was done"}, "summary": {"type":"string","description":"Detailed summary to append to memory"} }, "required": ["agent","action","summary"] } },
            { "name": "handover",        "description": "Hand off the current task to another agent. Use when you cannot continue or another agent is better suited.", "inputSchema": { "type": "object", "properties": { "target": {"type":"string","enum":["claude","gemini","antigravity"],"description":"Target agent name"}, "instruction": {"type":"string","description":"Specific instruction for the target agent"}, "context": {"type":"string","description":"Summary of what has been done so far"} }, "required": ["target","instruction"] } },
            { "name": "add_task",        "description": "Add a new task to tasks.md", "inputSchema": { "type": "object", "properties": { "text": {"type":"string","description":"Task description"}, "agent": {"type":"string","description":"Assigned agent (optional)"}, "project": {"type":"string","description":"Project name (optional)"} }, "required": ["text"] } },
            { "name": "get_health",      "description": "Get health report for one or all projects (git status, compliance grade, memory.md presence).", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name filter (leave empty for all)"} } } },
            { "name": "list_projects",   "description": "List all known projects from entities.json with their status and category.", "inputSchema": { "type": "object", "properties": { "filter": {"type":"string","description":"Name/category filter (optional)"}, "status": {"type":"string","description":"Status filter: active | archived (optional)"} } } },
            { "name": "get_stats",       "description": "Get portfolio-wide statistics: total projects, grade distribution, dirty count, local-only count.", "inputSchema": { "type": "object", "properties": {} } },
            { "name": "semantic_search", "description": "Semantic (intent-aware) search across the entire Dev Ops workspace. Finds relevant code, docs, and notes by meaning, not just keywords.", "inputSchema": { "type": "object", "properties": { "query": {"type":"string","description":"Natural language search query"}, "top_k": {"type":"integer","description":"Number of results to return (default 8, max 20)"} }, "required": ["query"] } },
            { "name": "project_info",    "description": "Get a complete snapshot of a project in one call: git status, health grades, version, deps, env, disk usage, build type. Use this instead of calling individual tools one by one.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "portfolio_status","description": "Lightweight status overview of all known projects: name, status, git dirty, health grades, version. Use for getting the big picture before drilling into a specific project.", "inputSchema": { "type": "object", "properties": { "filter": {"type":"string","description":"Filter by project name (optional)"}, "status": {"type":"string","description":"Filter by status: active | archived (optional)"} } } },
            { "name": "disk_usage",      "description": "Analyze disk usage of a project: total size, source files, cache dirs and largest files.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "list_ports",      "description": "List all listening TCP ports on this machine with their PID and process name.", "inputSchema": { "type": "object", "properties": {} } },
            { "name": "version_info",    "description": "Get current version, last git tag, and commits since last tag for a project.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "version_bump",    "description": "Bump project semver (patch/minor/major), optionally update CHANGELOG.md and create git tag.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"}, "level": {"type":"string","description":"patch | minor | major"}, "changelog": {"type":"boolean","description":"Update CHANGELOG.md (default false)"}, "tag": {"type":"boolean","description":"Create git tag (default false)"} }, "required": ["project","level"] } },
            { "name": "env_status",      "description": "Check .env file health: missing keys vs .env.example, empty values, undocumented keys. Never returns secret values — key names only.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "deps_status",     "description": "Check dependency health: outdated packages and CVE vulnerabilities. Auto-detects Rust/Node/Python/Go.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "run_build",       "description": "Build a project. Auto-detects Rust/Node/Python/Go. Returns ok status, warnings, errors, and diagnostics.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "run_tests",       "description": "Run tests for a project. Auto-detects cargo test / npm test / pytest / go test. Returns passed/failed counts.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "git_status",      "description": "Get git status of a project: branch, dirty files, staged/unstaged/untracked lists, ahead/behind remote.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"} }, "required": ["project"] } },
            { "name": "git_log",         "description": "Get recent commit history of a project.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"}, "count": {"type":"integer","description":"Number of commits to return (default 10)"} }, "required": ["project"] } },
            { "name": "git_diff",        "description": "Get diff summary and full diff text of a project.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"}, "staged": {"type":"boolean","description":"Show staged changes only (default false)"} }, "required": ["project"] } },
            { "name": "git_commit",      "description": "Stage all changes and commit in a project. Optionally push after commit.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name or absolute path"}, "message": {"type":"string","description":"Commit message"}, "push": {"type":"boolean","description":"Push after committing (default false)"} }, "required": ["project","message"] } },
            { "name": "ask_architect",   "description": "Consult the Architectural Memory. Searches MASTER.md rules and memory.md decision logs.", "inputSchema": { "type": "object", "properties": { "question": {"type":"string","description":"The architectural question"} }, "required": ["question"] } },
            { "name": "get_validation_errors", "description": "Get latest compilation or compliance errors for a project. Useful for self-healing after a code change.", "inputSchema": { "type": "object", "properties": { "project": {"type":"string","description":"Project name (optional)"} } } },
            { "name": "session_note",    "description": "Write a structured note to the current session memory.", "inputSchema": { "type": "object", "required": ["note"], "properties": { "note": {"type":"string","description":"The note to record (max 500 chars)"}, "session_id": {"type":"string","description":"Session ID (omit to use current open session)"} } } },
            { "name": "create_swarm_task",    "description": "Create an isolated swarm task in a new git worktree for parallel agent development.", "inputSchema": { "type": "object", "required": ["project_name","project_path","description"], "properties": { "project_name": {"type":"string"}, "project_path": {"type":"string","description":"Absolute path to the project"}, "description": {"type":"string","description":"What the agent should do in this worktree"}, "agent": {"type":"string","description":"Agent name (default: claude)"} } } },
            { "name": "list_swarm_tasks",     "description": "List all active swarm tasks (excludes merged/rejected).", "inputSchema": { "type": "object", "properties": {} } },
            { "name": "approve_swarm_task",   "description": "Approve and merge a completed swarm task into the main branch.", "inputSchema": { "type": "object", "required": ["task_id"], "properties": { "task_id": {"type":"string"} } } },
            { "name": "route_capability",          "description": "Semantically route a natural language query to the best matching raios capability name.", "inputSchema": { "type": "object", "required": ["query"], "properties": { "query": {"type":"string","description":"Natural language description of what you want to do"} } } },
            { "name": "list_evolution_candidates", "description": "List pending instinct candidates learned from agent job outcomes.", "inputSchema": { "type": "object", "properties": { "limit": {"type":"integer","description":"Max results (default: 20)"} } } },
            { "name": "promote_evolution_candidate","description": "Promote a learned instinct candidate to active memory and the instinct store.", "inputSchema": { "type": "object", "required": ["rule"], "properties": { "rule": {"type":"string","description":"The rule text to promote"} } } }
        ]}))
    }

    pub(super) fn handle_tools_call(&mut self, params: &Value) -> Result<Value, String> {
        if self.pin_broken {
            return Err(
                "tool_pin: manifest tampered — all tool calls blocked. \
                 Run `raios pin-reset` after verifying the binary."
                    .to_string(),
            );
        }

        let name = params["name"].as_str().ok_or("missing tool name")?;
        let args = &params["arguments"];

        if let Err(e) = self.rate_limiter.check(name) {
            return Err(e.to_string());
        }

        if self.quarantine.is_enabled() {
            let args_str = serde_json::to_string(args).unwrap_or_default();
            if let Ok(conn) = crate::db::open_db() {
                if let Err(e) = self.quarantine.check(&conn, name, &args_str) {
                    return Err(e.to_string());
                }
            }
        }

        match name {
            "update_state"    => self.tool_update_state(args),
            "handover"        => self.tool_handover(args),
            "add_task"        => self.tool_add_task(args),
            "get_health"      => self.tool_get_health(args),
            "list_projects"   => self.tool_list_projects(args),
            "get_stats"       => self.tool_get_stats(),
            "semantic_search" => self.tool_semantic_search(args),
            "ask_architect"   => self.tool_ask_architect(args),
            "get_validation_errors" => self.tool_get_validation_errors(args),
            "project_info"    => self.tool_project_info(args),
            "portfolio_status"=> self.tool_portfolio_status(args),
            "disk_usage"      => self.tool_disk_usage(args),
            "list_ports"      => self.tool_list_ports(),
            "version_info"    => self.tool_version_info(args),
            "version_bump"    => self.tool_version_bump(args),
            "env_status"      => self.tool_env_status(args),
            "deps_status"     => self.tool_deps_status(args),
            "run_build"       => self.tool_run_build(args),
            "run_tests"       => self.tool_run_tests(args),
            "git_status"      => self.tool_git_status(args),
            "git_log"         => self.tool_git_log(args),
            "git_diff"        => self.tool_git_diff(args),
            "git_commit"      => self.tool_git_commit(args),
            "session_note"    => self.tool_session_note(args),
            "create_swarm_task"         => self.tool_create_swarm_task(args),
            "list_swarm_tasks"          => self.tool_list_swarm_tasks(),
            "approve_swarm_task"        => self.tool_approve_swarm_task(args),
            "route_capability"          => self.tool_route_capability(args),
            "list_evolution_candidates" => self.tool_list_evolution_candidates(args),
            "promote_evolution_candidate" => self.tool_promote_evolution_candidate(args),
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }

    pub(super) fn resolve_git_path(&self, args: &Value) -> Result<std::path::PathBuf, String> {
        let project = args["project"].as_str().ok_or("missing project")?;
        let direct = std::path::Path::new(project);
        if direct.exists() { return Ok(direct.to_path_buf()); }
        if let Ok(conn) = crate::db::open_db() {
            if let Ok(projects) = crate::db::load_all_projects(&conn) {
                if let Some(found) = projects.iter()
                    .find(|p| p.name.to_lowercase().contains(&project.to_lowercase()))
                {
                    return Ok(std::path::PathBuf::from(&found.path));
                }
            }
        }
        Err(format!("Project not found: {}", project))
    }
}
