use serde_json::{json, Value};

use super::McpServer;

fn extract_validation_errors_from_state_sync(
    state_sync: &Value,
    project_filter: Option<&str>,
) -> Result<Vec<Value>, String> {
    if state_sync["event"] != "StateSync" {
        return Err("daemon protocol mismatch: expected StateSync event".into());
    }

    let errors = state_sync
        .get("latest_errors")
        .and_then(Value::as_array)
        .ok_or_else(|| "daemon protocol mismatch: StateSync missing latest_errors".to_string())?;

    let mut errors = errors.clone();
    if let Some(filter) = project_filter {
        errors.retain(|e| {
            e["file"]
                .as_str()
                .is_some_and(|f| f.to_lowercase().contains(filter))
        });
    }

    Ok(errors)
}

impl McpServer {
    pub(super) fn tool_ask_architect(&self, args: &Value) -> Result<Value, String> {
        let question = args["question"].as_str().ok_or("missing question")?;
        let mut cortex = raios_runtime::cortex::Cortex::init().map_err(|e| e.to_string())?;
        let _ = cortex.index_file(&self.config.master_md_path);
        let memory_files = raios_runtime::filebrowser::discover_memory_files(&self.config.dev_ops_path, 10);
        for mem in memory_files {
            let _ = cortex.index_file(&mem.path);
        }
        let hits = cortex.search(question, 5).map_err(|e| e.to_string())?;
        let results: Vec<Value> = hits
            .iter()
            .map(|r| json!({ "source": r.path, "rule_or_decision": r.text, "score": r.score }))
            .collect();
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("Architectural Guidance for '{}':\n\n{}", question, serde_json::to_string_pretty(&results).unwrap_or_default()) }] }),
        )
    }

    pub(super) fn tool_get_validation_errors(&self, args: &Value) -> Result<Value, String> {
        use std::io::{Read, Write};
        let project_filter = args["project"].as_str().map(|s| s.to_lowercase());
        let mut stream = std::net::TcpStream::connect("127.0.0.1:42069")
            .map_err(|e| format!("Could not connect to daemon: {}", e))?;
        let token_path = raios_core::config::Config::config_file()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
            .join(".session_token");
        if let Ok(token) = std::fs::read_to_string(token_path) {
            let _ = stream.write_all(format!("AUTH {}\n", token.trim()).as_bytes());
        }
        let _ = stream.write_all(b"{\"command\":\"GetState\"}\n");
        let mut buffer = [0; 32768];
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        let response = String::from_utf8_lossy(&buffer[..n]);
        for line in response.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                if v["event"] == "StateSync" {
                    let errors =
                        extract_validation_errors_from_state_sync(&v, project_filter.as_deref())?;
                    return Ok(
                        json!({ "content": [{ "type": "text", "text": if errors.is_empty() { "No validation errors found.".into() } else { serde_json::to_string_pretty(&errors).unwrap_or_default() } }] }),
                    );
                }
            }
        }
        Err("Failed to retrieve validation errors from daemon".into())
    }

    pub(super) fn tool_update_state(&self, args: &Value) -> Result<Value, String> {
        let agent = args["agent"].as_str().unwrap_or("unknown");
        let action = args["action"].as_str().unwrap_or("");
        let summary = args["summary"].as_str().unwrap_or("");
        let mem_path = raios_runtime::filebrowser::discover_memory_files(&self.config.dev_ops_path, 1)
            .into_iter()
            .next()
            .map(|e| e.path)
            .unwrap_or_else(|| self.config.dev_ops_path.join("memory.md"));
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let entry = format!(
            "\n<!-- MCP update by {} at {} -->\n- [{}] **{}**: {}\n",
            agent, now, now, action, summary
        );
        let existing = std::fs::read_to_string(&mem_path).unwrap_or_default();
        raios_core::safe_io::safe_write(&mem_path, &format!("{}{}", existing, entry))
            .map_err(|e| e.to_string())?;
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("Memory updated: {} — {}", action, summary) }] }),
        )
    }

    pub(super) fn tool_handover(&self, args: &Value) -> Result<Value, String> {
        let target = args["target"].as_str().unwrap_or("unknown");
        let instruction = args["instruction"].as_str().unwrap_or("");
        let context = args["context"].as_str().unwrap_or("(no context)");
        let project_path = self.config.dev_ops_path.to_str().unwrap_or(".");
        let msg = format!("{}\n\nContext: {}", instruction, context);

        let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
        let ids = raios_core::db::create_handoff_workflow(
            &conn,
            project_path,
            "claude_kaira",
            target,
            "SUCCESS",
            &msg,
            None,
        )
        .map_err(|e| e.to_string())?;

        Ok(json!({ "content": [{ "type": "text", "text": format!(
            "Handoff registered in control plane → {}.\nTask: {}\nInstruction: {}\nContext: {}",
            target, ids.task_id, instruction, context
        ) }] }))
    }

    pub(super) fn tool_get_inbox(&self) -> Result<Value, String> {
        let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
        let tasks = raios_core::db::cp_query_active_tasks(&conn).unwrap_or_default();
        let approvals = raios_core::db::cp_query_pending_approvals_scored(&conn).unwrap_or_default();
        let runs = raios_core::db::cp_query_active_runs(&conn).unwrap_or_default();
        let blocked = raios_core::db::cp_query_blocked_tasks(&conn).unwrap_or_default();
        let summary = format!(
            "Inbox: {} active task(s), {} pending approval(s), {} active run(s), {} blocked task(s)",
            tasks.len(),
            approvals.len(),
            runs.len(),
            blocked.len(),
        );
        Ok(json!({ "content": [{ "type": "text", "text": format!(
            "{}\n\n{}",
            summary,
            serde_json::to_string_pretty(&serde_json::json!({
                "active_tasks": tasks,
                "pending_approvals": approvals,
                "active_runs": runs,
                "blocked_tasks": blocked,
            })).unwrap_or_default()
        ) }] }))
    }

    pub(super) fn tool_get_agent_stats(&self, args: &Value) -> Result<Value, String> {
        let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
        let agent = args["agent"].as_str();
        let stats = match agent {
            Some(a) => raios_core::db::cp_agent_stats_for(&conn, a)
                .map_err(|e| e.to_string())?
                .into_iter()
                .collect::<Vec<_>>(),
            None => raios_core::db::cp_agent_stats(&conn).map_err(|e| e.to_string())?,
        };
        Ok(json!({ "content": [{ "type": "text", "text":
            serde_json::to_string_pretty(&stats).unwrap_or_default()
        }] }))
    }

    pub(super) fn tool_add_task(&self, args: &Value) -> Result<Value, String> {
        let text = args["text"].as_str().ok_or("missing text")?;
        let agent = args["agent"].as_str();
        let project = args["project"].as_str();
        let fake_line = match (agent, project) {
            (Some(a), Some(p)) => format!("- [ ] {} @{} #{}", text, a, p),
            (Some(a), None) => format!("- [ ] {} @{}", text, a),
            (None, Some(p)) => format!("- [ ] {} #{}", text, p),
            (None, None) => format!("- [ ] {}", text),
        };
        if let Some(task) = raios_runtime::tasks::parse_task_line(&fake_line) {
            if let Ok(mut tasks) = raios_runtime::tasks::load_tasks(&self.config.dev_ops_path) {
                tasks.push(task);
                let _ = raios_runtime::tasks::save_tasks(&self.config.dev_ops_path, &tasks);
            }
        }
        Ok(json!({ "content": [{ "type": "text", "text": format!("Task added: {}", text) }] }))
    }

    pub(super) fn tool_get_health(&self, args: &Value) -> Result<Value, String> {
        let filter = args["project"].as_str().map(str::to_lowercase);
        let projects = raios_core::entities::load_entities(&self.config.dev_ops_path);
        let reports: Vec<_> = projects.iter()
            .filter(|p| filter.as_ref().is_none_or(|f| p.name.to_lowercase().contains(f.as_str())))
            .map(|p| {
                let h = raios_runtime::health::check_project(p);
                json!({ "name": h.name, "status": h.status, "git_dirty": h.git_dirty, "compliance_grade": h.compliance_grade, "compliance_score": h.compliance_score, "has_memory": h.has_memory, "remote_url": h.remote_url, "graphify_done": h.graphify_done, "constitution_issues": h.constitution_issues })
            }).collect();
        let summary = format!("{} project(s) checked", reports.len());
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("{}\n\n{}", summary, serde_json::to_string_pretty(&reports).unwrap_or_default()) }] }),
        )
    }

    pub(super) fn tool_list_projects(&self, args: &Value) -> Result<Value, String> {
        let filter = args["filter"].as_str().map(str::to_lowercase);
        let status_filter = args["status"].as_str().map(str::to_lowercase);
        let projects = raios_core::entities::load_entities(&self.config.dev_ops_path);
        let list: Vec<_> = projects.iter()
            .filter(|p| {
                let name_ok = filter.as_ref().is_none_or(|f| p.name.to_lowercase().contains(f.as_str()) || p.category.to_lowercase().contains(f.as_str()));
                let status_ok = status_filter.as_ref().is_none_or(|s| p.status.to_lowercase().contains(s.as_str()));
                name_ok && status_ok
            })
            .map(|p| json!({ "name": p.name, "category": p.category, "status": p.status, "github": p.github, "local_path": p.local_path.display().to_string(), "stars": p.stars, "last_commit": p.last_commit }))
            .collect();
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("{} project(s) found\n\n{}", list.len(), serde_json::to_string_pretty(&list).unwrap_or_default()) }] }),
        )
    }

    pub(super) fn tool_get_stats(&self) -> Result<Value, String> {
        use std::collections::HashMap;
        let projects = raios_core::entities::load_entities(&self.config.dev_ops_path);
        let total = projects.len();
        let mut active = 0usize;
        let mut archived = 0usize;
        let mut dirty = 0usize;
        let mut no_memory = 0usize;
        let mut local_only = 0usize;
        let mut grade_counts: HashMap<&str, usize> = HashMap::new();
        for p in &projects {
            match p.status.as_str() {
                "archived" | "legacy" => archived += 1,
                _ => active += 1,
            }
            if p.github.is_none() {
                local_only += 1;
            }
            if !p.local_path.join("memory.md").exists() {
                no_memory += 1;
            }
            if raios_runtime::filebrowser::git_is_dirty(&p.local_path) == Some(true) {
                dirty += 1;
            }
            let h = raios_runtime::health::check_project(p);
            let grade: &'static str = match h.compliance_grade.as_str() {
                "A" => "A",
                "B" => "B",
                "C" => "C",
                _ => "D",
            };
            *grade_counts.entry(grade).or_insert(0) += 1;
        }
        let stats = json!({ "total": total, "active": active, "archived": archived, "dirty": dirty, "no_memory": no_memory, "local_only": local_only, "grades": { "A": grade_counts.get("A").copied().unwrap_or(0), "B": grade_counts.get("B").copied().unwrap_or(0), "C": grade_counts.get("C").copied().unwrap_or(0), "D": grade_counts.get("D").copied().unwrap_or(0) } });
        Ok(
            json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&stats).unwrap_or_default() }] }),
        )
    }

    /// Resolve the `path` arg to a search scope: an explicit project name/absolute
    /// path if given, otherwise the current working directory (the project the
    /// MCP server itself was launched from).
    fn resolve_search_scope(&self, args: &Value) -> Result<std::path::PathBuf, String> {
        let Some(project) = args["path"].as_str() else {
            return std::env::current_dir().map_err(|e| e.to_string());
        };
        let direct = std::path::Path::new(project);
        if direct.exists() {
            return Ok(direct.to_path_buf());
        }
        if let Ok(conn) = raios_core::db::open_db() {
            if let Ok(projects) = raios_core::db::load_all_projects(&conn) {
                if let Some(found) = projects
                    .iter()
                    .find(|p| p.name.to_lowercase().contains(&project.to_lowercase()))
                {
                    return Ok(std::path::PathBuf::from(&found.path));
                }
            }
        }
        Err(format!("Project not found: {}", project))
    }

    pub(super) fn tool_semantic_search(&self, args: &Value) -> Result<Value, String> {
        let query = args["query"].as_str().ok_or("missing query")?;
        let top_k = args["top_k"].as_u64().unwrap_or(8).min(20) as usize;
        let scope = self.resolve_search_scope(args)?;
        let mut cortex = raios_runtime::cortex::Cortex::init().map_err(|e| e.to_string())?;
        let _ = cortex.index_project(&scope).unwrap_or(0);
        let vector_hits = cortex
            .search_scoped(query, top_k, &scope)
            .map_err(|e| format!("Search failed: {e}"))?;
        let bm25_hits = raios_runtime::search::indexer::ProjectIndex::build(&scope)
            .map_err(|e| format!("BM25 index build failed: {e}"))?
            .search(query);
        let fused = raios_runtime::search::hybrid::fuse(bm25_hits, vector_hits, top_k);
        let results: Vec<Value> = fused.iter().map(|r| json!({ "path": r.path.to_string_lossy(), "project": r.project, "snippet": r.snippet, "line": r.start_line, "rrf_score": format!("{:.4}", r.rrf_score), "source": r.source.label() })).collect();
        let summary = format!(
            "Semantic search for '{}' -> {} result(s) (index: {} chunks, {} files)",
            query,
            results.len(),
            cortex.chunk_count(),
            cortex.file_count()
        );
        Ok(
            json!({ "content": [{ "type": "text", "text": format!("{}\n\n{}", summary, serde_json::to_string_pretty(&results).unwrap_or_default()) }] }),
        )
    }

    pub(super) fn tool_project_info(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let git = raios_core::core::git::status(&path);
        let health_cached = self.get_health_from_db(&path);
        let ver = raios_core::core::version::info(&path);
        let env = raios_core::core::env::check(&path);
        let disk = raios_core::core::disk::analyze(&path);
        let has_lockfile = path.join("Cargo.lock").exists()
            || path.join("package-lock.json").exists()
            || path.join("pnpm-lock.yaml").exists()
            || path.join("go.sum").exists();
        let project_type = raios_core::core::build::detect_type(&path);
        let has_memory = path.join("memory.md").exists();
        let has_sigmap = path.join("SIGMAP.md").exists();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let summary = format!("Project: {} ({})\nGit: {} {}\nHealth: compliance={} security={} refactor={}\nVersion: {}\nEnv: {} ({} missing)\nDisk: {} total, {} cache\nType: {}  lockfile: {}  memory: {}  sigmap: {}",
            name, path.display(),
            git.branch.as_deref().unwrap_or("?"), if git.dirty { "dirty" } else { "clean" },
            health_cached["compliance_grade"].as_str().unwrap_or("-"), health_cached["security_grade"].as_str().unwrap_or("-"), health_cached["refactor_grade"].as_str().unwrap_or("-"),
            ver.as_ref().map(|v| v.current.as_str()).unwrap_or("-"),
            if env.ok { "OK" } else { "issues" }, env.missing_keys.len(),
            raios_core::core::disk::human_size(disk.total_bytes), raios_core::core::disk::human_size(disk.cache_bytes),
            project_type.label(), has_lockfile, has_memory, has_sigmap);
        Ok(json!({
            "content": [{ "type": "text", "text": summary }],
            "name": name, "path": path.display().to_string(), "project_type": project_type.label(),
            "git": { "branch": git.branch, "dirty": git.dirty, "staged_count": git.staged.len(), "unstaged_count": git.unstaged.len(), "untracked_count": git.untracked.len(), "ahead": git.ahead, "behind": git.behind, "remote": git.remote },
            "health": health_cached,
            "version": ver.as_ref().map(|v| json!({ "current": v.current, "last_tag": v.last_tag, "commits_since_tag": v.commits_since_tag })),
            "env": { "ok": env.ok, "has_env": env.has_env, "has_example": env.has_example, "missing_keys": env.missing_keys, "empty_keys": env.empty_keys, "total_keys": env.total_env_keys },
            "disk": { "total_mb": disk.total_mb(), "source_mb": disk.source_mb(), "cache_mb": disk.cache_mb(), "file_count": disk.file_count, "cache_dirs": disk.cache_dirs.iter().map(|c| json!({ "kind": c.kind, "mb": c.mb() })).collect::<Vec<_>>() },
            "has_lockfile": has_lockfile, "has_memory": has_memory, "has_sigmap": has_sigmap
        }))
    }

    pub(super) fn get_health_from_db(&self, path: &std::path::Path) -> Value {
        let path_str = path.to_string_lossy().to_string();
        if let Ok(conn) = raios_core::db::open_db() {
            let result = conn.query_row(
                "SELECT h.compliance_grade, h.compliance_score, h.security_grade, h.security_score, h.security_issues, h.security_critical, h.git_dirty, h.has_memory, h.has_sigmap, h.refactor_grade, h.refactor_score, h.refactor_high, h.scanned_at FROM health_cache h JOIN projects p ON p.id = h.project_id WHERE p.path = ?1",
                rusqlite::params![path_str],
                |row| Ok(json!({
                    "compliance_grade": row.get::<_, String>(0).unwrap_or_default(),
                    "compliance_score": row.get::<_, Option<i64>>(1).unwrap_or_default(),
                    "security_grade": row.get::<_, Option<String>>(2).unwrap_or_default(),
                    "security_score": row.get::<_, Option<i64>>(3).unwrap_or_default(),
                    "security_issues": row.get::<_, i64>(4).unwrap_or_default(),
                    "security_critical": row.get::<_, i64>(5).unwrap_or_default(),
                    "git_dirty": row.get::<_, i64>(6).unwrap_or_default() != 0,
                    "has_memory": row.get::<_, i64>(7).unwrap_or_default() != 0,
                    "has_sigmap": row.get::<_, i64>(8).unwrap_or_default() != 0,
                    "refactor_grade": row.get::<_, String>(9).unwrap_or_else(|_| "-".into()),
                    "refactor_score": row.get::<_, Option<i64>>(10).unwrap_or_default(),
                    "refactor_high": row.get::<_, i64>(11).unwrap_or_default(),
                    "scanned_at": row.get::<_, String>(12).unwrap_or_default()
                })),
            );
            if let Ok(v) = result {
                return v;
            }
        }
        json!({ "compliance_grade": "-", "security_grade": null, "refactor_grade": "-" })
    }

    pub(super) fn tool_portfolio_status(&self, args: &Value) -> Result<Value, String> {
        let name_filter = args["filter"].as_str().map(str::to_lowercase);
        let status_filter = args["status"].as_str();
        let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
        let projects = raios_core::db::load_all_projects(&conn).map_err(|e| e.to_string())?;
        let filtered: Vec<_> = projects
            .iter()
            .filter(|p| {
                if let Some(ref f) = name_filter {
                    if !p.name.to_lowercase().contains(f.as_str()) {
                        return false;
                    }
                }
                if let Some(s) = status_filter {
                    if p.status != s {
                        return false;
                    }
                }
                true
            })
            .collect();
        let mut rows = Vec::new();
        let mut text_lines = vec![format!(
            "{:<30} {:<10} {:<8} {:<6} {:<6} {:<8}",
            "PROJECT", "STATUS", "GIT", "COMP", "SEC", "REFACTOR"
        )];
        text_lines.push("─".repeat(72));
        for p in &filtered {
            let health = self.get_health_from_db(std::path::Path::new(&p.path));
            let dirty = if health["git_dirty"].as_bool().unwrap_or(false) {
                "dirty"
            } else {
                "clean"
            };
            let comp = health["compliance_grade"].as_str().unwrap_or("-");
            let sec = health["security_grade"].as_str().unwrap_or("-");
            let rf = health["refactor_grade"].as_str().unwrap_or("-");
            text_lines.push(format!(
                "{:<30} {:<10} {:<8} {:<6} {:<6} {:<8}",
                &p.name[..p.name.len().min(29)],
                p.status,
                dirty,
                comp,
                sec,
                rf
            ));
            rows.push(json!({ "name": p.name, "status": p.status, "category": p.category, "github": p.github, "git_dirty": health["git_dirty"], "compliance_grade": comp, "security_grade": sec, "refactor_grade": rf, "has_memory": health["has_memory"], "has_sigmap": health["has_sigmap"] }));
        }
        text_lines.push(format!("\n{} projects", filtered.len()));
        Ok(
            json!({ "content": [{ "type": "text", "text": text_lines.join("\n") }], "total": filtered.len(), "projects": rows }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::extract_validation_errors_from_state_sync;
    use serde_json::json;

    #[test]
    fn extract_validation_errors_requires_latest_errors_field() {
        let payload = json!({
            "event": "StateSync",
            "projects": []
        });

        let err = extract_validation_errors_from_state_sync(&payload, None).unwrap_err();
        assert!(err.contains("missing latest_errors"));
    }

    #[test]
    fn extract_validation_errors_filters_by_project_hint() {
        let payload = json!({
            "event": "StateSync",
            "latest_errors": [
                { "file": "/workspace/raios/src/main.rs", "message": "a" },
                { "file": "/workspace/other/lib.rs", "message": "b" }
            ]
        });

        let errors = extract_validation_errors_from_state_sync(&payload, Some("raios")).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["file"], "/workspace/raios/src/main.rs");
    }
}
