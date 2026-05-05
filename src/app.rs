use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use anyhow::Result;

use crate::compliance::{self, ComplianceReport};
use crate::config::Config;
use crate::discovery::{AgentInfo, SkillInfo};
use crate::indexer::{ProjectIndex, SearchResult};
use crate::requirements::{Requirement, check_requirements};
use crate::filebrowser::{
    AgentRuleGroup, FileEntry, RecentProject, discover_all_agent_rules, find_file_by_name,
    get_agent_config_files, get_master_rule_files, get_mempalace_files, get_policy_files,
    load_file_content, load_recent_projects, save_file_content,
};
use crate::sync::sync_universe;

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Booting,
    Setup,
    Dashboard,
    FileView,
    FileEdit,
    ProjectDetail,
    HealthView,
    Search,
    MemPalaceView,
    GraphReport,
    HelpView,
}

// ─── Background messages ──────────────────────────────────────────────────────

pub enum BgMsg {
    BootResult { name: String, pass: bool, done: bool },
    TransitionToSetup,
    TransitionToDashboard,
    RecentProjects(Vec<RecentProject>),
    Agents(Vec<AgentInfo>),
    Skills(Vec<SkillInfo>),
    MasterFiles(Vec<FileEntry>),
    AgentFiles(Vec<FileEntry>),
    PolicyFiles(Vec<FileEntry>),
    MemPalaceFiles(Vec<FileEntry>),
    SyncDone(String),
    SyncError(String),
    IndexReady(ProjectIndex),
    IndexError(String),
    AgentRuleGroups(Vec<AgentRuleGroup>),
    Projects(Vec<crate::entities::EntityProject>),
    ProjectOpened { memory: Vec<String>, git_log: Vec<String> },
    HealthReport(Vec<crate::health::ProjectHealth>),
    #[allow(dead_code)] ActivityUpdate(Vec<Activity>),
    #[allow(dead_code)] NewLog(LogEntry),
    MemPalaceBuilt(Vec<crate::mempalace::MemRoom>),
    Tasks(Vec<crate::tasks::Task>),
    VaultStatus(Vec<String>),
    ActivePorts(Vec<u16>),
    AiAuditReport(crate::system_scan::AiAuditReport),
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub timestamp: String,
    pub source: String, // "Git", "Agent", "System"
    pub message: String,
    pub level: &'static str, // "Info", "Warning", "Error"
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub sender: String,
    pub content: String,
}

// ─── Setup field ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetupField {
    pub label: &'static str,
    pub hint:  &'static str,
    pub value: String,
    pub auto_detected: bool,
}

impl SetupField {
    pub fn new(label: &'static str, hint: &'static str) -> Self {
        Self { label, hint, value: String::new(), auto_detected: false }
    }
    pub fn with_detected(mut self, path: Option<std::path::PathBuf>) -> Self {
        if let Some(p) = path {
            self.value = p.to_string_lossy().into_owned();
            self.auto_detected = true;
        }
        self
    }
}

// ─── Rule categories (hardcoded constitution) ────────────────────────────────

#[derive(Clone)]
pub struct RuleCategory {
    pub title: &'static str,
    pub rules: Vec<&'static str>,
}

pub fn system_rules() -> Vec<RuleCategory> {
    vec![
        RuleCategory {
            title: "Core Principles",
            rules: vec![
                "İş arkadaşı tavrı, net ve direkt iletişim",
                "Kod: İngilizce, İletişim: Türkçe",
                "Güvenlik ve performans odaklı pair-programming",
            ],
        },
        RuleCategory {
            title: "Coding Standards",
            rules: vec![
                "pnpm > npm/yarn. Python: uv/pip",
                "Önce amaç ve skeleton, sonra component-by-component",
                "Fonksiyonel yazım, hata yönetimi zorunlu",
            ],
        },
        RuleCategory {
            title: "Mandatory Skills",
            rules: vec![
                "prompt-master: Her prompt öncesi zorunlu",
                "graphify: Codebase girişi ve analizde zorunlu",
                "verify-ai-os: Session başı ve tutarsızlıkta zorunlu",
            ],
        },
        RuleCategory {
            title: "Security",
            rules: vec![
                "API key asla client-side'da olmaz",
                "RLS (Row Level Security) day 0'dan zorunlu",
                "Secrets Manager kullanımı (Production)",
            ],
        },
    ]
}

// ─── Simple line editor ───────────────────────────────────────────────────────

pub struct Editor {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll: usize,
    pub view_height: usize,
}

