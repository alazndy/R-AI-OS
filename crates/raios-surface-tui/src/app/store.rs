//! Typed control-plane UI state store.

use raios_contracts::{
    ExploreSnapshot, GovernSnapshot, NowSnapshot, SnapshotEnvelope, WorkSnapshot,
};

use crate::app::route::Route;
use crate::app::state::SortMode;

/// Focus target inside the three-pane Work route.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkFocus {
    /// Registered projects in the left pane.
    #[default]
    Projects,
    /// Read-only Ocak summary lines in the upper right pane.
    Ocak,
    /// Control-plane tasks in the lower right pane.
    Tasks,
}

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
    /// Active focus target while the Work route is open.
    pub work_focus: WorkFocus,
    /// Client-side ordering for the typed Work project snapshot.
    pub work_sort: SortMode,
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
            work_focus: WorkFocus::Projects,
            work_sort: SortMode::default(),
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

    /// Returns project indices in the currently selected local ordering.
    pub fn work_project_indices(&self) -> Vec<usize> {
        let projects = &self.snapshot.work.projects;
        let mut indices: Vec<usize> = (0..projects.len()).collect();
        match self.work_sort {
            SortMode::Name => indices.sort_by_key(|&index| projects[index].name.to_lowercase()),
            SortMode::Grade => indices.sort_by_key(|&index| !projects[index].has_memory),
            SortMode::GitDirty => {
                indices.sort_by_key(|&index| std::cmp::Reverse(projects[index].dirty_files));
            }
            SortMode::Category => indices.sort_by_key(|&index| {
                std::path::Path::new(&projects[index].path)
                    .parent()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            }),
            SortMode::Status => {
                indices.sort_by_key(|&index| projects[index].status.to_ascii_lowercase());
            }
        }
        indices
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

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::app::state::SortMode;
    use raios_contracts::ProjectDto;

    #[test]
    fn work_project_order_follows_the_selected_sort_mode() {
        let mut store = Store::new();
        store.snapshot.work.projects = vec![
            ProjectDto {
                name: "zeta".into(),
                dirty_files: 1,
                ..Default::default()
            },
            ProjectDto {
                name: "alpha".into(),
                dirty_files: 4,
                ..Default::default()
            },
        ];

        assert_eq!(store.work_project_indices(), vec![1, 0]);
        store.work_sort = SortMode::GitDirty;
        assert_eq!(store.work_project_indices(), vec![1, 0]);
    }
}
