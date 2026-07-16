#![allow(clippy::field_reassign_with_default)]

use raios_core::config::Config;
use raios_runtime::filebrowser::{
    discover_all_agent_rules, get_agent_config_files, get_master_rule_files, get_mempalace_files,
    get_policy_files, load_recent_projects,
};
use raios_core::requirements::check_requirements;
use std::thread;
use std::time::{Duration, SystemTime};

use raios_surface_tui::app::{state::*, App};
use raios_runtime::compliance;
use raios_runtime::filebrowser::load_file_content;

impl App {
    pub fn handle_bg_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::BootResult { name, pass, done } => self.handle_boot_result(name, pass, done),
            BgMsg::TransitionToSetup => self.handle_transition_to_setup(),
            BgMsg::TransitionToDashboard => self.handle_transition_to_dashboard(),
            BgMsg::ControlEvent(evt) => {
                crate::app::reducer::reduce_event(&mut self.store, evt);
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
            BgMsg::SyncDone(msg) => self.handle_sync_done(msg),
            BgMsg::SyncError(e) => {
                self.system.is_syncing = false;
                self.system.sync_status = Some(format!("Error: {}", e));
            }
            BgMsg::AgentRuleGroups(groups) => {
                self.inventory.agent_rule_groups = groups;
                // Flatten into agent_files so the file panel still works
                self.inventory.agent_files = self
                    .inventory
                    .agent_rule_groups
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
            BgMsg::FileChangeRequested { approval } => self.handle_file_change_requested(approval),
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
            BgMsg::BuildTestDepsResult { idx, health } => {
                if let Some(entry) = self.health.report.get_mut(idx) {
                    entry.build_ok = health.build_ok;
                    entry.test_passed = health.test_passed;
                    entry.test_failed = health.test_failed;
                    entry.deps_outdated = health.deps_outdated;
                    entry.deps_cve = health.deps_cve;
                }
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
            BgMsg::ProjectOpened(data) => {
                self.projects.memory_lines = data.memory_lines;
                self.projects.git_log = data.git_log;
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
            BgMsg::MemPalaceBuilt(rooms) => self.handle_mempalace_built(rooms),
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
                self.wizard.step = raios_surface_tui::setup_wizard::WizardStep::Done;
            }
            BgMsg::GitActionDone {
                project,
                action,
                ok,
                message,
            } => self.handle_git_action_done(project, action, ok, message),
            BgMsg::FileChanged(path) => self.handle_file_changed(path),
            BgMsg::AgentStarted { agent_id, name, project_path } => {
                self.system.active_agents.push(raios_runtime::daemon::proxy::AgentProcess {
                    id: uuid::Uuid::parse_str(&agent_id).unwrap_or_default(),
                    name: name.clone(),
                    status: "Running".into(),
                    started_at: std::time::SystemTime::now(),
                    logs: vec![],
                });
                self.add_activity(
                    "Agent",
                    &format!("{} started in {}", name, project_path),
                    "Info",
                );
            }
            BgMsg::AgentStopped { agent_id, name, final_status } => {
                if let Ok(id) = uuid::Uuid::parse_str(&agent_id) {
                    if let Some(agent) = self.system.active_agents.iter_mut().find(|a| a.id == id) {
                        agent.status = final_status.clone();
                    }
                }
                self.add_activity(
                    "Agent",
                    &format!("{} stopped — {}", name, final_status),
                    if final_status.contains("Error") || final_status.contains("Killed") {
                        "Warning"
                    } else {
                        "Info"
                    },
                );
            }
            BgMsg::HealthDelta(report) => {
                self.health.report = report;
                self.add_activity("Health", "Health delta received from daemon", "Info");
            }
            BgMsg::RemoteCommandResult { output } => {
                // Show first line in status bar, full output is already in Live Logs
                let preview = output
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("done")
                    .chars()
                    .take(80)
                    .collect::<String>();
                self.system.sync_status = Some(format!("✓ {}", preview));
            }
            BgMsg::ExtensionsLoaded(exts) => self.handle_extensions_loaded(exts),
            BgMsg::ExtCmdOutput { ext, cmd, line } => self.handle_ext_cmd_output(ext, cmd, line),
        }
    }

    fn handle_boot_result(&mut self, name: String, pass: bool, done: bool) {
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

    fn handle_transition_to_setup(&mut self) {
        use raios_core::config::Config as Cfg;
        let detected = Cfg::auto_detect();

        if let Some(p) = &detected.dev_ops {
            self.wizard.dev_ops = p.to_string_lossy().into_owned();
        }
        if let Some(p) = &detected.master_md {
            self.wizard.master = p.to_string_lossy().into_owned();
        }
        if let Some(p) = &detected.vault_projects {
            self.wizard.vault = p.to_string_lossy().into_owned();
        }

        self.setup.requirements = check_requirements();
        self.setup.fields = vec![
            SetupField::new("Dev Ops Path", "Root workspace").with_detected(detected.dev_ops.clone()),
            SetupField::new("MASTER.md Path", "Agent constitution")
                .with_detected(detected.master_md.clone()),
            SetupField::new("Skills Path", ".agents/skills").with_detected(detected.skills.clone()),
            SetupField::new("Vault Projects Path", "Obsidian Vault")
                .with_detected(detected.vault_projects.clone()),
        ];
        self.setup.cursor = 0;
        self.wizard.step = raios_surface_tui::setup_wizard::WizardStep::Welcome;
        self.state = AppState::Setup;

        let tx = self.tx.clone();
        thread::spawn(move || {
            let status = raios_surface_tui::setup_wizard::detect_agents();
            tx.send(BgMsg::AgentStatusReady(status)).ok();
        });
    }

    fn handle_transition_to_dashboard(&mut self) {
        self.state = AppState::Dashboard;
        if let Err(problem) = self.client.send_query(raios_contracts::Query::GetSystemSnapshot) {
            self.store.last_error = Some(problem.message);
        }
        self.system.graphify_script =
            raios_runtime::health::find_graphify_script(&self.config.dev_ops_path);
        let tx = self.tx.clone();
        let cfg = self.config.clone();
        thread::spawn(move || {
            tx.send(BgMsg::RecentProjects(load_recent_projects(&cfg.dev_ops_path)))
                .ok();
            tx.send(BgMsg::Agents(raios_runtime::discovery::discover_agents())).ok();
            tx.send(BgMsg::Skills(raios_runtime::discovery::discover_skills(&cfg.skills_path)))
                .ok();
            tx.send(BgMsg::MemPalaceFiles(get_mempalace_files(&cfg.dev_ops_path)))
                .ok();
            tx.send(BgMsg::MasterFiles(get_master_rule_files(&cfg.master_md_path)))
                .ok();
            tx.send(BgMsg::AgentFiles(get_agent_config_files())).ok();
            tx.send(BgMsg::PolicyFiles(get_policy_files())).ok();
            tx.send(BgMsg::AgentRuleGroups(discover_all_agent_rules(&cfg.dev_ops_path)))
                .ok();
            let discovered = raios_core::entities::discover_entities(&cfg.dev_ops_path);
            let count = discovered.len();
            tx.send(BgMsg::Projects(discovered)).ok();
            tx.send(BgMsg::NewLog(LogEntry {
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                sender: "System".into(),
                content: format!("Discovery: Found {} projects in total", count),
            }))
            .ok();
            tx.send(BgMsg::MemPalaceBuilt(raios_core::mempalace::build(&cfg.dev_ops_path)))
                .ok();
            if let Ok(tasks) = raios_runtime::tasks::load_tasks(&cfg.dev_ops_path) {
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

            let projects = raios_core::entities::load_entities(&cfg.dev_ops_path);
            #[allow(clippy::field_reassign_with_default)]
            let mut stats = raios_surface_tui::app::state::PortfolioStats::default();
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
                if raios_runtime::filebrowser::git_is_dirty(&p.local_path) == Some(true) {
                    stats.dirty += 1;
                    *cat_dirty.entry(p.category.clone()).or_insert(0) += 1;
                }
                let health = raios_runtime::health::check_project(p);
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

    fn handle_sync_done(&mut self, msg: String) {
        self.system.is_syncing = false;
        self.system.sync_status = Some(msg.clone());
        self.add_activity("System", &msg, "Info");
        let tx = self.tx.clone();
        let cfg = self.config.clone();
        thread::spawn(move || {
            tx.send(BgMsg::RecentProjects(load_recent_projects(&cfg.dev_ops_path)))
                .ok();
            tx.send(BgMsg::Agents(raios_runtime::discovery::discover_agents())).ok();
            tx.send(BgMsg::MemPalaceFiles(raios_runtime::filebrowser::get_mempalace_files(
                &cfg.dev_ops_path,
            )))
            .ok();
            tx.send(BgMsg::MemPalaceBuilt(raios_core::mempalace::build(&cfg.dev_ops_path)))
                .ok();
        });
    }

    fn handle_file_change_requested(
        &mut self,
        approval: raios_runtime::daemon::state::FileChangeApproval,
    ) {
        self.system.pending_file_changes.push(approval.clone());
        self.system.pending_change_cursor = self.system.pending_file_changes.len().saturating_sub(1);
        self.projects.git_diff_lines =
            raios_surface_tui::app::editor::simple_diff(&approval.original_content, &approval.new_content);
        self.state = AppState::GitDiffView;
        self.add_activity(
            "Agent",
            &format!("File Change Requested: {}", approval.path),
            "Warning",
        );
    }

    fn handle_mempalace_built(&mut self, rooms: Vec<raios_core::mempalace::MemRoom>) {
        let n = rooms.len();
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

    fn handle_git_action_done(
        &mut self,
        project: String,
        action: String,
        ok: bool,
        message: String,
    ) {
        let status = if ok {
            format!("✓ {} {} — {}", action, project, message)
        } else {
            format!("✗ {} {} failed: {}", action, project, message)
        };
        self.system.sync_status = Some(status.clone());
        self.add_activity("Git", &status, if ok { "Info" } else { "Warning" });
    }

    fn handle_file_changed(&mut self, path: std::path::PathBuf) {
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
                self.system.handover_count = count;
                self.system.bouncing_alert = count >= 3;
            }
        }

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

    fn handle_extensions_loaded(&mut self, exts: Vec<ExtensionInfo>) {
        self.ext.extensions = exts;
        self.ext.loaded = true;
        self.ext.ext_cursor = 0;
    }

    fn handle_ext_cmd_output(&mut self, ext: String, cmd: String, line: String) {
        self.ext.status = Some(format!(
            "[{}:{}] {}",
            ext,
            cmd,
            &line.chars().take(60).collect::<String>()
        ));
        self.timeline.logs.push(LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            sender: format!("EXT:{}", ext.to_uppercase()),
            content: line,
        });
    }
}
