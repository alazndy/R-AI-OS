use crate::filebrowser::find_file_by_name;
use crate::sync::sync_universe;
use anyhow::Result;
use chrono::Local;
use std::io::Write;
use std::path::Path;
use std::thread;

use crate::app::{state::*, App};

impl App {
    pub(crate) fn execute_command(&mut self, raw: &str) -> Result<()> {
        let raw = raw.trim();
        if !raw.starts_with('/') {
            return Ok(());
        }
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "/sync" | "/setup" => {
                self.system.is_syncing = true;
                self.system.sync_status = None;
                self.add_activity("System", "Starting Universal Sync...", "Info");
                let tx = self.tx.clone();
                let dev = self.config.dev_ops_path.clone();
                let mst = self.config.master_md_path.clone();
                thread::spawn(move || match sync_universe(&dev, &mst) {
                    Ok(msg) => tx.send(BgMsg::SyncDone(msg)).ok(),
                    Err(e) => tx.send(BgMsg::SyncError(e.to_string())).ok(),
                });
            }
            "/discover" => {
                let projects = crate::entities::discover_entities(&self.config.dev_ops_path);
                self.projects.list = projects.clone();
                match crate::entities::save_entities(&self.config.dev_ops_path, projects) {
                    Ok(_) => {
                        self.system.sync_status = Some("Discovery complete: entities.json updated".into())
                    }
                    Err(e) => self.system.sync_status = Some(format!("Discovery error: {}", e)),
                }
                self.add_activity("System", "Full Dev Ops discovery complete", "Info");
            }
            "/q" | "/quit" | "/exit" => self.should_quit = true,
            "/rules" => {
                self.ui.menu_cursor = 1;
                self.ui.right_panel_focus = true;
                self.ui.right_file_cursor = 0;
            }
            "/memory" => {
                self.ui.menu_cursor = 5;
                self.ui.right_panel_focus = true;
                self.ui.right_file_cursor = 2;
            }
            "/mempalace" | "/palace" | "/mp" => {
                if self.mempalace.rooms.is_empty() && !self.mempalace.is_building {
                    self.mempalace.is_building = true;
                    let tx = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&dev_ops)))
                            .ok();
                    });
                }
                self.state = AppState::MemPalaceView;
                self.mempalace.filter.clear();
            }
            "/view" if !arg.is_empty() => {
                if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                    self.open_file_view(entry);
                }
            }
            "/edit" if !arg.is_empty() => {
                if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                    if !entry.read_only {
                        self.open_file_edit(entry);
                    }
                }
            }
            "/search" | "/s" => {
                self.ui.menu_cursor = 6;
                self.ui.right_panel_focus = false;
                if !arg.is_empty() {
                    self.add_activity("User", &format!("Searching for: {}", arg), "Info");
                    if let Some(ref idx) = self.search.index {
                        self.search.results = idx.search(arg);
                        self.search.cursor = 0;
                        if !self.search.results.is_empty() {
                            self.ui.right_panel_focus = true;
                        }
                    } else {
                        self.search.status = Some("Index not ready — try again shortly".into());
                    }
                }
            }
            "/memo" | "/note" if !arg.is_empty() => {
                let result = append_memo(arg, &self.config.dev_ops_path);
                self.system.sync_status = Some(result);
            }
            "/scan-system" | "/audit" => {
                self.system.is_scanning = true;
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let report = crate::system_scan::scan_system();
                    tx.send(BgMsg::AiAuditReport(report)).ok();
                });
            }
            "/open" | "/project" => {
                if !arg.is_empty() {
                    let q = arg.to_lowercase();
                    if let Some(proj) = self
                        .projects.list
                        .iter()
                        .find(|p| p.name.to_lowercase().contains(&q))
                        .cloned()
                    {
                        self.open_project_detail(proj);
                    } else {
                        self.system.sync_status = Some(format!("Project not found: {}", arg));
                    }
                } else {
                    self.ui.menu_cursor = 7;
                    self.ui.right_panel_focus = false;
                }
            }
            "/timeline" | "/history" => {
                self.ui.menu_cursor = 8;
                self.ui.right_panel_focus = false;
            }
            "/logs" | "/log" => {
                self.ui.menu_cursor = 9;
                self.ui.right_panel_focus = false;
            }
            "/help" | "/?" => {
                self.state = AppState::HelpView;
            }
            "/task" => {
                // /task add <text> [@agent] [#project]
                // /task send claude|gemini|antigravity
                if arg.starts_with("add ") {
                    let rest = arg.trim_start_matches("add ").trim();
                    // Parse the line using the same parser (add checkbox prefix)
                    let fake_line = format!("- [ ] {}", rest);
                    // Re-use load logic: parse inline
                    let new_task = crate::tasks::parse_task_line(&fake_line).unwrap_or_else(|| {
                        crate::tasks::Task {
                            text: rest.to_string(),
                            completed: false,
                            agent: None,
                            project: None,
                        }
                    });
                    let agent_hint = new_task.agent.as_deref().unwrap_or("-");
                    let proj_hint = new_task.project.as_deref().unwrap_or("-");
                    self.system.sync_status = Some(format!("Task added [{}→{}]", agent_hint, proj_hint));
                    self.tasks.list.push(new_task);
                    let _ = crate::tasks::save_tasks(&self.config.dev_ops_path, &self.tasks.list);
                } else if let Some(agent) = arg.strip_prefix("send ") {
                    self.dispatch_task(agent.trim());
                } else if arg == "load" {
                    if let Ok(tasks) = crate::tasks::load_tasks(&self.config.dev_ops_path) {
                        self.tasks.list = tasks;
                        self.system.sync_status = Some(format!("{} tasks loaded", self.tasks.list.len()));
                    }
                }
            }
            "/vault-create" => {
                let name = arg.trim();
                if name.is_empty() {
                    self.system.sync_status = Some("Usage: /vault-create <project_name>".into());
                } else {
                    let proj = self.projects.list.iter().find(|p| p.name == name).cloned();
                    if let Some(p) = proj {
                        let vault_file = self
                            .config
                            .vault_projects_path
                            .join(format!("{}.md", p.name));
                        if vault_file.exists() {
                            self.system.sync_status = Some("Vault note already exists".into());
                        } else {
                            let content = format!(
                                "---\ncategory: {}\nstatus: {}\ntags: [project, raios]\ncreated: {}\n---\n# {}\n\n## Overview\n{} is a project managed by R-AI-OS.\n\n## Details\n- Path: {}\n",
                                p.category, p.status, chrono::Local::now().format("%Y-%m-%d"), p.name, p.name, p.local_path.display()
                            );
                            if std::fs::write(&vault_file, content).is_ok() {
                                self.system.vault_projects.push(p.name.clone());
                                self.system.sync_status = Some(format!("Vault note created: {}", p.name));
                                self.add_activity(
                                    "System",
                                    &format!("Created vault note for {}", p.name),
                                    "Info",
                                );
                            } else {
                                self.system.sync_status = Some("Failed to write vault note".into());
                            }
                        }
                    } else {
                        self.system.sync_status = Some(format!("Project not found: {}", name));
                    }
                }
            }
            "/health" => {
                if !self.projects.list.is_empty() {
                    self.health.is_checking = true;
                    self.state = AppState::HealthView;
                    if let Some(ref tx_daemon) = self.tx_daemon {
                        let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    }
                } else {
                    self.system.sync_status = Some("Load entities.json first".into());
                }
            }
            "/reindex" => {
                self.add_activity("System", "Requesting search re-index", "Info");
                if self.config.dev_ops_path.exists() && !self.search.is_indexing {
                    self.search.is_indexing = true;
                    self.search.status = Some("Rebuilding index...".into());
                    let tx = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        match crate::indexer::ProjectIndex::build(&dev_ops) {
                            Ok(idx) => tx.send(BgMsg::IndexReady(idx)).ok(),
                            Err(e) => tx.send(BgMsg::IndexError(e.to_string())).ok(),
                        };
                    });
                }
            }
            "/graphify" | "/graph" => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let msg = self.run_graphify(&proj.local_path);
                    self.system.sync_status = Some(msg);
                } else {
                    self.system.sync_status = Some("Open a project detail first to run graphify".into());
                }
            }
            "/heal" => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let mut sentinel_errors = Vec::new();
                    let path_str = proj.local_path.to_string_lossy().to_string();
                    for file in &self.system.sentinel_files {
                        if file.path.contains(&path_str)
                            && file.state == crate::sentinel::SentinelState::Failed
                        {
                            for err in &file.errors {
                                sentinel_errors.push(format!(
                                    "{}:{}: {}",
                                    err.file,
                                    err.line.unwrap_or(0),
                                    err.message
                                ));
                            }
                        }
                    }

                    if sentinel_errors.is_empty() {
                        self.system.sync_status =
                            Some("No sentinel errors detected in current project.".into());
                    } else {
                        // Trigger correction task
                        let task_text = format!("FIX SENTINEL ERRORS: {}", sentinel_errors[0]);
                        let task = crate::tasks::Task {
                            text: task_text,
                            completed: false,
                            agent: Some("claude".into()), // Default to claude for healing
                            project: Some(proj.name.clone()),
                        };
                        let result = crate::tasks::dispatch_to_agent(
                            &task,
                            "claude",
                            Some(&proj.local_path),
                            Some(sentinel_errors),
                        );
                        self.system.sync_status = Some(result);
                        self.add_activity("Sentinel", "Self-Correction cycle triggered", "Warning");
                    }
                } else {
                    self.system.sync_status = Some("Open a project detail first to run /heal".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn append_memo(text: &str, dev_ops: &Path) -> String {
    use std::fs::OpenOptions;
    let ts = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let entry = format!("- [{}] {}\n", ts, text);
    let notes_path = dev_ops.join("_session_notes.md");
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes_path)
    {
        Ok(mut f) => {
            let _ = f.write_all(entry.as_bytes());
            "Memo saved → _session_notes.md".to_string()
        }
        Err(e) => format!("Memo error: {}", e),
    }
}
