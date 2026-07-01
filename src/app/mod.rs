use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::config::Config;
use crate::filebrowser::{load_file_content, load_recent_projects, FileEntry};
#[allow(unused_imports)]
use crate::safe_io;
#[allow(unused_imports)]
use notify::{Config as WatchConfig, RecursiveMode, Watcher};

pub mod state;
pub use state::*;

pub mod editor;
pub use editor::*;

pub mod ipc;
pub use ipc::*;

pub mod ipc_support;
pub mod ipc_events;

pub mod services;
pub use services::*;

pub mod events;

// ─── App ──────────────────────────────────────────────────────────────────────

// ─── Command palette ─────────────────────────────────────────────────────────

pub struct PaletteItem {
    pub cmd: &'static str,
    pub desc: &'static str,
}

pub const PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        cmd: "/discover",
        desc: "Scan Dev Ops for new projects & update entities.json",
    },
    PaletteItem {
        cmd: "/sync",
        desc: "Sync all agents with MASTER.md",
    },
    PaletteItem {
        cmd: "/search",
        desc: "Neural search: /search <query>",
    },
    PaletteItem {
        cmd: "/open",
        desc: "Open project: /open <name>",
    },
    PaletteItem {
        cmd: "/view",
        desc: "View file: /view <filename>",
    },
    PaletteItem {
        cmd: "/edit",
        desc: "Edit file: /edit <filename>",
    },
    PaletteItem {
        cmd: "/memo",
        desc: "Quick note: /memo <text>",
    },
    PaletteItem {
        cmd: "/health",
        desc: "Open Health Dashboard",
    },
    PaletteItem {
        cmd: "/rules",
        desc: "Go to System Rules",
    },
    PaletteItem {
        cmd: "/memory",
        desc: "Go to MemPalace",
    },
    PaletteItem {
        cmd: "/graphify",
        desc: "Run Graphify (Knowledge Graph) on project",
    },
    PaletteItem {
        cmd: "/heal",
        desc: "Trigger Sentinel Self-Correction for current project",
    },
    PaletteItem {
        cmd: "/reindex",
        desc: "Rebuild Neural Search index",
    },
    PaletteItem {
        cmd: "/quit",
        desc: "Exit R-AI-OS",
    },
];

pub fn filtered_palette(query: &str) -> Vec<&'static PaletteItem> {
    let q = query.trim_start_matches('/').to_lowercase();
    PALETTE_ITEMS
        .iter()
        .filter(|p| {
            q.is_empty() || p.cmd.contains(q.as_str()) || p.desc.to_lowercase().contains(q.as_str())
        })
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
    "Sentinel Hub",
    "Help",
    "AI System Audit",
    "Inbox",
    "Scheduler",
    "Extensions",
];

pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub tick: u64,

    // Config
    pub config: Config,

    pub setup: SetupState,

    // Search
    pub search: SearchState,

    // System & Diagnostics
    pub system: SystemState,

    pub ui: UIState,

    pub inventory: InventoryState,

    // Editor & File View
    pub editor: EditorState,

    // Health Dashboard
    pub health: HealthState,

    // Projects
    pub projects: ProjectState,

    // Background
    pub tx: Sender<BgMsg>,
    pub rx: Receiver<BgMsg>,
    pub tx_daemon: Option<Sender<String>>,

    // Remote Hub mode
    pub is_remote: bool,
    pub remote_host: Option<String>,

    pub width: u16,
    pub height: u16,

    pub timeline: TimelineState,

    // MemPalace
    pub mempalace: MempalaceState,

    // Tasks
    pub tasks: TaskState,

    // Extensions
    pub ext: ExtState,

    // File Watcher
    pub _watcher: Option<Box<dyn Watcher>>,

    // Setup Wizard
    pub wizard: WizardState,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BgMsg>();
        let boot_tx = tx.clone();

        let config = Config::load().unwrap_or_default();

        // Boot results (minimal check for starting)
        let master = config.master_md_path.clone();
        thread::spawn(move || {
            let checks: Vec<(String, PathBuf)> = vec![
                ("MASTER.md".into(), master),
            ];
            for (i, (name, path)) in checks.iter().enumerate() {
                let pass = path.exists();
                let done = i == checks.len() - 1;
                boot_tx
                    .send(BgMsg::BootResult {
                        name: name.clone(),
                        pass,
                        done,
                    })
                    .ok();
            }
        });

        // --- Connect to aiosd Daemon (or spawn embedded workers if offline) ---
        let tx_daemon = ipc::connect_daemon(tx.clone());

        // Spawn embedded workers when aiosd is not available (local only)
        if !config.dev_ops_path.as_os_str().is_empty() {
            let (runtime_tx, runtime_rx) = mpsc::channel::<crate::workers::RuntimeEvent>();
            let tx_forward = tx.clone();
            thread::spawn(move || {
                for event in runtime_rx {
                    let msg = match event {
                        crate::workers::RuntimeEvent::Projects(p) => BgMsg::Projects(p),
                        crate::workers::RuntimeEvent::HealthReport(h) => BgMsg::HealthReport(h),
                        crate::workers::RuntimeEvent::FileChanged(f) => BgMsg::FileChanged(f),
                        crate::workers::RuntimeEvent::SentinelUpdate {
                            project,
                            status,
                            error_count,
                        } => BgMsg::SentinelUpdate {
                            project,
                            status,
                            error_count,
                        },
                    };
                    let _ = tx_forward.send(msg);
                }
            });
            crate::workers::spawn_embedded_workers(runtime_tx, config.dev_ops_path.clone());
        }

        Self {
            state: AppState::Booting,
            should_quit: false,
            tick: 0,
            config,
            setup: SetupState::default(),
            search: SearchState::default(),
            system: SystemState::default(),
            ui: UIState::default(),
            inventory: InventoryState {
                system_rules: crate::app::state::system_rules(),
                ..Default::default()
            },
            editor: EditorState::default(),
            health: HealthState::default(),
            projects: ProjectState::default(),
            tx,
            rx,
            tx_daemon,
            is_remote: false,
            remote_host: None,
            width: 80,
            height: 24,
            timeline: TimelineState::default(),
            mempalace: MempalaceState::default(),
            tasks: TaskState::default(),
            ext: ExtState::default(),
            _watcher: None,
            wizard: WizardState::default(),
        }
    }

    /// Launch TUI connected to a remote Cortex Hub over Tailscale.
    /// Skips local daemon auto-spawn and embedded workers.
    pub fn new_remote(host: String) -> Self {
        let (tx, rx) = mpsc::channel::<BgMsg>();

        let config = Config::load().unwrap_or_default();

        // Probe remote hub HTTP health endpoint to confirm reachability
        let boot_tx = tx.clone();
        let probe_host = host.clone();
        thread::spawn(move || {
            let addr = format!("{}:42071", probe_host);
            let pass = std::net::TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "127.0.0.1:42071".parse().unwrap()),
                std::time::Duration::from_secs(5),
            )
            .is_ok();
            boot_tx
                .send(BgMsg::BootResult {
                    name: format!("Remote Hub ({})", probe_host),
                    pass,
                    done: true,
                })
                .ok();
        });

        let tx_daemon = ipc::connect_daemon_addr(tx.clone(), Some(host.clone()));

        Self {
            state: AppState::Booting,
            should_quit: false,
            tick: 0,
            config,
            setup: SetupState::default(),
            search: SearchState::default(),
            system: SystemState::default(),
            ui: UIState::default(),
            inventory: InventoryState {
                system_rules: crate::app::state::system_rules(),
                ..Default::default()
            },
            editor: EditorState::default(),
            health: HealthState::default(),
            projects: ProjectState::default(),
            tx,
            rx,
            tx_daemon,
            is_remote: true,
            remote_host: Some(host),
            width: 80,
            height: 24,
            timeline: TimelineState::default(),
            mempalace: MempalaceState::default(),
            tasks: TaskState::default(),
            ext: ExtState::default(),
            _watcher: None,
            wizard: WizardState::default(),
        }
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        // Active-file watcher: every ~1 second
        if self.tick.is_multiple_of(25)
            && matches!(self.state, AppState::FileView | AppState::FileEdit)
        {
            if let Some(ref file) = self.editor.active_file {
                let current = std::fs::metadata(&file.path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                if let (Some(watched), Some(cur)) = (self.editor.watched_mtime, current) {
                    if cur != watched {
                        self.editor.changed_externally = true;
                    }
                }
            }
        }

        // Memory-file watcher: every ~2 seconds (50 ticks @ 40ms)
        if self.tick.is_multiple_of(50) && !self.system.memory_watch.is_empty() {
            self.check_memory_files();
        }
    }

    fn check_memory_files(&mut self) {
        let mut changed_paths: Vec<PathBuf> = Vec::new();

        for (path, old_mtime) in &mut self.system.memory_watch {
            let new_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
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
        if let Some(ref proj) = self.projects.active {
            let proj_mem = proj.local_path.join("memory.md");
            if changed_paths.contains(&proj_mem) {
                let content = load_file_content(&proj_mem);
                self.projects.memory_lines = content.lines().map(str::to_owned).collect();
            }
        }

        // Kick off background refresh
        self.system.memory_refresh_pending = true;
        let tx = self.tx.clone();
        let dev_ops = self.config.dev_ops_path.clone();
        thread::spawn(move || {
            tx.send(BgMsg::RecentProjects(load_recent_projects(&dev_ops)))
                .ok();
            tx.send(BgMsg::MemPalaceBuilt(crate::mempalace::build(&dev_ops)))
                .ok();
        });
    }

    pub fn current_menu_files(&self) -> Vec<FileEntry> {
        match self.ui.menu_cursor {
            1 => self.inventory.master_files.clone(),
            3 => self.inventory.agent_files.clone(),
            4 => self.inventory.policy_files.clone(),
            5 => self.inventory.mempalace_files.clone(),
            _ => vec![],
        }
    }

    pub fn sorted_project_indices(&self) -> Vec<usize> {
        crate::app::sort_project_indices(&self.projects.list, &self.health.report, &self.projects.sort)
    }

    pub fn project_at_cursor(&self) -> Option<&crate::entities::EntityProject> {
        let indices = self.sorted_project_indices();
        indices
            .get(self.projects.cursor)
            .and_then(|&i| self.projects.list.get(i))
    }

    fn find_project_path_by_name(&self, name: &str) -> Option<PathBuf> {
        let q = name.to_lowercase();
        self.projects
            .list
            .iter()
            .find(|p| p.name.to_lowercase() == q || p.name.to_lowercase().contains(&q))
            .map(|p| p.local_path.clone())
    }

    pub fn dispatch_task(&mut self, agent: &str) {
        let task = match self.tasks.list.get(self.tasks.cursor) {
            Some(t) => t.clone(),
            None => return,
        };

        let proj_path = task
            .project
            .as_deref()
            .and_then(|name| self.find_project_path_by_name(name))
            .or_else(|| self.projects.active.as_ref().map(|p| p.local_path.clone()));

        // Collect sentinel errors for this project
        let mut sentinel_errors = Vec::new();
        if let Some(ref path) = proj_path {
            let path_str = path.to_string_lossy().to_string();
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
        }

        let result = crate::tasks::dispatch_to_agent(
            &task,
            agent,
            proj_path.as_ref(),
            if sentinel_errors.is_empty() {
                None
            } else {
                Some(sentinel_errors)
            },
        );
        self.system.sync_status = Some(result);
        self.add_activity(
            "Task",
            &format!("Dispatched to {}: {}", agent, task.text),
            "Info",
        );
    }

    pub fn add_activity(&mut self, source: &str, message: &str, level: &'static str) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.timeline.activities.push(Activity {
            timestamp: now,
            source: source.to_string(),
            message: message.to_string(),
            level,
        });
        if self.timeline.activities.len() > 100 {
            self.timeline.activities.remove(0);
        }
    }

    pub(crate) fn get_selected_mempalace_project(&self) -> Option<crate::mempalace::MemProject> {
        let pi = self.mempalace.proj_cursor?;
        self.mempalace
            .rooms
            .get(self.mempalace.room_cursor)?
            .projects
            .get(pi)
            .cloned()
    }

    pub(crate) fn update_search(&mut self) {
        if let Some(ref tx) = self.tx_daemon {
            let cmd = format!(
                "{{\"command\":\"Search\",\"query\":\"{}\"}}",
                self.search.query
            );
            let _ = tx.send(cmd);
        }
    }

    pub fn run_graphify(&self, project_path: &Path) -> String {
        let script = match &self.system.graphify_script {
            Some(s) => s,
            None => return "Graphify script not found in Dev Ops/AI OS/graphify".into(),
        };

        let path_str = project_path.to_string_lossy().into_owned();
        let script_str = script.to_string_lossy().into_owned();
        let (python, python_args) = crate::core::process::python_command();
        let mut graphify_cmd = String::new();
        graphify_cmd.push_str(&python);
        for arg in &python_args {
            graphify_cmd.push(' ');
            graphify_cmd.push_str(arg);
        }
        graphify_cmd.push(' ');
        graphify_cmd.push('"');
        graphify_cmd.push_str(&script_str);
        graphify_cmd.push('"');
        graphify_cmd.push(' ');
        graphify_cmd.push('"');
        graphify_cmd.push_str(&path_str);
        graphify_cmd.push('"');

        if crate::core::process::launch_in_terminal(&graphify_cmd, project_path) {
            return "Graphify started".to_string();
        }

        let mut cmd = std::process::Command::new(&python);
        cmd.args(&python_args).arg(&script_str).arg(&path_str);
        match cmd.spawn() {
            Ok(_) => "Graphify started".to_string(),
            Err(e) => format!("Graphify launch error: {}", e),
        }
    }
}
