use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};
use chrono::Local;
use std::io::Write;
use crate::app::{Editor, filtered_palette, MENU_ITEMS};
use crate::config::Config;
use crate::requirements::check_requirements;
use crate::filebrowser::{
    discover_all_agent_rules, find_file_by_name, get_agent_config_files,
    get_master_rule_files, get_mempalace_files, get_policy_files,
    load_recent_projects
};
use crate::sync::sync_universe;

use crate::app::{App, state::*};
use crate::filebrowser::{FileEntry, load_file_content, save_file_content};
use crate::compliance;

impl App {
    pub fn open_file_view(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        self.compliance = Some(compliance::check_file(&entry.path, &content));
        self.file_lines = content.lines().map(str::to_owned).collect();
        self.file_scroll = 0;
        self.watched_file_mtime = std::fs::metadata(&entry.path).ok().and_then(|m| m.modified().ok());
        self.file_changed_externally = false;
        self.active_file = Some(entry);
        self.edit_save_msg = None;
        self.state = AppState::FileView;
    }

    pub fn open_file_edit(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        self.compliance = Some(compliance::check_file(&entry.path, &content));
        let view_h = self.height.saturating_sub(8) as usize;
        self.editor = Editor::from_content(&content, view_h.max(5));
        self.watched_file_mtime = std::fs::metadata(&entry.path).ok().and_then(|m| m.modified().ok());
        self.file_changed_externally = false;
        self.active_file = Some(entry);
        self.edit_save_msg = None;
        self.state = AppState::FileEdit;
    }

    pub fn open_graph_report(&mut self, project_path: &Path) {
        let report_path = project_path.join("GRAPH_REPORT.md");
        if report_path.exists() {
            let content = load_file_content(&report_path);
            self.graph_report_lines = content.lines().map(str::to_owned).collect();
            self.graph_report_scroll = 0;
            self.state = AppState::GraphReport;
        } else {
            self.sync_status = Some("Graph report not found. Run Graphify first.".into());
        }
    }

    pub fn open_git_diff(&mut self, project_path: &Path) {
        let output = std::process::Command::new("git")
            .current_dir(project_path)
            .args(&["diff"])
            .output();

        if let Ok(out) = output {
            let diff = String::from_utf8_lossy(&out.stdout).to_string();
            if diff.trim().is_empty() {
                self.git_diff_lines = vec!["No unstaged changes.".to_string()];
            } else {
                self.git_diff_lines = diff.lines().map(|s| s.to_string()).collect();
            }
            self.git_diff_scroll = 0;
            self.state = AppState::GitDiffView;
        } else {
            self.git_diff_lines = vec!["Failed to run git diff.".to_string()];
            self.state = AppState::GitDiffView;
        }
    }

    pub fn save_file(&mut self) {
        if let Some(ref file) = self.active_file.clone() {
            let content = self.editor.to_string();
            match save_file_content(&file.path, &content) {
                Ok(()) => {
                    self.file_lines = content.lines().map(str::to_owned).collect();
                    self.edit_save_msg = Some("Saved!".into());
                    self.state = AppState::FileView;
                }
                Err(e) => {
                    self.edit_save_msg = Some(format!("Error: {}", e));
                }
            }
        }
    }

