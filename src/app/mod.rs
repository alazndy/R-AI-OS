use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::SystemTime;


use crate::compliance::ComplianceReport;
use crate::config::Config;
use crate::discovery::{AgentInfo, SkillInfo};
use crate::indexer::{ProjectIndex, SearchResult};
use crate::requirements::Requirement;
use crate::filebrowser::{
    AgentRuleGroup, FileEntry, RecentProject,
    load_file_content, load_recent_projects,
};
#[allow(unused_imports)] use crate::safe_io;
#[allow(unused_imports)] use notify::{Watcher, RecursiveMode, Config as WatchConfig};

pub mod state;
pub use state::*;

pub mod editor;
pub use editor::*;

pub mod ipc;
pub use ipc::*;

pub mod events;

// ─── App ──────────────────────────────────────────────────────────────────────

// ─── Command palette ─────────────────────────────────────────────────────────

pub struct PaletteItem {
    pub cmd: &'static str,
    pub desc: &'static str,
}

pub const PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem { cmd: "/discover", desc: "Scan Dev Ops for new projects & update entities.json" },
    PaletteItem { cmd: "/sync",     desc: "Sync all agents with MASTER.md" },
    PaletteItem { cmd: "/search",   desc: "Neural search: /search <query>" },
    PaletteItem { cmd: "/open",     desc: "Open project: /open <name>" },
    PaletteItem { cmd: "/view",     desc: "View file: /view <filename>" },
    PaletteItem { cmd: "/edit",     desc: "Edit file: /edit <filename>" },
    PaletteItem { cmd: "/memo",     desc: "Quick note: /memo <text>" },
    PaletteItem { cmd: "/health",   desc: "Open Health Dashboard" },
    PaletteItem { cmd: "/rules",    desc: "Go to System Rules" },
    PaletteItem { cmd: "/memory",   desc: "Go to MemPalace" },
    PaletteItem { cmd: "/graphify", desc: "Run Graphify (Knowledge Graph) on project" },
    PaletteItem { cmd: "/reindex",  desc: "Rebuild Neural Search index" },
    PaletteItem { cmd: "/quit",     desc: "Exit R-AI-OS" },
];

pub fn filtered_palette(query: &str) -> Vec<&'static PaletteItem> {
    let q = query.trim_start_matches('/').to_lowercase();
    PALETTE_ITEMS
        .iter()
        .filter(|p| q.is_empty() || p.cmd.contains(q.as_str()) || p.desc.to_lowercase().contains(q.as_str()))
        .collect()
}

pub const MENU_ITEMS: &[&str] = &[
    "Recent",
    "System Rules",
    "System Core",
    "Agents & Tools",
    "Policies",
    "MemPalace",
    "Neural Search",
    "All Projects",
    "Timeline",
    "Live Logs",
    "Help",
    "AI System Audit",
];

pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub tick: u64,

    // Config
    pub config: Config,

    // Setup screen
    pub setup_fields: Vec<SetupField>,
    pub setup_cursor: usize,
    pub setup_editing: bool,
    pub setup_input: String,
    pub setup_status: Option<String>,
    pub requirements: Vec<Requirement>,

    // Search
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub search_cursor: usize,
    pub index: Option<ProjectIndex>,
    pub is_indexing: bool,
    pub index_status: Option<String>,

    // Boot
    pub boot_results: Vec<(String, bool)>,

    // Dashboard
    pub menu_cursor: usize,
    pub right_panel_focus: bool,
    pub right_file_cursor: usize,
    pub right_panel_scroll: usize,

    // Command input
    pub command_mode: bool,
    pub command_buf: String,

    // Content
    pub recent_projects: Vec<RecentProject>,
    pub system_rules: Vec<RuleCategory>,
    pub agents: Vec<AgentInfo>,
    pub skills: Vec<SkillInfo>,
    pub master_files: Vec<FileEntry>,
    pub agent_files: Vec<FileEntry>,
    pub policy_files: Vec<FileEntry>,
    pub mempalace_files: Vec<FileEntry>,
    pub sync_status: Option<String>,
    pub is_syncing: bool,

    // File view
    pub active_file: Option<FileEntry>,
    pub file_lines: Vec<String>,
    pub file_scroll: u16,
    pub edit_save_msg: Option<String>,

    // Editor
    pub editor: Editor,

    // Agent rule groups (dynamically discovered)
    pub agent_rule_groups: Vec<AgentRuleGroup>,

    // Health Dashboard
    pub health_report: Vec<crate::health::ProjectHealth>,
    pub health_cursor: usize,
    pub is_checking_health: bool,

    // Command palette
    pub palette_cursor: usize,

    // File watcher
    pub watched_file_mtime: Option<SystemTime>,
    pub file_changed_externally: bool,

    // Agent launcher overlay
    pub show_launcher: bool,
    pub launcher_cursor: usize,
    pub launcher_input: String,

    // All Projects (entities.json)
    pub projects: Vec<crate::entities::EntityProject>,
    pub project_cursor: usize,
    pub project_sort: SortMode,

    // Project Detail screen
    pub active_project: Option<crate::entities::EntityProject>,
    pub project_memory_lines: Vec<String>,
    pub project_git_log: Vec<String>,
    pub project_memory_scroll: u16,
    pub project_panel_focus: bool,

    // Background
    pub tx: Sender<BgMsg>,
    pub rx: Receiver<BgMsg>,
    pub tx_daemon: Option<Sender<String>>,

    pub width: u16,
    pub height: u16,

    // Compliance & Auto-Fix
    pub compliance: Option<ComplianceReport>,
    pub is_fixing: bool,
    pub fix_status: Option<String>,

    // Timeline & Logs
    pub activities: Vec<Activity>,
    pub logs: Vec<LogEntry>,

    // Memory file watcher — all known memory.md mtimes
    pub memory_watch: HashMap<PathBuf, SystemTime>,
    pub memory_refresh_pending: bool,

    // Graphify
    pub graphify_script: Option<PathBuf>,

    // MemPalace
    pub mp_rooms: Vec<crate::mempalace::MemRoom>,
    pub mp_room_cursor: usize,
    pub mp_proj_cursor: Option<usize>,
    pub mp_expanded: Vec<bool>,
    pub mp_filter: String,
    pub mp_is_building: bool,
    
    // System Scan
    pub system_report: Option<crate::system_scan::AiAuditReport>,
    pub is_scanning_system: bool,
    
    // Tasks
    pub tasks: Vec<crate::tasks::Task>,
    pub task_cursor: usize,

    // File Watcher
    pub _watcher: Option<Box<dyn Watcher>>,

    // Vault
    pub vault_projects: Vec<String>,

    // Ports
    pub active_ports: Vec<u16>,

    // Graph Report
    pub graph_report_lines: Vec<String>,
    pub graph_report_scroll: u16,

    // Bouncing Limit
    pub handover_count: usize,
    pub bouncing_alert: bool,
    pub handover_modal: Option<(String, String)>,

    // Git Diff View
    pub git_diff_lines: Vec<String>,
    pub git_diff_scroll: u16,

    // Active Agents
    pub active_agents: Vec<crate::daemon::proxy::AgentProcess>,
    pub selected_agent_idx: usize,

    // File Change Approvals (Inbox)
    pub pending_file_changes: Vec<crate::daemon::state::FileChangeApproval>,
    pub pending_change_cursor: usize,

    // Portfolio stats cache (computed in background on dashboard load)
    pub stats_cache: Option<PortfolioStats>,
    pub is_computing_stats: bool,

    // Setup Wizard
    pub wizard_step: WizardStep,
    pub wizard_dev_ops: String,
    pub wizard_master: String,
    pub wizard_github: String,
    pub wizard_vault: String,
    pub wizard_field_cursor: usize,  // which input field is active in current step
    pub wizard_editing: bool,
    pub wizard_input: String,
    pub wizard_agent_status: Option<crate::setup_wizard::AgentStatus>,
    pub wizard_action_log: Vec<crate::setup_wizard::WizardAction>,
    pub wizard_skip_claude: bool,
    pub wizard_skip_gemini: bool,
    pub wizard_skip_antigravity: bool,
    pub wizard_running: bool,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BgMsg>();
        let boot_tx = tx.clone();
        
        let config = Config::load().unwrap_or_else(|| Config {
            dev_ops_path:   PathBuf::from(""),
            master_md_path: PathBuf::from(""),
            skills_path:    PathBuf::from(""),
            vault_projects_path: PathBuf::from(""),
        });

        // Boot results (minimal check for starting)
        let home = dirs::home_dir().unwrap_or_default();
        let master = config.master_md_path.clone();
        thread::spawn(move || {
            let checks: Vec<(String, PathBuf)> = vec![
                ("MASTER.md".into(), master),
                ("Global Config".into(), home.join(".gemini/GEMINI.md")),
            ];
            for (i, (name, path)) in checks.iter().enumerate() {
                let pass = path.exists();
                let done = i == checks.len() - 1;
                boot_tx.send(BgMsg::BootResult { name: name.clone(), pass, done }).ok();
            }
        });

        // --- Connect to aiosd Daemon ---
        let tx_daemon = ipc::connect_daemon(tx.clone());

        Self {
            state: AppState::Booting,
            should_quit: false,
            tick: 0,
            config,
            setup_fields: vec![],
            setup_cursor: 0,
            setup_editing: false,
            setup_input: String::new(),
            setup_status: None,
            requirements: Vec::new(),
            show_launcher: false,
            launcher_cursor: 0,
            launcher_input: String::new(),
            handover_modal: None,
            handover_count: 0,
            tx_daemon,
            active_ports: Vec::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_cursor: 0,
            index: None,
            is_indexing: false,
            index_status: None,
            boot_results: Vec::new(),
            menu_cursor: 0,
            right_panel_focus: false,
            right_file_cursor: 0,
            command_mode: false,
            command_buf: String::new(),
            recent_projects: Vec::new(),
            system_rules: system_rules(),
            agents: Vec::new(),
            skills: Vec::new(),
            master_files: Vec::new(),
            agent_files: Vec::new(),
            policy_files: Vec::new(),
            mempalace_files: Vec::new(),
            sync_status: None,
            is_syncing: false,
            active_file: None,
            file_lines: Vec::new(),
            file_scroll: 0,
            edit_save_msg: None,
            editor: Editor::from_content("", 20),
            tx,
            rx,
            width: 80,
            height: 24,
            agent_rule_groups: Vec::new(),
            health_report: Vec::new(),
            health_cursor: 0,
            is_checking_health: false,
            palette_cursor: 0,
            watched_file_mtime: None,
            file_changed_externally: false,
            projects: Vec::new(),
            project_cursor: 0,
            project_sort: SortMode::default(),
            active_project: None,
            project_memory_lines: Vec::new(),
            project_git_log: Vec::new(),
            project_memory_scroll: 0,
            project_panel_focus: false,
            compliance: None,
            is_fixing: false,
            fix_status: None,
            activities: Vec::new(),
            logs: Vec::new(),
            memory_watch: HashMap::new(),
            memory_refresh_pending: false,
            graphify_script: None,
            mp_rooms: Vec::new(),
            mp_room_cursor: 0,
            mp_proj_cursor: None,
            mp_expanded: Vec::new(),
            mp_filter: String::new(),
            mp_is_building: false,
            graph_report_lines: Vec::new(),
            graph_report_scroll: 0,
            right_panel_scroll: 0,
            tasks: Vec::new(),
            task_cursor: 0,
            _watcher: None,
            vault_projects: Vec::new(),
            system_report: None,
            is_scanning_system: false,
            bouncing_alert: false,
            git_diff_lines: Vec::new(),
            git_diff_scroll: 0,
            active_agents: Vec::new(),
            selected_agent_idx: 0,
            pending_file_changes: Vec::new(),
            pending_change_cursor: 0,
            stats_cache: None,
            is_computing_stats: false,
            wizard_step: WizardStep::default(),
            wizard_dev_ops: String::new(),
            wizard_master: String::new(),
            wizard_github: String::new(),
            wizard_vault: String::new(),
            wizard_field_cursor: 0,
            wizard_editing: false,
            wizard_input: String::new(),
            wizard_agent_status: None,
            wizard_action_log: Vec::new(),
            wizard_skip_claude: false,
            wizard_skip_gemini: false,
            wizard_skip_antigravity: false,
            wizard_running: false,
        }
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        // Active-file watcher: every ~1 second
        if self.tick % 25 == 0 && matches!(self.state, AppState::FileView | AppState::FileEdit) {
            if let Some(ref file) = self.active_file {
                let current = std::fs::metadata(&file.path).ok().and_then(|m| m.modified().ok());
                if let (Some(watched), Some(cur)) = (self.watched_file_mtime, current) {
                    if cur != watched {
                        self.file_changed_externally = true;
                    }
                }
            }
        }

        // Memory-file watcher: every ~2 seconds (50 ticks @ 40ms)
        if self.tick % 50 == 0 && !self.memory_watch.is_empty() {
            self.check_memory_files();
        }
    }

    fn check_memory_files(&mut self) {
        let mut changed_paths: Vec<PathBuf> = Vec::new();

        for (path, old_mtime) in &mut self.memory_watch {
            let new_mtime = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok());
            if let Some(new) = new_mtime {
                if new != *old_mtime {
                    *old_mtime = new;
                    changed_paths.push(path.clone());
                }
            }
        }

        if changed_paths.is_empty() {
            return;
        }

        // If the currently open project's memory.md changed, reload it in-place
        if let Some(ref proj) = self.active_project {
            let proj_mem = proj.local_path.join("memory.md");
            if changed_paths.contains(&proj_mem) {
                let content = load_file_content(&proj_mem);
                self.project_memory_lines = content.lines().map(str::to_owned).collect();
            }
        }

        // Kick off background refresh
        self.memory_refresh_pending = true;
        let tx = self.tx.clone();
        let dev_ops = self.config.dev_ops_path.clone();
        thread::spawn(move || {
            tx.send(BgMsg::RecentProjects(load_recent_projects(&dev_ops))).ok();
            tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&dev_ops))).ok();
        });
    }

    pub fn current_menu_files(&self) -> Vec<FileEntry> {
        match self.menu_cursor {
            1 => self.master_files.clone(),
            3 => self.agent_files.clone(),
            4 => self.policy_files.clone(),
            5 => self.mempalace_files.clone(),
            _ => vec![],
        }
    }







    pub fn sorted_project_indices(&self) -> Vec<usize> {
        use crate::filebrowser::git_is_dirty;
        let mut indices: Vec<usize> = (0..self.projects.len()).collect();
        match self.project_sort {
            SortMode::Name => indices.sort_by(|&a, &b| {
                self.projects[a].name.to_lowercase().cmp(&self.projects[b].name.to_lowercase())
            }),
            SortMode::Grade => indices.sort_by(|&a, &b| {
                let ha = crate::health::check_project(&self.projects[a]);
                let hb = crate::health::check_project(&self.projects[b]);
                ha.compliance_grade.cmp(&hb.compliance_grade)
            }),
            SortMode::GitDirty => indices.sort_by(|&a, &b| {
                let da = git_is_dirty(&self.projects[a].local_path).unwrap_or(false);
                let db = git_is_dirty(&self.projects[b].local_path).unwrap_or(false);
                db.cmp(&da)
            }),
            SortMode::Category => indices.sort_by(|&a, &b| {
                self.projects[a].category.cmp(&self.projects[b].category)
            }),
            SortMode::Status => indices.sort_by(|&a, &b| {
                self.projects[a].status.cmp(&self.projects[b].status)
            }),
        }
        indices
    }

    pub fn project_at_cursor(&self) -> Option<&crate::entities::EntityProject> {
        let indices = self.sorted_project_indices();
        indices.get(self.project_cursor).and_then(|&i| self.projects.get(i))
    }

    fn find_project_path_by_name(&self, name: &str) -> Option<PathBuf> {
        let q = name.to_lowercase();
        self.projects
            .iter()
            .find(|p| p.name.to_lowercase() == q || p.name.to_lowercase().contains(&q))
            .map(|p| p.local_path.clone())
    }

    pub fn dispatch_task(&mut self, agent: &str) {
        let task = match self.tasks.get(self.task_cursor) {
            Some(t) => t.clone(),
            None => return,
        };

        let proj_path = task
            .project
            .as_deref()
            .and_then(|name| self.find_project_path_by_name(name))
            .or_else(|| self.active_project.as_ref().map(|p| p.local_path.clone()));

        let result = crate::tasks::dispatch_to_agent(&task, agent, proj_path.as_ref());
        self.sync_status = Some(result);
        self.add_activity("Task", &format!("Dispatched to {}: {}", agent, task.text), "Info");
    }





    pub fn add_activity(&mut self, source: &str, message: &str, level: &'static str) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.activities.push(Activity {
            timestamp: now,
            source: source.to_string(),
            message: message.to_string(),
            level,
        });
        if self.activities.len() > 100 {
            self.activities.remove(0);
        }
    }




    fn get_selected_mempalace_project(&self) -> Option<crate::mempalace::MemProject> {
        let pi = self.mp_proj_cursor?;
        self.mp_rooms.get(self.mp_room_cursor)?.projects.get(pi).cloned()
    }


    fn update_search(&mut self) {
        if let Some(ref tx) = self.tx_daemon {
            let cmd = format!("{{\"command\":\"Search\",\"query\":\"{}\"}}", self.search_query);
            let _ = tx.send(cmd);
        }
    }







    pub fn run_graphify(&self, project_path: &Path) -> String {
        let script = match &self.graphify_script {
            Some(s) => s,
            None => return "Graphify script not found in Dev Ops/AI OS/graphify".into(),
        };

        let path_str = project_path.to_string_lossy().into_owned();
        let script_str = script.to_string_lossy().into_owned();

        // Command: python graphify.py <project_path>
        let cmd_args = format!("python \"{}\" \"{}\"", script_str, path_str);

        if std::process::Command::new("wt")
            .args(["-d", &path_str, "cmd", "/k", &cmd_args])
            .spawn()
            .is_ok()
        {
            format!("Graphify started in Windows Terminal")
        } else {
            match std::process::Command::new("cmd")
                .args(["/c", "start", "cmd", "/k", &cmd_args])
                .spawn()
            {
                Ok(_) => format!("Graphify started"),
                Err(e) => format!("Graphify launch error: {}", e),
            }
        }
    }
}