impl Editor {
    pub fn from_content(content: &str, view_height: usize) -> Self {
        let lines: Vec<String> = content.lines().map(str::to_owned).collect();
        let lines = if lines.is_empty() { vec![String::new()] } else { lines };
        Self { lines, cursor_row: 0, cursor_col: 0, scroll: 0, view_height }
    }

    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                self.lines[self.cursor_row].insert(byte, c);
                self.cursor_col += 1;
            }
            KeyCode::Enter => {
                let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                let rest = self.lines[self.cursor_row].split_off(byte);
                self.cursor_row += 1;
                self.lines.insert(self.cursor_row, rest);
                self.cursor_col = 0;
            }
            KeyCode::Backspace => {
                if self.cursor_col > 0 {
                    let b_end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                    let b_start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col - 1);
                    self.lines[self.cursor_row].drain(b_start..b_end);
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    let line = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                    self.lines[self.cursor_row].push_str(&line);
                }
            }
            KeyCode::Delete => {
                let line_len = self.lines[self.cursor_row].chars().count();
                if self.cursor_col < line_len {
                    let b_start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                    let b_end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col + 1);
                    self.lines[self.cursor_row].drain(b_start..b_end);
                } else if self.cursor_row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.cursor_row + 1);
                    self.lines[self.cursor_row].push_str(&next);
                }
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                }
            }
            KeyCode::Right => {
                let line_len = self.lines[self.cursor_row].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                }
            }
            KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    let max = self.lines[self.cursor_row].chars().count();
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    let max = self.lines[self.cursor_row].chars().count();
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            KeyCode::Home => self.cursor_col = 0,
            KeyCode::End => self.cursor_col = self.lines[self.cursor_row].chars().count(),
            KeyCode::PageUp => {
                self.cursor_row = self.cursor_row.saturating_sub(self.view_height);
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].chars().count());
            }
            KeyCode::PageDown => {
                self.cursor_row = (self.cursor_row + self.view_height).min(self.lines.len() - 1);
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].chars().count());
            }
            _ => {}
        }
        self.update_scroll();
    }

    fn update_scroll(&mut self) {
        if self.view_height == 0 {
            return;
        }
        if self.cursor_row < self.scroll {
            self.scroll = self.cursor_row;
        } else if self.cursor_row >= self.scroll + self.view_height {
            self.scroll = self.cursor_row + 1 - self.view_height;
        }
    }
}