    fn handle_health_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => { self.state = AppState::Dashboard; }
            KeyCode::Up   | KeyCode::Char('k') => { if self.health_cursor > 0 { self.health_cursor -= 1; } }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.health_cursor + 1 < self.health_report.len() {
                    self.health_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(h) = self.health_report.get(self.health_cursor).cloned() {
                    if let Some(proj) = self.projects.iter()
                        .find(|p| p.local_path == h.path)
                        .cloned()
                    {
                        self.open_project_detail(proj);
                    }
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.is_checking_health = true;
                if let Some(ref tx_daemon) = self.tx_daemon {
                    let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    self.add_activity("System", "Manual health refresh requested", "Info");
                }
            }
            _ => {}
        }
    }

    pub fn open_project_detail(&mut self, project: crate::entities::EntityProject) {
        self.project_memory_scroll = 0;
        self.project_panel_focus = false;
        self.project_memory_lines = Vec::new();
        self.project_git_log = Vec::new();
        self.active_project = Some(project.clone());
        self.state = AppState::ProjectDetail;

        let tx = self.tx.clone();
        let proj_path = project.local_path.clone();
        thread::spawn(move || {
            let memory_path = proj_path.join("memory.md");
            let content = load_file_content(&memory_path);
            let memory: Vec<String> = content.lines().map(str::to_owned).collect();
            let git_log = crate::filebrowser::get_git_log(&proj_path);
            tx.send(BgMsg::ProjectOpened { memory, git_log }).ok();
        });
    }

    fn handle_project_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Tab => {
                self.project_panel_focus = !self.project_panel_focus;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.project_memory_scroll = self.project_memory_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.project_memory_lines.len() as u16).saturating_sub(10);
                self.project_memory_scroll = (self.project_memory_scroll + 1).min(max);
            }
            KeyCode::Char('e') => {
                if let Some(ref proj) = self.active_project.clone() {
                    let p = proj.local_path.join("memory.md");
                    if p.exists() {
                        self.open_file_edit(FileEntry::new("memory.md", p));
                    }
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                if self.active_project.is_some() {
                    self.show_launcher = true;
                }
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                if let Some(ref proj) = self.active_project.clone() {
                    let msg = self.run_graphify(&proj.local_path);
                    self.sync_status = Some(msg);
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(ref proj) = self.active_project.clone() {
                    self.open_graph_report(&proj.local_path);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(ref proj) = self.active_project.clone() {
                    self.open_git_diff(&proj.local_path);
                }
            }
            _ => {}
        }
    }

    fn handle_graph_report_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.state = AppState::ProjectDetail;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.graph_report_scroll > 0 {
                    self.graph_report_scroll -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                if self.graph_report_scroll < max {
                    self.graph_report_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.graph_report_scroll = self.graph_report_scroll.saturating_sub(self.height / 2);
            }
            KeyCode::PageDown => {
                let max = (self.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                self.graph_report_scroll = (self.graph_report_scroll + self.height / 2).min(max);
            }
            _ => {}
        }
    }



    pub fn handle_bg_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::BootResult { name, pass, done } => {
                self.boot_results.push((name, pass));
                if done {
                    let tx = self.tx.clone();
                    let has_config = Config::load().is_some();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(150));
                        if has_config {
                            tx.send(BgMsg::TransitionToDashboard).ok();
                        } else {
                            tx.send(BgMsg::TransitionToSetup).ok();
                        }
                    });
                }
            }
            BgMsg::TransitionToSetup => {
                use crate::config::Config as Cfg;
                let detected = Cfg::auto_detect();

                // Pre-fill wizard fields from auto-detect
                if let Some(p) = &detected.dev_ops {
                    self.wizard_dev_ops = p.to_string_lossy().into_owned();
                }
                if let Some(p) = &detected.master_md {
                    self.wizard_master = p.to_string_lossy().into_owned();
                }
                if let Some(p) = &detected.vault_projects {
                    self.wizard_vault = p.to_string_lossy().into_owned();
                }

                // Keep legacy setup_fields for compatibility
                self.requirements = check_requirements();
                self.setup_fields = vec![
                    SetupField::new("Dev Ops Path", "Root workspace").with_detected(detected.dev_ops.clone()),
                    SetupField::new("MASTER.md Path", "Agent constitution").with_detected(detected.master_md.clone()),
                    SetupField::new("Skills Path", ".agents/skills").with_detected(detected.skills.clone()),
                    SetupField::new("Vault Projects Path", "Obsidian Vault").with_detected(detected.vault_projects.clone()),
                ];
                self.setup_cursor = 0;
                self.wizard_step = crate::setup_wizard::WizardStep::Welcome;
                self.state = AppState::Setup;

                // Detect agents in background
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let status = crate::setup_wizard::detect_agents();
                    tx.send(BgMsg::AgentStatusReady(status)).ok();
                });
            }
            BgMsg::TransitionToDashboard => {
                self.state = AppState::Dashboard;
                // Discover graphify before spawning threads (no borrow conflict here)
                self.graphify_script = crate::health::find_graphify_script(&self.config.dev_ops_path);
                let tx = self.tx.clone();
                let cfg = self.config.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects(&cfg.dev_ops_path))).ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents())).ok();
                    tx.send(BgMsg::Skills(crate::discovery::discover_skills(&cfg.skills_path))).ok();
                    tx.send(BgMsg::MemPalaceFiles(get_mempalace_files(&cfg.dev_ops_path))).ok();
                    tx.send(BgMsg::MasterFiles(get_master_rule_files(&cfg.master_md_path))).ok();
                    tx.send(BgMsg::AgentFiles(get_agent_config_files())).ok();
                    tx.send(BgMsg::PolicyFiles(get_policy_files())).ok();
                    tx.send(BgMsg::AgentRuleGroups(discover_all_agent_rules(&cfg.dev_ops_path))).ok();
                    let discovered = crate::entities::discover_entities(&cfg.dev_ops_path);
                    let count = discovered.len();
                    tx.send(BgMsg::Projects(discovered)).ok();
                    let log = LogEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        sender: "System".into(),
                        content: format!("Discovery: Found {} projects in total", count),
                    };
                    tx.send(BgMsg::NewLog(log)).ok();
                    tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&cfg.dev_ops_path))).ok();
                    if let Ok(tasks) = crate::tasks::load_tasks(&cfg.dev_ops_path) {
                        tx.send(BgMsg::Tasks(tasks)).ok();
                    }
                    
                    let vault_path = cfg.vault_projects_path.clone();
                    if vault_path.exists() {
                        let mut vault_projs = Vec::new();
                        if let Ok(entries) = std::fs::read_dir(vault_path) {
                            for entry in entries.filter_map(|e| e.ok()) {
                                if let Some(name) = entry.path().file_stem() {
                                    vault_projs.push(name.to_string_lossy().into_owned());
                                }
                            }
                        }
                        tx.send(BgMsg::VaultStatus(vault_projs)).ok();
                    }
                    
                    // Removed Port Monitor from TUI, it runs in aiosd.

                    // Compute portfolio stats in background
                    let projects = crate::entities::load_entities(&cfg.dev_ops_path);
                    let mut stats = crate::app::state::PortfolioStats::default();
                    stats.total = projects.len();
                    let mut cat_dirty: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                    for p in &projects {
                        match p.status.as_str() {
                            "archived" | "legacy" => stats.archived += 1,
                            _ => stats.active += 1,
                        }
                        if p.github.is_none() { stats.local_only += 1; }
                        if !p.local_path.join("memory.md").exists() { stats.no_memory += 1; }
                        if crate::filebrowser::git_is_dirty(&p.local_path) == Some(true) {
                            stats.dirty += 1;
                            *cat_dirty.entry(p.category.clone()).or_insert(0) += 1;
                        }
                        let health = crate::health::check_project(p);
                        match health.compliance_grade.as_str() {
                            "A" => stats.grade_a += 1,
                            "B" => stats.grade_b += 1,
                            "C" => stats.grade_c += 1,
                            _ => stats.grade_d += 1,
                        }
                    }
                    stats.top_dirty_category = cat_dirty.into_iter()
                        .max_by_key(|(_, v)| *v)
                        .map(|(k, _)| k)
                        .unwrap_or_default();
                    tx.send(BgMsg::StatsReady(stats)).ok();
                });
            }
            BgMsg::SearchResults(results) => {
                self.search_results = results;
                self.search_cursor = 0;
            }
            BgMsg::RecentProjects(p) => {
                self.recent_projects = p;
                self.memory_refresh_pending = false;
            }
            BgMsg::Agents(a) => self.agents = a,
            BgMsg::Skills(s) => self.skills = s,
            BgMsg::MasterFiles(m) => self.master_files = m,
            BgMsg::AgentFiles(a) => self.agent_files = a,
            BgMsg::PolicyFiles(p) => self.policy_files = p,
            BgMsg::MemPalaceFiles(m) => self.mempalace_files = m,
            BgMsg::SyncDone(msg) => {
                self.is_syncing = false;
                self.sync_status = Some(msg.clone());
                self.add_activity("System", &msg, "Info");
                let tx = self.tx.clone();
                let cfg = self.config.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects(&cfg.dev_ops_path))).ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents())).ok();
                    tx.send(BgMsg::MemPalaceFiles(crate::filebrowser::get_mempalace_files(&cfg.dev_ops_path))).ok();
                    tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&cfg.dev_ops_path))).ok();
                });
            }
            BgMsg::SyncError(e) => {
                self.is_syncing = false;
                self.sync_status = Some(format!("Error: {}", e));
            }
            BgMsg::AgentRuleGroups(groups) => {
                self.agent_rule_groups = groups;
                // Flatten into agent_files so the file panel still works
                self.agent_files = self
                    .agent_rule_groups
                    .iter()
                    .flat_map(|g| g.files.iter().cloned())
                    .collect();
            }
            BgMsg::Projects(p) => {
                self.projects = p;
                self.project_cursor = 0;
            }
            BgMsg::Tasks(t) => {
                self.tasks = t;
            }
            BgMsg::VaultStatus(v) => {
                self.vault_projects = v;
            }
            BgMsg::ActivePorts(ports) => {
                self.active_ports = ports;
            }
            BgMsg::HandoverApproved { target, instruction: _, count } => {
                self.add_activity("System", &format!("Handover to {} approved. Count: {}", target, count), "Info");
            }
            BgMsg::HumanApprovalRequired { target, instruction, reason } => {
                self.add_activity("System", &format!("Human Approval Required: {}", reason), "Warning");
                self.handover_modal = Some((target, instruction));
            }
            BgMsg::FileChangeRequested { approval } => {
                self.pending_file_changes.push(approval.clone());
                self.pending_change_cursor = self.pending_file_changes.len().saturating_sub(1);
                self.git_diff_lines = crate::app::editor::simple_diff(&approval.original_content, &approval.new_content);
                self.state = AppState::GitDiffView;
                self.add_activity("Agent", &format!("File Change Requested: {}", approval.path), "Warning");
            }
            BgMsg::HumanApprovalResult { status } => {
                self.add_activity("System", &format!("Human Approval Result: {}", status), "Info");
                self.handover_modal = None;
            }
            BgMsg::AiAuditReport(report) => {
                self.system_report = Some(report);
                self.is_scanning_system = false;
                self.menu_cursor = 11; // Open Diagnostics/System tab
            }
            BgMsg::HealthReport(report) => {
                self.health_report = report;
                self.is_checking_health = false;
                self.health_cursor = 0;
            }
            BgMsg::StateSync { projects, health_reports, active_agents, index_ready, handover_count, pending_file_changes } => {
                self.projects = projects;
                self.health_report = health_reports;
                self.active_agents = active_agents;
                self.handover_count = handover_count as usize;
                self.pending_file_changes = pending_file_changes;
                if index_ready {
                    self.index_status = Some("Index Ready (Synced)".into());
                }
                self.is_checking_health = false;
                self.add_activity("System", "Full state synchronized with daemon", "Info");
            }
            BgMsg::ProjectOpened { memory, git_log } => {
                self.project_memory_lines = memory;
                self.project_git_log = git_log;
            }
            BgMsg::IndexReady(idx) => {
                let doc_count = idx.doc_count;
                self.index = Some(idx);
                self.is_indexing = false;
                self.index_status = Some(format!("{} files indexed", doc_count));
            }
            BgMsg::IndexError(e) => {
                self.is_indexing = false;
                self.index_status = Some(format!("Index error: {}", e));
            }
            BgMsg::ActivityUpdate(mut acts) => {
                self.activities.append(&mut acts);
            }
            BgMsg::NewLog(log) => {
                self.logs.push(log);
            }
            BgMsg::MemPalaceBuilt(rooms) => {
                let n = rooms.len();
                // Register all memory.md files for watching
                for room in &rooms {
                    for proj in &room.projects {
                        let mem = proj.path.join("memory.md");
                        if mem.exists() {
                            let mtime = std::fs::metadata(&mem)
                                .ok()
                                .and_then(|m| m.modified().ok())
                                .unwrap_or(SystemTime::UNIX_EPOCH);
                            self.memory_watch.entry(mem).or_insert(mtime);
                        }
                    }
                }
                self.mp_expanded = vec![true; n];
                self.mp_rooms = rooms;
                self.mp_room_cursor = 0;
                self.mp_proj_cursor = None;
                self.mp_is_building = false;
            }
            BgMsg::StatsReady(stats) => {
                self.stats_cache = Some(stats);
                self.is_computing_stats = false;
            }
            BgMsg::AgentStatusReady(status) => {
                self.wizard_agent_status = Some(status);
            }
            BgMsg::WizardActions(actions) => {
                self.wizard_action_log.extend(actions);
                self.wizard_running = false;
            }
            BgMsg::WizardDone => {
                self.wizard_running = false;
                self.wizard_step = crate::setup_wizard::WizardStep::Done;
            }
            BgMsg::FileChanged(path) => {
                // Bouncing Limit takibi: _session_notes.md değiştiğinde ardışık HANDOVER kontrolü yap
                if path.file_name().and_then(|n| n.to_str()) == Some("_session_notes.md") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
                        let mut count = 0;
                        for line in lines.iter().rev() {
                            if line.contains("HANDOVER →") {
                                count += 1;
                            } else if !line.trim().starts_with("Context:") {
                                break;
                            }
                        }
                        self.handover_count = count;
                        if count >= 3 {
                            self.bouncing_alert = true;
                        } else {
                            self.bouncing_alert = false;
                        }
                    }
                }
                
                // Eğer değişen dosya aktif açık olan dosya ise otomatik reload ve compliance tetikle
                if let Some(ref active) = self.active_file {
                    if active.path == path {
                        let content = load_file_content(&path);
                        self.compliance = Some(compliance::check_file(&path, &content));
                        self.file_lines = content.lines().map(str::to_owned).collect();
                        self.watched_file_mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
                        self.file_changed_externally = false;
                    }
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state = AppState::Search;
            self.search_query.clear();
            self.search_results.clear();
            return Ok(());
        }

        // Handover modal takes absolute priority
        if self.handover_modal.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = format!("{{\"command\":\"HumanApproval\",\"approved\":true}}");
                        let _ = tx.send(msg);
                    }
                    self.handover_modal = None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = format!("{{\"command\":\"HumanApproval\",\"approved\":false}}");
                        let _ = tx.send(msg);
                    }
                    self.handover_modal = None;
                }
                _ => {}
            }
            return Ok(());
        }

        // Launcher overlay takes priority over all other input
        if self.show_launcher {
            match key.code {
                KeyCode::Esc => { self.show_launcher = false; }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some(ref proj) = self.active_project.clone() {
                        let msg = launch_agent("claude", &proj.local_path);
                        self.sync_status = Some(msg);
                    }
                    self.show_launcher = false;
                }
                KeyCode::Char('g') | KeyCode::Char('G') => {
                    if let Some(ref proj) = self.active_project.clone() {
                        let msg = launch_agent("gemini", &proj.local_path);
                        self.sync_status = Some(msg);
                    }
                    self.show_launcher = false;
                }
                _ => {}
            }
            return Ok(());
        }

        if self.state == AppState::Dashboard && self.menu_cursor == 2 && key.code == KeyCode::Char('f') {
            if let Some(ref report) = self.compliance {
                if !report.violations.is_empty() && !self.is_fixing {
                    self.is_fixing = true;
                    self.fix_status = Some("Claude fixing issues...".into());
                    self.add_activity("Agent", "Initiating Auto-Fix with Claude Code", "Warning");
                    let tx = self.tx.clone();
                    thread::spawn(move || {
                        thread::sleep(std::time::Duration::from_secs(3));
                        tx.send(BgMsg::SyncDone("Auto-Fix Complete: Issues resolved".into())).ok();
                    });
                }
            }
        }

        if self.state == AppState::Dashboard && self.menu_cursor == 2 && key.code == KeyCode::Char('f') {
            if let Some(ref report) = self.compliance {
                if !report.violations.is_empty() && !self.is_fixing {
                    self.is_fixing = true;
                    self.fix_status = Some("Claude fixing issues...".into());
                    self.add_activity("Agent", "Initiating Auto-Fix with Claude Code", "Warning");
                    let tx = self.tx.clone();
                    thread::spawn(move || {
                        thread::sleep(std::time::Duration::from_secs(3));
                        tx.send(BgMsg::SyncDone("Auto-Fix Complete: Issues resolved".into())).ok();
                    });
                }
            }
        }

        match self.state {
            AppState::Search => self.handle_key_search(key),
            AppState::Booting => {
                if key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                }
                Ok(())
            }

            AppState::Setup => self.handle_setup_key(key),
            AppState::FileView => { self.handle_file_view_key(key); Ok(()) }
            AppState::FileEdit => self.handle_file_edit_key(key),
            AppState::ProjectDetail => { self.handle_project_detail_key(key); Ok(()) }
            AppState::HealthView => { self.handle_health_view_key(key); Ok(()) }
            AppState::MemPalaceView => { self.handle_mempalace_key(key); Ok(()) }
            AppState::GraphReport => { self.handle_graph_report_key(key); Ok(()) }
            AppState::GitDiffView => { self.handle_git_diff_key(key); Ok(()) }
            AppState::HelpView => {
                // Any key closes help
                self.state = AppState::Dashboard;
                Ok(())
            }

            AppState::Dashboard => {
                if self.command_mode {
                    self.handle_command_key(key)?;
                } else {
                    self.handle_dashboard_key(key)?;
                }
                Ok(())
            }
        }
    }

    fn handle_git_diff_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(pending) = self.pending_file_changes.get(self.pending_change_cursor).cloned() {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = serde_json::json!({
                            "command": "ApproveFileChange",
                            "id": pending.id.to_string(),
                            "path": pending.path,
                            "approved": true
                        });
                        let _ = tx.send(msg.to_string());
                        self.add_activity("System", &format!("File change approved for {}", pending.path), "Info");
                    }
                    self.pending_file_changes.remove(self.pending_change_cursor);
                    if self.pending_change_cursor >= self.pending_file_changes.len() && !self.pending_file_changes.is_empty() {
                        self.pending_change_cursor = self.pending_file_changes.len() - 1;
                    }
                }
                if self.pending_file_changes.is_empty() {
                    self.state = AppState::Dashboard;
                } else {
                    // Load next diff
                    let next = &self.pending_file_changes[self.pending_change_cursor];
                    self.git_diff_lines = crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                if let Some(pending) = self.pending_file_changes.get(self.pending_change_cursor).cloned() {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = serde_json::json!({
                            "command": "ApproveFileChange",
                            "id": pending.id.to_string(),
                            "path": pending.path,
                            "approved": false
                        });
                        let _ = tx.send(msg.to_string());
                        self.add_activity("System", &format!("File change rejected for {}", pending.path), "Warning");
                    }
                    self.pending_file_changes.remove(self.pending_change_cursor);
                    if self.pending_change_cursor >= self.pending_file_changes.len() && !self.pending_file_changes.is_empty() {
                        self.pending_change_cursor = self.pending_file_changes.len() - 1;
                    }
                }
                if self.pending_file_changes.is_empty() {
                    self.state = AppState::Dashboard;
                } else {
                    // Load next diff
                    let next = &self.pending_file_changes[self.pending_change_cursor];
                    self.git_diff_lines = crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.git_diff_scroll > 0 {
                    self.git_diff_scroll -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.git_diff_lines.len() as u16).saturating_sub(self.height - 6);
                if self.git_diff_scroll < max {
                    self.git_diff_scroll += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.pending_change_cursor > 0 {
                    self.pending_change_cursor -= 1;
                    let next = &self.pending_file_changes[self.pending_change_cursor];
                    self.git_diff_lines = crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.pending_change_cursor + 1 < self.pending_file_changes.len() {
                    self.pending_change_cursor += 1;
                    let next = &self.pending_file_changes[self.pending_change_cursor];
                    self.git_diff_lines = crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            _ => {}
        }
    }

    fn handle_mempalace_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }

            // Filter typing
            KeyCode::Char('/') => {
                self.mp_filter.clear();
            }
            KeyCode::Char(c) if self.mp_proj_cursor.is_none() => {
                // Not navigating projects — accumulate filter
                self.mp_filter.push(c);
            }

            KeyCode::Backspace => {
                self.mp_filter.pop();
            }

            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(pi) = self.mp_proj_cursor {
                    if pi > 0 {
                        self.mp_proj_cursor = Some(pi - 1);
                    } else {
                        // Go back to room level
                        self.mp_proj_cursor = None;
                    }
                } else if self.mp_room_cursor > 0 {
                    self.mp_room_cursor -= 1;
                }
            }

            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(pi) = self.mp_proj_cursor {
                    let room = &self.mp_rooms[self.mp_room_cursor];
                    if pi + 1 < room.projects.len() {
                        self.mp_proj_cursor = Some(pi + 1);
                    }
                } else if self.mp_room_cursor + 1 < self.mp_rooms.len() {
                    self.mp_room_cursor += 1;
                }
            }

            // → / Enter: go into room or open project
            KeyCode::Right | KeyCode::Char('l') => {
                if self.mp_proj_cursor.is_none() && !self.mp_rooms.is_empty() {
                    let room = &self.mp_rooms[self.mp_room_cursor];
                    if !room.projects.is_empty() {
                        self.mp_proj_cursor = Some(0);
                    }
                }
            }

            // ← : go back to room level
            KeyCode::Left | KeyCode::Char('h') => {
                self.mp_proj_cursor = None;
            }

            KeyCode::Enter => {
                if let Some(pi) = self.mp_proj_cursor {
                    let proj_path = self.mp_rooms[self.mp_room_cursor].projects[pi].path.clone();
                    // Find matching entities project, else create a stub
                    let proj = self.projects.iter()
                        .find(|p| p.local_path == proj_path)
                        .cloned()
                        .unwrap_or_else(|| {
                            let mp = &self.mp_rooms[self.mp_room_cursor].projects[pi];
                            crate::entities::EntityProject {
                                name: mp.name.clone(),
                                category: self.mp_rooms[self.mp_room_cursor].folder_name.clone(),
                                local_path: proj_path,
                                github: None,
                                status: mp.status.clone(),
                                stars: None,
                                last_commit: None,
                                version: mp.version.clone(),
                                version_nickname: mp.version_nickname.clone(),
                            }
                        });
                    self.open_project_detail(proj);
                } else {
                    // Toggle expand/collapse
                    if let Some(exp) = self.mp_expanded.get_mut(self.mp_room_cursor) {
                        *exp = !*exp;
                    }
                }
            }

            // Space: toggle expand
            KeyCode::Char(' ') => {
                if let Some(exp) = self.mp_expanded.get_mut(self.mp_room_cursor) {
                    *exp = !*exp;
                    self.mp_proj_cursor = None;
                }
            }
            KeyCode::Char('C') | KeyCode::Char('G') | KeyCode::Char('A') => {
                if let Some(proj) = self.get_selected_mempalace_project() {
                    let agent = match key.code {
                        KeyCode::Char('C') => "claude",
                        KeyCode::Char('G') => "gemini",
                        _ => "antigravity",
                    };
                    self.add_activity("Agent", &format!("Launching {} from MemPalace", agent), "Info");
                    self.sync_status = Some(launch_agent(agent, &proj.path));
                }
            }
            _ => {}
        }
    }

    fn handle_key_search(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.state = AppState::Dashboard,
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.update_search();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.update_search();
            }
            KeyCode::Up => {
                if self.search_cursor > 0 {
                    self.search_cursor -= 1;
                }
            }
            KeyCode::Down => {
                if self.search_cursor + 1 < self.search_results.len() {
                    self.search_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(res) = self.search_results.get(self.search_cursor) {
                    let entry = FileEntry::new(
                        res.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        res.path.clone()
                    );
                    self.open_file_view(entry);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_file_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.file_changed_externally {
                    if let Some(ref file) = self.active_file.clone() {
                        let content = load_file_content(&file.path);
                        self.compliance = Some(compliance::check_file(&file.path, &content));
                        self.file_lines = content.lines().map(str::to_owned).collect();
                        self.file_scroll = 0;
                        self.watched_file_mtime = std::fs::metadata(&file.path).ok().and_then(|m| m.modified().ok());
                        self.file_changed_externally = false;
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Some(f) = self.active_file.clone() {
                    if !f.read_only {
                        self.open_file_edit(f);
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.file_scroll > 0 {
                    self.file_scroll -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.file_lines.len() as u16).saturating_sub(self.height - 6);
                if self.file_scroll < max {
                    self.file_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.file_scroll = self.file_scroll.saturating_sub(self.height / 2);
            }
            KeyCode::PageDown => {
                let max = (self.file_lines.len() as u16).saturating_sub(self.height - 6);
                self.file_scroll = (self.file_scroll + self.height / 2).min(max);
            }
            _ => {}
        }
    }

    fn handle_file_edit_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => self.save_file(),
                KeyCode::Char('q') => self.state = AppState::FileView,
                _ => self.editor.handle_key(key),
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => self.state = AppState::FileView,
            _ => self.editor.handle_key(key),
        }
        Ok(())
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.command_mode = false;
                self.command_buf.clear();
                self.palette_cursor = 0;
            }

            KeyCode::Enter => {
                // If user typed a command (buf starts with '/'), run it directly.
                // Otherwise use the palette cursor to pick a command.
                let cmd = if self.command_buf.starts_with('/') {
                    self.command_buf.trim().to_string()
                } else {
                    let filtered = filtered_palette(&self.command_buf);
                    filtered
                        .get(self.palette_cursor)
                        .map(|item| item.cmd.to_string())
                        .unwrap_or_default()
                };
                self.command_buf.clear();
                self.command_mode = false;
                self.palette_cursor = 0;
                if !cmd.is_empty() {
                    self.execute_command(&cmd)?;
                }
            }

            // Tab fills the selected palette item into the buffer
            KeyCode::Tab => {
                let filtered = filtered_palette(&self.command_buf);
                if let Some(item) = filtered.get(self.palette_cursor) {
                    self.command_buf = format!("{} ", item.cmd);
                    self.palette_cursor = 0;
                }
            }

            KeyCode::Up => {
                if self.palette_cursor > 0 {
                    self.palette_cursor -= 1;
                }
            }

            KeyCode::Down => {
                let max = filtered_palette(&self.command_buf).len().saturating_sub(1);
                if self.palette_cursor < max {
                    self.palette_cursor += 1;
                }
            }

            KeyCode::Backspace => {
                if self.command_buf.is_empty() {
                    self.command_mode = false;
                } else {
                    self.command_buf.pop();
                    self.palette_cursor = 0;
                }
            }

            KeyCode::Char(c) => {
                self.command_buf.push(c);
                self.palette_cursor = 0;
            }

            _ => {}
        }
        Ok(())
    }

    fn handle_setup_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::setup_wizard::WizardStep;

        // Editing mode — capture text input
        if self.wizard_editing {
            match key.code {
                KeyCode::Enter => {
                    self.wizard_commit_input();
                    self.wizard_editing = false;
                }
                KeyCode::Esc => { self.wizard_editing = false; }
                KeyCode::Char(c) => self.wizard_input.push(c),
                KeyCode::Backspace => { self.wizard_input.pop(); }
                _ => {}
            }
            return Ok(());
        }

        // Running — ignore all keys
        if self.wizard_running { return Ok(()); }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Navigate fields within a step
            KeyCode::Up | KeyCode::Char('k') => {
                if self.wizard_field_cursor > 0 { self.wizard_field_cursor -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.wizard_field_cursor += 1;
            }

            // Enter: begin editing or advance step
            KeyCode::Enter => {
                match &self.wizard_step {
                    WizardStep::Welcome => {
                        self.wizard_step = WizardStep::Workspace;
                        self.wizard_field_cursor = 0;
                    }
                    WizardStep::Done => {
                        // Apply config and open dashboard
                        let dev_ops = PathBuf::from(&self.wizard_dev_ops);
                        let master  = PathBuf::from(&self.wizard_master);
                        let skills  = dev_ops.join(".agents").join("skills");
                        let vault   = if self.wizard_vault.is_empty() { PathBuf::new() }
                                      else { PathBuf::from(&self.wizard_vault) };
                        if !dev_ops.as_os_str().is_empty() {
                            let cfg = Config {
                                dev_ops_path: dev_ops,
                                master_md_path: master,
                                skills_path: skills,
                                vault_projects_path: vault,
                            };
                            let _ = cfg.save();
                            self.config = cfg;
                        }
                        self.state = AppState::Dashboard;
                        let tx = self.tx.clone();
                        tx.send(BgMsg::TransitionToDashboard).ok();
                    }
                    WizardStep::Initialize => {
                        self.wizard_run_initialize();
                    }
                    _ => {
                        // Start editing current field
                        self.wizard_start_edit();
                    }
                }
            }

            // [s] — advance to next step (also runs current step's setup)
            KeyCode::Char('s') => {
                self.wizard_advance_step();
            }

            // [Tab] — skip/unskip agent steps
            KeyCode::Tab => {
                match self.wizard_step {
                    WizardStep::Claude      => self.wizard_skip_claude      = !self.wizard_skip_claude,
                    WizardStep::Gemini      => self.wizard_skip_gemini      = !self.wizard_skip_gemini,
                    WizardStep::Antigravity => self.wizard_skip_antigravity = !self.wizard_skip_antigravity,
                    _ => { self.wizard_advance_step(); }
                }
            }

            _ => {}
        }
        Ok(())
    }

    fn wizard_commit_input(&mut self) {
        use crate::setup_wizard::WizardStep;
        match self.wizard_step {
            WizardStep::Workspace => {
                match self.wizard_field_cursor {
                    0 => self.wizard_dev_ops = self.wizard_input.clone(),
                    1 => self.wizard_github  = self.wizard_input.clone(),
                    2 => self.wizard_vault   = self.wizard_input.clone(),
                    _ => {}
                }
            }
            WizardStep::Master => {
                self.wizard_master = self.wizard_input.clone();
            }
            _ => {}
        }
        self.wizard_input.clear();
    }

    fn wizard_start_edit(&mut self) {
        use crate::setup_wizard::WizardStep;
        self.wizard_input = match self.wizard_step {
            WizardStep::Workspace => match self.wizard_field_cursor {
                0 => self.wizard_dev_ops.clone(),
                1 => self.wizard_github.clone(),
                2 => self.wizard_vault.clone(),
                _ => String::new(),
            },
            WizardStep::Master => self.wizard_master.clone(),
            _ => return,
        };
        self.wizard_editing = true;
    }

    fn wizard_advance_step(&mut self) {
        use crate::setup_wizard::WizardStep;
        use std::path::Path;

        // Run setup actions for current step before advancing
        let dev_ops = PathBuf::from(&self.wizard_dev_ops);
        let master  = PathBuf::from(&self.wizard_master);
        let tx = self.tx.clone();
        let github  = self.wizard_github.clone();
        let skip_c  = self.wizard_skip_claude;
        let skip_g  = self.wizard_skip_gemini;
        let skip_a  = self.wizard_skip_antigravity;

        match &self.wizard_step {
            WizardStep::Workspace if !dev_ops.as_os_str().is_empty() => {
                let tx2 = tx.clone();
                let d = dev_ops.clone(); let gh = github.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_workspace(&d, &gh);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Master => {
                if !master.as_os_str().is_empty() {
                    let tx2 = tx.clone(); let m = master.clone(); let gh = github.clone();
                    thread::spawn(move || {
                        let actions = crate::setup_wizard::exec_master(&m, &gh);
                        tx2.send(BgMsg::WizardActions(actions)).ok();
                    });
                }
            }
            WizardStep::Claude if !skip_c => {
                let tx2 = tx.clone(); let d = dev_ops.clone(); let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_claude(&d, &m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Gemini if !skip_g => {
                let tx2 = tx.clone(); let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_gemini(&m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Antigravity if !skip_a => {
                let tx2 = tx.clone(); let d = dev_ops.clone(); let m = master.clone();
                thread::spawn(move || {
                    let actions = crate::setup_wizard::exec_antigravity(&d, &m);
                    tx2.send(BgMsg::WizardActions(actions)).ok();
                });
            }
            WizardStep::Skills => {
                if !dev_ops.as_os_str().is_empty() {
                    let tx2 = tx.clone(); let d = dev_ops.clone();
                    thread::spawn(move || {
                        let actions = crate::setup_wizard::exec_skills(&d);
                        tx2.send(BgMsg::WizardActions(actions)).ok();
                    });
                }
            }
            _ => {}
        }

        self.wizard_step = self.wizard_step.next();
        self.wizard_field_cursor = 0;
    }

    fn wizard_run_initialize(&mut self) {
        use std::path::Path;
        let dev_ops = PathBuf::from(&self.wizard_dev_ops);
        let master  = PathBuf::from(&self.wizard_master);
        let vault   = if self.wizard_vault.is_empty() { None }
                      else { Some(PathBuf::from(&self.wizard_vault)) };
        let skills  = dev_ops.join(".agents").join("skills");
        let tx = self.tx.clone();

        if dev_ops.as_os_str().is_empty() {
            return;
        }

        self.wizard_running = true;
        thread::spawn(move || {
            let actions = crate::setup_wizard::exec_initialize(
                &dev_ops, &master, &skills, vault.as_deref(),
            );
            tx.send(BgMsg::WizardActions(actions)).ok();
            tx.send(BgMsg::WizardDone).ok();
        });
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => {
                self.state = AppState::HelpView;
            }
            KeyCode::Char('i') | KeyCode::Char('I') => {
                if !self.pending_file_changes.is_empty() {
                    let first = &self.pending_file_changes[self.pending_change_cursor];
                    self.git_diff_lines = crate::app::editor::simple_diff(&first.original_content, &first.new_content);
                    self.state = AppState::GitDiffView;
                }
            }
            KeyCode::Char('L') => {
                // Uppercase L = launcher (lowercase l = vim right)
                if self.menu_cursor == 7 && self.right_panel_focus {
                    if let Some(proj) = self.project_at_cursor().cloned() {
                        self.active_project = Some(proj);
                        self.show_launcher = true;
                    }
                }
            }
            KeyCode::Char('s') => {
                // Cycle sort mode on All Projects view
                if self.menu_cursor == 7 {
                    self.project_sort = self.project_sort.next();
                    self.project_cursor = 0;
                }
            }
            KeyCode::Char('/') | KeyCode::Tab => {
                self.command_mode = true;
                self.palette_cursor = 0;
                // '/' starts with a slash so typed commands work; Tab shows full palette
                if key.code == KeyCode::Char('/') {
                    self.command_buf = "/".into();
                } else {
                    self.command_buf.clear();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.right_panel_focus {
                    match self.menu_cursor {
                        0 => { if self.task_cursor > 0 { self.task_cursor -= 1; } }
                        6 => { if self.search_cursor > 0 { self.search_cursor -= 1; } }
                        7 => { if self.project_cursor > 0 { self.project_cursor -= 1; } }
                        _ => { if self.right_file_cursor > 0 { self.right_file_cursor -= 1; } }
                    }
                } else if self.menu_cursor > 0 {
                    self.menu_cursor -= 1;
                    self.right_file_cursor = 0;
                    self.project_cursor = 0;
                    self.search_cursor = 0;
                    self.right_panel_scroll = 0;
                    self.right_panel_focus = false;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.right_panel_focus {
                    match self.menu_cursor {
                        0 => {
                            let max = self.tasks.len().saturating_sub(1);
                            if self.task_cursor < max { self.task_cursor += 1; }
                        }
                        6 => {
                            let max = self.search_results.len().saturating_sub(1);
                            if self.search_cursor < max { self.search_cursor += 1; }
                        }
                        7 => {
                            let max = self.projects.len().saturating_sub(1);
                            if self.project_cursor < max { self.project_cursor += 1; }
                        }
                        _ => {
                            let max = self.current_menu_files().len().saturating_sub(1);
                            if self.right_file_cursor < max { self.right_file_cursor += 1; }
                        }
                    }
                } else if self.menu_cursor < MENU_ITEMS.len() - 1 {
                    self.menu_cursor += 1;
                    self.right_file_cursor = 0;
                    self.project_cursor = 0;
                    self.search_cursor = 0;
                    self.right_panel_scroll = 0;
                    self.right_panel_focus = false;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let can_focus = !self.current_menu_files().is_empty()
                    || (self.menu_cursor == 0 && !self.tasks.is_empty())
                    || (self.menu_cursor == 6 && !self.search_results.is_empty())
                    || (self.menu_cursor == 7 && !self.projects.is_empty());
                if can_focus {
                    self.right_panel_focus = true;
                    self.right_file_cursor = 0;
                    self.right_panel_scroll = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.right_panel_focus = false;
            }
            KeyCode::Char(' ') | KeyCode::Char('x') | KeyCode::Char('X') => {
                if self.menu_cursor == 0 && self.right_panel_focus {
                    if let Some(task) = self.tasks.get_mut(self.task_cursor) {
                        task.completed = !task.completed;
                        let _ = crate::tasks::save_tasks(&self.config.dev_ops_path, &self.tasks);
                    }
                }
            }

            // Task → Agent dispatch  (only active when task panel is focused)
            KeyCode::Char('c') => {
                if self.menu_cursor == 0 && self.right_panel_focus {
                    self.dispatch_task("claude");
                }
            }
            KeyCode::Char('g') => {
                if self.menu_cursor == 0 && self.right_panel_focus {
                    self.dispatch_task("gemini");
                }
            }
            KeyCode::Char('a') => {
                if self.menu_cursor == 0 && self.right_panel_focus {
                    self.dispatch_task("antigravity");
                }
            }
            KeyCode::Enter => {
                if self.right_panel_focus {
                    match self.menu_cursor {
                        6 => {
                            if let Some(result) = self.search_results.get(self.search_cursor) {
                                let name = result.path.file_name()
                                    .unwrap_or_default().to_string_lossy().into_owned();
                                self.open_file_view(FileEntry::new(name, result.path.clone()));
                            }
                        }
                        7 => {
                            if let Some(proj) = self.project_at_cursor().cloned() {
                                self.open_project_detail(proj);
                            }
                        }
                        _ => {
                            let files = self.current_menu_files();
                            if let Some(entry) = files.into_iter().nth(self.right_file_cursor) {
                                self.open_file_view(entry);
                            }
                        }
                    }
                }
            }
            KeyCode::Char('e') => {
                if self.right_panel_focus {
                    let files = self.current_menu_files();
                    if let Some(entry) = files.into_iter().nth(self.right_file_cursor) {
                        if !entry.read_only {
                            self.open_file_edit(entry);
                        }
                    }
                }
            }
            KeyCode::Char('o') => {
                if self.right_panel_focus {
                    let files = self.current_menu_files();
                    if let Some(entry) = files.into_iter().nth(self.right_file_cursor) {
                        let _ = crate::discovery::open_in_editor(&entry.path);
                    }
                }
            }
            KeyCode::Char('C') | KeyCode::Char('G') | KeyCode::Char('A') => {
                if self.right_panel_focus {
                    let project_path = match self.menu_cursor {
                        7 => self.project_at_cursor().map(|p| p.local_path.clone()),
                        _ => None,
                    };

                    if let Some(path) = project_path {
                        let agent = match key.code {
                            KeyCode::Char('C') => "claude",
                            KeyCode::Char('G') => "gemini",
                            _ => "antigravity",
                        };
                        self.add_activity("Agent", &format!("Launching {} for project", agent), "Info");
                        self.sync_status = Some(launch_agent(agent, &path));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn execute_command(&mut self, raw: &str) -> Result<()> {
        let raw = raw.trim();
        if !raw.starts_with('/') {
            return Ok(());
        }
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "/sync" | "/setup" => {
                self.is_syncing = true;
                self.sync_status = None;
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
                self.projects = projects.clone();
                match crate::entities::save_entities(&self.config.dev_ops_path, projects) {
                    Ok(_) => self.sync_status = Some("Discovery complete: entities.json updated".into()),
                    Err(e) => self.sync_status = Some(format!("Discovery error: {}", e)),
                }
                self.add_activity("System", "Full Dev Ops discovery complete", "Info");
            }
            "/q" | "/quit" | "/exit" => self.should_quit = true,
            "/rules" => {
                self.menu_cursor = 1;
                self.right_panel_focus = true;
                self.right_file_cursor = 0;
            }
            "/memory" => {
                self.menu_cursor = 5;
                self.right_panel_focus = true;
                self.right_file_cursor = 2;
            }
            "/mempalace" | "/palace" | "/mp" => {
                if self.mp_rooms.is_empty() && !self.mp_is_building {
                    self.mp_is_building = true;
                    let tx = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&dev_ops))).ok();
                    });
                }
                self.state = AppState::MemPalaceView;
                self.mp_filter.clear();
            }
            "/view" => {
                if !arg.is_empty() {
                    if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                        self.open_file_view(entry);
                    }
                }
            }
            "/edit" => {
                if !arg.is_empty() {
                    if let Some(entry) = find_file_by_name(arg, &self.config.master_md_path) {
                        if !entry.read_only {
                            self.open_file_edit(entry);
                        }
                    }
                }
            }
            "/search" | "/s" => {
                self.menu_cursor = 6;
                self.right_panel_focus = false;
                if !arg.is_empty() {
                    self.add_activity("User", &format!("Searching for: {}", arg), "Info");
                    if let Some(ref idx) = self.index {
                        self.search_results = idx.search(arg);
                        self.search_cursor = 0;
                        if !self.search_results.is_empty() {
                            self.right_panel_focus = true;
                        }
                    } else {
                        self.index_status = Some("Index not ready — try again shortly".into());
                    }
                }
            }
            "/memo" | "/note" => {
                if !arg.is_empty() {
                    let result = append_memo(arg, &self.config.dev_ops_path);
                    self.sync_status = Some(result);
                }
            }
            "/scan-system" | "/audit" => {
                self.is_scanning_system = true;
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let report = crate::system_scan::scan_system();
                    tx.send(BgMsg::AiAuditReport(report)).ok();
                });
            }
            "/open" | "/project" => {
                if !arg.is_empty() {
                    let q = arg.to_lowercase();
                    if let Some(proj) = self.projects.iter()
                        .find(|p| p.name.to_lowercase().contains(&q))
                        .cloned()
                    {
                        self.open_project_detail(proj);
                    } else {
                        self.sync_status = Some(format!("Project not found: {}", arg));
                    }
                } else {
                    self.menu_cursor = 7;
                    self.right_panel_focus = false;
                }
            }
            "/timeline" | "/history" => {
                self.menu_cursor = 8;
                self.right_panel_focus = false;
            }
            "/logs" | "/log" => {
                self.menu_cursor = 9;
                self.right_panel_focus = false;
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
                    self.sync_status = Some(format!("Task added [{}→{}]", agent_hint, proj_hint));
                    self.tasks.push(new_task);
                    let _ = crate::tasks::save_tasks(&self.config.dev_ops_path, &self.tasks);
                } else if let Some(agent) = arg.strip_prefix("send ") {
                    self.dispatch_task(agent.trim());
                } else if arg == "load" {
                    if let Ok(tasks) = crate::tasks::load_tasks(&self.config.dev_ops_path) {
                        self.tasks = tasks;
                        self.sync_status = Some(format!("{} tasks loaded", self.tasks.len()));
                    }
                }
            }
            "/vault-create" => {
                let name = arg.trim();
                if name.is_empty() {
                    self.sync_status = Some("Usage: /vault-create <project_name>".into());
                } else {
                    let proj = self.projects.iter().find(|p| p.name == name).cloned();
                    if let Some(p) = proj {
                        let vault_file = self.config.vault_projects_path.join(format!("{}.md", p.name));
                        if vault_file.exists() {
                            self.sync_status = Some("Vault note already exists".into());
                        } else {
                            let content = format!(
                                "---\ncategory: {}\nstatus: {}\ntags: [project, raios]\ncreated: {}\n---\n# {}\n\n## Overview\n{} is a project managed by R-AI-OS.\n\n## Details\n- Path: {}\n",
                                p.category, p.status, chrono::Local::now().format("%Y-%m-%d"), p.name, p.name, p.local_path.display()
                            );
                            if std::fs::write(&vault_file, content).is_ok() {
                                self.vault_projects.push(p.name.clone());
                                self.sync_status = Some(format!("Vault note created: {}", p.name));
                                self.add_activity("System", &format!("Created vault note for {}", p.name), "Info");
                            } else {
                                self.sync_status = Some("Failed to write vault note".into());
                            }
                        }
                    } else {
                        self.sync_status = Some(format!("Project not found: {}", name));
                    }
                }
            }
            "/health" => {
                if !self.projects.is_empty() {
                    self.is_checking_health = true;
                    self.state = AppState::HealthView;
                    if let Some(ref tx_daemon) = self.tx_daemon {
                        let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    }
                } else {
                    self.sync_status = Some("Load entities.json first".into());
                }
            }
            "/reindex" => {
                self.add_activity("System", "Requesting search re-index", "Info");
                if self.config.dev_ops_path.exists() && !self.is_indexing {
                    self.is_indexing = true;
                    self.index_status = Some("Rebuilding index...".into());
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
                if let Some(ref proj) = self.active_project.clone() {
                    let msg = self.run_graphify(&proj.local_path);
                    self.sync_status = Some(msg);
                } else {
                    self.sync_status = Some("Open a project detail first to run graphify".into());
                }
            }
            _ => {}
        }
        Ok(())
    }

}

fn launch_agent(agent: &str, project_path: &Path) -> String {
    let path_str = project_path.to_string_lossy().into_owned();
    // Try Windows Terminal
    if std::process::Command::new("wt")
        .args(["-d", &path_str, "--", agent])
        .spawn()
        .is_ok()
    {
        return format!("{} launched in Windows Terminal", agent);
    }
    // Fallback: new cmd window
    let cmd_str = format!("cd /d \"{}\" && {}", path_str, agent);
    match std::process::Command::new("cmd")
        .args(["/c", "start", "cmd", "/k", &cmd_str])
        .spawn()
    {
        Ok(_) => format!("{} launched", agent),
        Err(e) => format!("Launch error: {}", e),
    }
}

fn append_memo(text: &str, dev_ops: &Path) -> String {
    use std::fs::OpenOptions;
    let ts = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let entry = format!("- [{}] {}\n", ts, text);
    let notes_path = dev_ops.join("_session_notes.md");
    match OpenOptions::new().create(true).append(true).open(&notes_path) {
        Ok(mut f) => {
            let _ = f.write_all(entry.as_bytes());
            format!("Memo saved → _session_notes.md")
        }
        Err(e) => format!("Memo error: {}", e),
    }
}

