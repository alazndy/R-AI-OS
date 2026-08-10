//! Typed control-plane UI state store.

use raios_contracts::{
    ExploreSnapshot, GovernSnapshot, NowSnapshot, ProjectDto, SearchResultDto, SnapshotEnvelope,
    WorkSnapshot,
};

use crate::app::operations::{OperationsConsole, TaskComposer};
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
    /// Local interaction state for the read-only Explore search workflow.
    pub explore_search: ExploreSearch,
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
    /// Typed selection state for the operational-console workflow.
    pub operations: OperationsConsole,
    /// Local draft state for the WORK-route task composer.
    pub task_composer: TaskComposer,
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
            explore_search: ExploreSearch::default(),
            command_mode: false,
            command_buf: String::new(),
            help_open: false,
            logs: Vec::new(),
            last_error: None,
            operations: OperationsConsole::default(),
            task_composer: TaskComposer::default(),
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
        self.ensure_selected_project();
        self.rebuild_operations();
    }

    /// Returns the selected project, falling back to the first registered project.
    pub fn selected_project(&self) -> Option<&ProjectDto> {
        let selected_path = self.selected_project_path.as_deref();
        self.snapshot
            .work
            .projects
            .iter()
            .find(|project| Some(project.path.as_str()) == selected_path)
            .or_else(|| self.snapshot.work.projects.first())
    }

    /// Rebuilds contextual console actions after a trusted snapshot or selection changes.
    pub fn rebuild_operations(&mut self) {
        self.operations.rebuild(
            !self.snapshot.now.approvals.is_empty(),
            !self.snapshot.now.blocked_tasks.is_empty(),
            self.selected_project().is_some(),
        );
    }

    fn ensure_selected_project(&mut self) {
        let still_exists = self.selected_project_path.as_deref().is_some_and(|path| {
            self.snapshot
                .work
                .projects
                .iter()
                .any(|project| project.path == path)
        });
        if !still_exists {
            self.selected_project_path = self
                .snapshot
                .work
                .projects
                .first()
                .map(|project| project.path.clone());
        }
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

    /// Returns live daemon search hits when available, otherwise the snapshot projection.
    pub fn explore_results(&self) -> &[SearchResultDto] {
        if self.explore_search.results.is_empty() {
            &self.snapshot.explore.search_results
        } else {
            &self.explore_search.results
        }
    }
}

/// In-memory state for the Explore route's read-only workspace search box.
#[derive(Debug, Clone, Default)]
pub struct ExploreSearch {
    /// Query text currently being composed by the operator.
    pub query: String,
    /// Whether keyboard input belongs to the search box instead of route navigation.
    pub is_editing: bool,
    /// Latest daemon-index search result projection.
    pub results: Vec<SearchResultDto>,
    /// Short lifecycle message, never treated as authoritative result data.
    pub status: Option<String>,
}

impl ExploreSearch {
    /// Starts editing while retaining the previous query for quick refinement.
    pub fn begin(&mut self) {
        self.is_editing = true;
        self.status = None;
    }

    /// Stops editing without discarding the last successful result set.
    pub fn cancel(&mut self) {
        self.is_editing = false;
    }

    /// Replaces the visible result set after a daemon response.
    pub fn set_results(&mut self, results: Vec<SearchResultDto>) {
        self.results = results;
        self.status = Some(format!("{} result(s)", self.results.len()));
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
    use raios_contracts::{ProjectDto, SearchResultDto, SnapshotEnvelope};

    use super::Store;
    use crate::app::state::SortMode;

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

    #[test]
    fn snapshot_selects_first_project_and_builds_contextual_actions() {
        let mut store = Store::new();
        let mut snapshot = SnapshotEnvelope {
            sequence: 1,
            timestamp: "2026-08-06T00:00:00Z".into(),
            now: Default::default(),
            work: Default::default(),
            explore: Default::default(),
            govern: Default::default(),
        };
        snapshot.work.projects.push(ProjectDto {
            name: "R-AI-OS".into(),
            path: "/workspace/raios".into(),
            ..ProjectDto::default()
        });

        store.set_snapshot(snapshot);

        assert_eq!(
            store.selected_project_path.as_deref(),
            Some("/workspace/raios")
        );
        assert_eq!(store.operations.actions.len(), 3);
        assert_eq!(store.operations.actions[0].id, "open-workbench");
        assert_eq!(store.operations.actions[1].id, "launch-codex");
        assert_eq!(store.operations.actions[2].id, "refresh-snapshot");
    }

    #[test]
    fn explore_search_retains_results_when_editing_is_cancelled() {
        let mut store = Store::new();
        store.explore_search.set_results(vec![SearchResultDto {
            file_path: "/workspace/raios/src/lib.rs".into(),
            line_number: 12,
            snippet: "pub fn search()".into(),
            score: 0.9,
        }]);

        store.explore_search.begin();
        store.explore_search.query = "search".into();
        store.explore_search.cancel();

        assert!(!store.explore_search.is_editing);
        assert_eq!(store.explore_results().len(), 1);
    }
}
