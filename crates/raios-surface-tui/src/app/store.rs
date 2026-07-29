//! Typed control-plane UI state store.

use raios_contracts::{
    ExploreSnapshot, GovernSnapshot, NowSnapshot, SnapshotEnvelope, WorkSnapshot,
};

use crate::app::route::Route;

/// Central reactive state store for the TUI control-plane views.
#[derive(Debug, Clone)]
pub struct Store {
    /// Currently active route view.
    pub current_route: Route,
    /// Latest system snapshot envelope received from daemon.
    pub snapshot: SnapshotEnvelope,
    /// Whether connected to the daemon IPC socket.
    pub daemon_connected: bool,
    /// Address of the target daemon socket or HTTP endpoint.
    pub daemon_address: String,
    /// `true` if focus is on the right-hand panel.
    pub right_panel_focus: bool,
    /// Main list selection cursor index.
    pub cursor: usize,
    /// Sub-item list selection cursor index.
    pub sub_cursor: usize,
    /// Project identity stays selected while the user navigates the task list.
    pub selected_project_path: Option<String>,
    /// Active search input string.
    pub search_input: String,
    /// `true` when command palette modal is active.
    pub command_mode: bool,
    /// Active command palette input buffer text.
    pub command_buf: String,
    /// `true` when help overlay is displayed.
    pub help_open: bool,
    /// Log message history buffer.
    pub logs: Vec<String>,
    /// Last error message string, if any.
    pub last_error: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            current_route: Route::Now,
            snapshot: SnapshotEnvelope {
                sequence: 0,
                timestamp: String::new(),
                now: NowSnapshot::default(),
                work: WorkSnapshot::default(),
                explore: ExploreSnapshot::default(),
                govern: GovernSnapshot::default(),
            },
            daemon_connected: false,
            daemon_address: "127.0.0.1:42071".into(),
            right_panel_focus: false,
            cursor: 0,
            sub_cursor: 0,
            selected_project_path: None,
            search_input: String::new(),
            command_mode: false,
            command_buf: String::new(),
            help_open: false,
            logs: Vec::new(),
            last_error: None,
        }
    }
}

impl Store {
    /// Creates a new default `Store` instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a new system snapshot envelope, enriching legacy daemon previews if needed.
    pub fn set_snapshot(&mut self, env: SnapshotEnvelope) {
        self.snapshot = enrich_legacy_daemon_memory(env);
    }

    /// Appends a new log message entry to the store's log buffer.
    pub fn add_log(&mut self, log: impl Into<String>) {
        self.logs.push(log.into());
        if self.logs.len() > 500 {
            self.logs.remove(0);
        }
    }
}

fn enrich_legacy_daemon_memory(mut env: SnapshotEnvelope) -> SnapshotEnvelope {
    for project in &mut env.work.projects {
        if project.memory_preview.is_some() {
            continue;
        }

        if let Some(memory) =
            crate::app::services::load_local_memory_preview(std::path::Path::new(&project.path))
        {
            project.has_memory = memory.has_memory;
            project.memory_preview = memory.preview;
        }
    }

    env
}
