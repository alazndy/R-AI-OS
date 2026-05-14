use crate::config::Config;
use crate::filebrowser::{
    discover_all_agent_rules, get_agent_config_files, get_master_rule_files, get_mempalace_files,
    get_policy_files, load_recent_projects,
};
use crate::requirements::check_requirements;
use std::thread;
use std::time::{Duration, SystemTime};

use crate::app::{state::*, App};
use crate::compliance;
use crate::filebrowser::load_file_content;

impl App {
    pub fn handle_bg_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::BootResult { name, pass, done } => {
                self.system.boot_results.push((name, pass));
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
                    self.wizard.dev_ops = p.to_string_lossy().into_owned();
                }
                if let Some(p) = &detected.master_md {
                    self.wizard.master = p.to_string_lossy().into_owned();
                }
                if let Some(p) = &detected.vault_projects {
                    self.wizard.vault = p.to_string_lossy().into_owned();
                }

                // Keep legacy setup_fields for compatibility
                self.setup.requirements = check_requirements();
                self.setup.fields = vec![
                    SetupField::new("Dev Ops Path", "Root workspace")
                        .with_detected(detected.dev_ops.clone()),
                    SetupField::new("MASTER.md Path", "Agent constitution")
                        .with_detected(detected.master_md.clone()),
                    SetupField::new("Skills Path", ".agents/skills")
                        .with_detected(detected.skills.clone()),
                    SetupField::new("Vault Projects Path", "Obsidian Vault")
                        .with_detected(detected.vault_projects.clone()),
                ];
                self.setup.cursor = 0;
                self.wizard.step = crate::setup_wizard::WizardStep::Welcome;
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
                self.system.graphify_script =
                    crate::health::find_graphify_script(&self.config.dev_ops_path);
                let tx = self.tx.clone();
                let cfg = self.config.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents()))
                        .ok();
                    tx.send(BgMsg::Skills(crate::discovery::discover_skills(
                        &cfg.skills_path,
                    )))
                    .ok();
                    tx.send(BgMsg::MemPalaceFiles(get_mempalace_files(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
                    tx.send(BgMsg::MasterFiles(get_master_rule_files(
                        &cfg.master_md_path,
                    )))
                    .ok();
                    tx.send(BgMsg::AgentFiles(get_agent_config_files())).ok();
                    tx.send(BgMsg::PolicyFiles(get_policy_files())).ok();
                    tx.send(BgMsg::AgentRuleGroups(discover_all_agent_rules(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
                    let discovered = crate::entities::discover_entities(&cfg.dev_ops_path);
                    let count = discovered.len();
                    tx.send(BgMsg::Projects(discovered)).ok();
                    let log = LogEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        sender: "System".into(),
                        content: format!("Discovery: Found {} projects in total", count),
                    };
                    tx.send(BgMsg::NewLog(log)).ok();
                    tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
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
                    let mut cat_dirty: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for p in &projects {
                        match p.status.as_str() {
                            "archived" | "legacy" => stats.archived += 1,
                            _ => stats.active += 1,
                        }
                        if p.github.is_none() {
                            stats.no_github += 1;
                        }
                        if !p.local_path.join("memory.md").exists() {
                            stats.no_memory += 1;
                        }
                        if !p.local_path.join("SIGMAP.md").exists() {
                            stats.no_sigmap += 1;
                        }
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
                    stats.top_dirty_category = cat_dirty
                        .into_iter()
                        .max_by_key(|(_, v)| *v)
                        .map(|(k, _)| k)
                        .unwrap_or_default();
                    tx.send(BgMsg::StatsReady(stats)).ok();
                });
            }
            BgMsg::SearchResults(results) => {
                self.search.results = results;
                self.search.cursor = 0;
            }
            BgMsg::RecentProjects(p) => {
                self.projects.recent = p;
                self.system.memory_refresh_pending = false;
            }
            BgMsg::Agents(a) => self.inventory.agents = a,
            BgMsg::Skills(s) => self.inventory.skills = s,
            BgMsg::MasterFiles(m) => self.inventory.master_files = m,
            BgMsg::AgentFiles(a) => self.inventory.agent_files = a,
            BgMsg::PolicyFiles(p) => self.inventory.policy_files = p,
            BgMsg::MemPalaceFiles(m) => self.inventory.mempalace_files = m,
            BgMsg::SyncDone(msg) => {
                self.system.is_syncing = false;
                self.system.sync_status = Some(msg.clone());
                self.add_activity("System", &msg, "Info");
                let tx = self.tx.clone();
                let cfg = self.config.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents()))
                        .ok();
                    tx.send(BgMsg::MemPalaceFiles(
                        crate::filebrowser::get_mempalace_files(&cfg.dev_ops_path),
                    ))
                    .ok();
                    tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(
                        &cfg.dev_ops_path,
                    )))
                    .ok();
                });
            }
            BgMsg::SyncError(e) => {
                self.system.is_syncing = false;
                self.system.sync_status = Some(format!("Error: {}", e));
            }
            BgMsg::AgentRuleGroups(groups) => {
                self.inventory.agent_rule_groups = groups;
                // Flatten into agent_files so the file panel still works
                self.inventory.agent_files = self
                    .inventory.agent_rule_groups
                    .iter()
                    .flat_map(|g| g.files.iter().cloned())
                    .collect();
            }
            BgMsg::Projects(p) => {
                self.projects.list = p;
                self.projects.cursor = 0;
            }
            BgMsg::Tasks(t) => {
                self.tasks.list = t;
            }
            BgMsg::VaultStatus(v) => {
                self.system.vault_projects = v;
            }
            BgMsg::ActivePorts(ports) => {
                self.system.active_ports = ports;
            }
            BgMsg::HandoverApproved {
                target,
                instruction: _,
                count,
            } => {
                self.add_activity(
                    "System",
                    &format!("Handover to {} approved. Count: {}", target, count),
                    "Info",
                );
            }
            BgMsg::HumanApprovalRequired {
                target,
                instruction,
                reason,
            } => {
                self.add_activity(
                    "System",
                    &format!("Human Approval Required: {}", reason),
                    "Warning",
                );
                self.system.handover_modal = Some((target, instruction));
            }
            BgMsg::FileChangeRequested { approval } => {
                self.system.pending_file_changes.push(approval.clone());
                self.system.pending_change_cursor = self.system.pending_file_changes.len().saturating_sub(1);
                self.projects.git_diff_lines = crate::app::editor::simple_diff(
                    &approval.original_content,
                    &approval.new_content,
                );
                self.state = AppState::GitDiffView;
                self.add_activity(
                    "Agent",
                    &format!("File Change Requested: {}", approval.path),
                    "Warning",
                );
            }
            BgMsg::HumanApprovalResult { status } => {
                self.add_activity(
                    "System",
                    &format!("Human Approval Result: {}", status),
                    "Info",
                );
                self.system.handover_modal = None;
            }
            BgMsg::AiAuditReport(report) => {
                self.system.report = Some(report);
                self.system.is_scanning = false;
                self.ui.menu_cursor = 11; // Open Diagnostics/System tab
            }
            BgMsg::HealthReport(report) => {
                self.health.report = report;
                self.health.is_checking = false;
                self.health.cursor = 0;
            }
            BgMsg::SentinelUpdate {
                project,
                status,
                error_count,
            } => {
                let level = if status == "Failed" {
                    "Warning"
                } else {
                    "Info"
                };
                self.add_activity(
                    "Sentinel",
                    &format!("{}: {} ({} errors)", project, status, error_count),
                    level,
                );
            }
            BgMsg::StateSync {
                projects,
                health_reports,
                active_agents,
                index_ready,
                handover_count,
                pending_file_changes,
                sentinel_files,
            } => {
                self.projects.list = projects;
                self.health.report = health_reports;
                self.system.active_agents = active_agents;
                self.system.handover_count = handover_count as usize;
                self.system.pending_file_changes = pending_file_changes;
                self.system.sentinel_files = sentinel_files;
                if index_ready {
                    self.search.status = Some("Index Ready (Synced)".into());
                }
                self.health.is_checking = false;
                self.add_activity("System", "Full state synchronized with daemon", "Info");
            }
            BgMsg::ProjectOpened { memory, git_log } => {
                self.projects.memory_lines = memory;
                self.projects.git_log = git_log;
            }
            BgMsg::IndexReady(idx) => {
                let doc_count = idx.doc_count;
                self.search.index = Some(idx);
                self.search.is_indexing = false;
                self.search.status = Some(format!("{} files indexed", doc_count));
            }
            BgMsg::IndexError(e) => {
                self.search.is_indexing = false;
                self.search.status = Some(format!("Index error: {}", e));
            }
            BgMsg::ActivityUpdate(mut acts) => {
                self.timeline.activities.append(&mut acts);
            }
            BgMsg::NewLog(log) => {
                self.timeline.logs.push(log);
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
                            self.system.memory_watch.insert(mem, mtime);
                        }
                    }
                }
                self.mempalace.expanded = vec![true; n];
                self.mempalace.rooms = rooms;
                self.mempalace.room_cursor = 0;
                self.mempalace.proj_cursor = None;
                self.mempalace.is_building = false;
            }
            BgMsg::StatsReady(stats) => {
                self.system.stats_cache = Some(stats);
                self.system.is_computing_stats = false;
            }
            BgMsg::AgentStatusReady(status) => {
                self.wizard.agent_status = Some(status);
            }
            BgMsg::WizardActions(actions) => {
                self.wizard.action_log.extend(actions);
                self.wizard.running = false;
            }
            BgMsg::WizardDone => {
                self.wizard.running = false;
                self.wizard.step = crate::setup_wizard::WizardStep::Done;
            }
            BgMsg::GitActionDone {
                project,
                action,
                ok,
                message,
            } => {
                let status = if ok {
                    format!("✓ {} {} — {}", action, project, message)
                } else {
                    format!("✗ {} {} failed: {}", action, project, message)
                };
                self.system.sync_status = Some(status.clone());
                self.add_activity("Git", &status, if ok { "Info" } else { "Warning" });
            }
            BgMsg::FileChanged(path) => {
                // Bouncing Limit takibi: _session_notes.md değiştiğinde ardışık HANDOVER kontrolü yap
                if path.file_name().and_then(|n| n.to_str()) == Some("_session_notes.md") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let lines: Vec<&str> =
                            content.lines().filter(|l| !l.trim().is_empty()).collect();
                        let mut count = 0;
                        for line in lines.iter().rev() {
                            if line.contains("HANDOVER →") {
                                count += 1;
                            } else if !line.trim().starts_with("Context:") {
                                break;
                            }
                        }
                        self.system.handover_count = count;
                        self.system.bouncing_alert = count >= 3;
                    }
                }

                // Eğer değişen dosya aktif açık olan dosya ise otomatik reload ve compliance tetikle
                if let Some(ref active) = self.editor.active_file {
                    if active.path == path {
                        let content = load_file_content(&path);
                        self.health.compliance = Some(compliance::check_file(&path, &content));
                        self.editor.lines = content.lines().map(str::to_owned).collect();
                        self.editor.watched_mtime = std::fs::metadata(&path)
                            .ok()
                            .and_then(|m| m.modified().ok());
                        self.editor.changed_externally = false;
                    }
                }
            }
        }
    }
}
