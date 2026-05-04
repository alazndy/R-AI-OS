use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use anyhow::Result;

use crate::discovery::{AgentInfo, SkillInfo};
use crate::filebrowser::{
    FileEntry, RecentProject, find_file_by_name, get_agent_config_files,
    get_master_rule_files, get_mempalace_files, get_policy_files, load_file_content,
    load_recent_projects, save_file_content,
};
use crate::sync::sync_universe;

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Booting,
    Dashboard,
    FileView,
    FileEdit,
}

// ─── Background messages ──────────────────────────────────────────────────────

pub enum BgMsg {
    BootResult { name: String, pass: bool, done: bool },
    TransitionToDashboard,
    RecentProjects(Vec<RecentProject>),
    Agents(Vec<AgentInfo>),
    Skills(Vec<SkillInfo>),
    SyncDone(String),
    SyncError(String),
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

pub const MENU_ITEMS: &[&str] =
    &["Recent", "System Rules", "System Core", "Agents & Tools", "Policies", "MemPalace"];

pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub tick: u64,

    // Boot
    pub boot_results: Vec<(String, bool)>,

    // Dashboard
    pub menu_cursor: usize,
    pub right_panel_focus: bool,
    pub right_file_cursor: usize,

    // Command input
    pub command_mode: bool,
    pub command_buf: String,

    // Content
    pub recent_projects: Vec<RecentProject>,
    pub system_rules: Vec<RuleCategory>,
    pub agents: Vec<AgentInfo>,
    pub skills: Vec<SkillInfo>,
    pub sync_status: Option<String>,
    pub is_syncing: bool,

    // File view
    pub active_file: Option<FileEntry>,
    pub file_lines: Vec<String>,
    pub file_scroll: u16,
    pub edit_save_msg: Option<String>,

    // Editor
    pub editor: Editor,

    // Background
    pub tx: Sender<BgMsg>,
    pub rx: Receiver<BgMsg>,

    pub width: u16,
    pub height: u16,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BgMsg>();
        let boot_tx = tx.clone();

        thread::spawn(move || {
            let checks: &[(&str, &str)] = &[
                ("Global GEMINI.md", r"C:\Users\turha\.gemini\GEMINI.md"),
                ("Global CLAUDE.md", r"C:\Users\turha\CLAUDE.md"),
                ("MASTER.md (Vault)", r"C:\Users\turha\Documents\Obsidian Vaults\Vault101\MASTER.md"),
                ("Policy Engine", r"C:\Users\turha\.gemini\policies\ai-os-policy.toml"),
                ("Gemini CLI", r"C:\Users\turha\AppData\Roaming\npm\gemini.cmd"),
            ];
            for (i, (name, path)) in checks.iter().enumerate() {
                thread::sleep(Duration::from_millis(350));
                let pass = std::path::Path::new(path).exists();
                let done = i == checks.len() - 1;
                boot_tx.send(BgMsg::BootResult {
                    name: name.to_string(),
                    pass,
                    done,
                }).ok();
            }
        });

        Self {
            state: AppState::Booting,
            should_quit: false,
            tick: 0,
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
        }
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn current_menu_files(&self) -> Vec<FileEntry> {
        match self.menu_cursor {
            1 => get_master_rule_files(),
            3 => get_agent_config_files(),
            4 => get_policy_files(),
            5 => get_mempalace_files(),
            _ => vec![],
        }
    }

    pub fn open_file_view(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        self.file_lines = content.lines().map(str::to_owned).collect();
        self.file_scroll = 0;
        self.active_file = Some(entry);
        self.edit_save_msg = None;
        self.state = AppState::FileView;
    }