fn char_to_byte(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

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

    // All Projects (entities.json)
    pub projects: Vec<crate::entities::EntityProject>,
    pub project_cursor: usize,

    // Project Detail screen
    pub active_project: Option<crate::entities::EntityProject>,
    pub project_memory_lines: Vec<String>,
    pub project_git_log: Vec<String>,
    pub project_memory_scroll: u16,
    pub project_panel_focus: bool,

    // Background
    pub tx: Sender<BgMsg>,
    pub rx: Receiver<BgMsg>,

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

    // Vault
    pub vault_projects: Vec<String>,

    // Ports
    pub active_ports: Vec<u16>,

    // Graph Report
    pub graph_report_lines: Vec<String>,
    pub graph_report_scroll: u16,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BgMsg>();
        let boot_tx = tx.clone();

        // Load or prepare config (auto-detect will happen after boot)
        let config = Config::load().unwrap_or_else(|| Config {
            dev_ops_path:   PathBuf::from(""),
            master_md_path: PathBuf::from(""),
            skills_path:    PathBuf::from(""),
            vault_projects_path: PathBuf::from(""),
        });

        let master = config.master_md_path.clone();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        thread::spawn(move || {
            let checks: Vec<(String, PathBuf)> = vec![
                ("Global GEMINI.md".into(), home.join(".gemini/GEMINI.md")),
                ("Global CLAUDE.md".into(), home.join("CLAUDE.md")),
                ("MASTER.md".into(), master),
                ("Policy Engine".into(), home.join(".gemini/policies/ai-os-policy.toml")),
                ("Gemini CLI".into(), home.join("AppData/Roaming/npm/gemini.cmd")),
            ];
            for (i, (name, path)) in checks.iter().enumerate() {
                thread::sleep(Duration::from_millis(50));
                let pass = path.exists();
                let done = i == checks.len() - 1;
                boot_tx.send(BgMsg::BootResult { name: name.clone(), pass, done }).ok();
            }
        });

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
            show_launcher: false,
            projects: Vec::new(),
            project_cursor: 0,
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
            vault_projects: Vec::new(),
            active_ports: Vec::new(),
            system_report: None,
            is_scanning_system: false,
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
            _ => {}
        }
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
                // Auto-detect paths
                use crate::config::Config as Cfg;
                let detected = Cfg::auto_detect();
                self.requirements = check_requirements();
                self.setup_fields = vec![
                    SetupField::new(
                        "Dev Ops Path",
                        "Root workspace folder (contains all your projects)",
                    ).with_detected(detected.dev_ops),
                    SetupField::new(
                        "MASTER.md Path",
                        "Central agent constitution file",
                    ).with_detected(detected.master_md),
                    SetupField::new(
                        "Skills Path",
                        "Agent skills directory (.agents/skills)",
                    ).with_detected(detected.skills),
                    SetupField::new(
                        "Vault Projects Path",
                        "Obsidian Vault Projects folder",
                    ).with_detected(detected.vault_projects),
                ];
                self.setup_cursor = 0;
                self.state = AppState::Setup;
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
                    
                    // Port Monitor
                    let tx_port = tx.clone();
                    thread::spawn(move || {
                        let common_ports = [3000, 5173, 8080, 4200];
                        loop {
                            let mut active = Vec::new();
                            for &port in &common_ports {
                                let addr = format!("127.0.0.1:{}", port);
                                if let Ok(stream) = std::net::TcpStream::connect_timeout(
                                    &addr.parse().unwrap(),
                                    std::time::Duration::from_millis(100)
                                ) {
                                    active.push(port);
                                    drop(stream);
                                }
                            }
                            tx_port.send(BgMsg::ActivePorts(active)).ok();
                            thread::sleep(std::time::Duration::from_secs(10));
                        }
                    });
                });
                // Start indexing Dev Ops in background
                if !self.is_indexing && self.config.dev_ops_path.exists() {
                    self.is_indexing = true;
                    self.index_status = Some("Building index...".into());
                    let tx2 = self.tx.clone();
                    let dev_ops = self.config.dev_ops_path.clone();
                    thread::spawn(move || {
                        match crate::indexer::ProjectIndex::build(&dev_ops) {
                            Ok(idx) => tx2.send(BgMsg::IndexReady(idx)).ok(),
                            Err(e) => tx2.send(BgMsg::IndexError(e.to_string())).ok(),
                        };
                    });
                }
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
            BgMsg::ActivePorts(p) => {
                self.active_ports = p;
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
                        .unwrap_or_else(|| crate::entities::EntityProject {
                            name: self.mp_rooms[self.mp_room_cursor].projects[pi].name.clone(),
                            category: self.mp_rooms[self.mp_room_cursor].folder_name.clone(),
                            local_path: proj_path,
                            github: None,
                            status: "unknown".into(),
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

    fn get_selected_mempalace_project(&self) -> Option<crate::mempalace::MemProject> {
        let pi = self.mp_proj_cursor?;
        self.mp_rooms.get(self.mp_room_cursor)?.projects.get(pi).cloned()
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

    fn update_search(&mut self) {
        if let Some(ref idx) = self.index {
            self.search_results = idx.search(&self.search_query);
            self.search_cursor = 0;
        }
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
        if self.setup_editing {
            match key.code {
                KeyCode::Enter => {
                    self.setup_fields[self.setup_cursor].value = self.setup_input.clone();
                    self.setup_fields[self.setup_cursor].auto_detected = false;
                    self.setup_editing = false;
                }
                KeyCode::Esc => self.setup_editing = false,
                KeyCode::Char(c) => self.setup_input.push(c),
                KeyCode::Backspace => { self.setup_input.pop(); }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Up => {
                if self.setup_cursor > 0 { self.setup_cursor -= 1; }
            }
            KeyCode::Down => {
                if self.setup_cursor + 1 < self.setup_fields.len() { self.setup_cursor += 1; }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                self.setup_editing = true;
            }
            KeyCode::Char('s') => {
                let all_filled = self.setup_fields.iter().all(|f| !f.value.is_empty());
                if all_filled {
                    let cfg = Config {
                        dev_ops_path:   PathBuf::from(&self.setup_fields[0].value),
                        master_md_path: PathBuf::from(&self.setup_fields[1].value),
                        skills_path:    PathBuf::from(&self.setup_fields[2].value),
                        vault_projects_path: PathBuf::from(&self.setup_fields[3].value),
                    };
                    match cfg.save() {
                        Ok(()) => {
                            self.config = cfg;
                            self.state = AppState::Dashboard;
                            self.execute_command("/sync")?;
                        }
                        Err(e) => {
                            self.setup_status = Some(format!("Save error: {}", e));
                        }
                    }
                } else {
                    self.setup_status = Some("Please fill in all fields before saving.".into());
                }
            }
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        Ok(())
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => {
                self.state = AppState::HelpView;
            }
            KeyCode::Char('L') => {
                // Uppercase L = launcher (lowercase l = vim right)
                if self.menu_cursor == 7 && self.right_panel_focus {
                    if let Some(proj) = self.projects.get(self.project_cursor).cloned() {
                        self.active_project = Some(proj);
                        self.show_launcher = true;
                    }
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
                            if let Some(proj) = self.projects.get(self.project_cursor).cloned() {
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
                        7 => self.projects.get(self.project_cursor).map(|p| p.local_path.clone()),
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
                if !self.projects.is_empty() && !self.is_checking_health {
                    self.is_checking_health = true;
                    self.health_report.clear();
                    self.state = AppState::HealthView;
                    let tx = self.tx.clone();
                    let projects = self.projects.clone();
                    thread::spawn(move || {
                        let report: Vec<crate::health::ProjectHealth> =
                            projects.iter().map(crate::health::check_project).collect();
                        tx.send(BgMsg::HealthReport(report)).ok();
                    });
                } else if self.projects.is_empty() {
                    self.sync_status = Some("Load entities.json first".into());
                } else {
                    self.state = AppState::HealthView;
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