    pub fn open_file_edit(&mut self, entry: FileEntry) {
        let content = load_file_content(&entry.path);
        let view_h = self.height.saturating_sub(8) as usize;
        self.editor = Editor::from_content(&content, view_h.max(5));
        self.active_file = Some(entry);
        self.edit_save_msg = None;
        self.state = AppState::FileEdit;
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

    pub fn handle_bg_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::BootResult { name, pass, done } => {
                self.boot_results.push((name, pass));
                if done {
                    let tx = self.tx.clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(700));
                        tx.send(BgMsg::TransitionToDashboard).ok();
                    });
                }
            }
            BgMsg::TransitionToDashboard => {
                self.state = AppState::Dashboard;
                let tx = self.tx.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects())).ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents())).ok();
                    tx.send(BgMsg::Skills(crate::discovery::discover_skills())).ok();
                });
            }
            BgMsg::RecentProjects(p) => self.recent_projects = p,
            BgMsg::Agents(a) => self.agents = a,
            BgMsg::Skills(s) => self.skills = s,
            BgMsg::SyncDone(msg) => {
                self.is_syncing = false;
                self.sync_status = Some(msg);
                let tx = self.tx.clone();
                thread::spawn(move || {
                    tx.send(BgMsg::RecentProjects(load_recent_projects())).ok();
                    tx.send(BgMsg::Agents(crate::discovery::discover_agents())).ok();
                });
            }
            BgMsg::SyncError(e) => {
                self.is_syncing = false;
                self.sync_status = Some(format!("Error: {}", e));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        match self.state {
            AppState::Booting => {
                if key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                }
            }

            AppState::FileView => self.handle_file_view_key(key),

            AppState::FileEdit => self.handle_file_edit_key(key)?,

            AppState::Dashboard => {
                if self.command_mode {
                    self.handle_command_key(key)?;
                } else {
                    self.handle_dashboard_key(key)?;
                }
            }
        }
        Ok(())
    }

    fn handle_file_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                self.state = AppState::Dashboard;
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
            }
            KeyCode::Enter => {
                let cmd = self.command_buf.clone();
                self.command_buf.clear();
                self.command_mode = false;
                self.execute_command(&cmd)?;
            }
            KeyCode::Backspace => {
                if self.command_buf.is_empty() {
                    self.command_mode = false;
                } else {
                    self.command_buf.pop();
                }
            }
            KeyCode::Char(c) => {
                self.command_buf.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('/') | KeyCode::Tab => {
                self.command_mode = true;
                if key.code == KeyCode::Char('/') {
                    self.command_buf = "/".into();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.right_panel_focus {
                    if self.right_file_cursor > 0 {
                        self.right_file_cursor -= 1;
                    }
                } else if self.menu_cursor > 0 {
                    self.menu_cursor -= 1;
                    self.right_file_cursor = 0;
                    self.right_panel_focus = false;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.right_panel_focus {
                    let max = self.current_menu_files().len().saturating_sub(1);
                    if self.right_file_cursor < max {
                        self.right_file_cursor += 1;
                    }
                } else {
                    if self.menu_cursor < MENU_ITEMS.len() - 1 {
                        self.menu_cursor += 1;
                        self.right_file_cursor = 0;
                        self.right_panel_focus = false;
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if !self.current_menu_files().is_empty() {
                    self.right_panel_focus = true;
                    self.right_file_cursor = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.right_panel_focus = false;
            }
            KeyCode::Enter => {
                if self.right_panel_focus {
                    let files = self.current_menu_files();
                    if let Some(entry) = files.into_iter().nth(self.right_file_cursor) {
                        self.open_file_view(entry);
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
                let tx = self.tx.clone();
                thread::spawn(move || match sync_universe() {
                    Ok(msg) => tx.send(BgMsg::SyncDone(msg)).ok(),
                    Err(e) => tx.send(BgMsg::SyncError(e.to_string())).ok(),
                });
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
            "/mempalace" => {
                let files = get_mempalace_files();
                if let Some(f) = files.into_iter().next() {
                    self.open_file_view(f);
                }
            }
            "/view" | "/open" => {
                if !arg.is_empty() {
                    if let Some(entry) = find_file_by_name(arg) {
                        self.open_file_view(entry);
                    }
                }
            }
            "/edit" => {
                if !arg.is_empty() {
                    if let Some(entry) = find_file_by_name(arg) {
                        if !entry.read_only {
                            self.open_file_edit(entry);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
