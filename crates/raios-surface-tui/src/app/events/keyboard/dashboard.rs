use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::{filtered_palette, App};

impl App {
    pub(crate) fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.ui.command_mode = false;
                self.ui.command_buf.clear();
                self.ui.palette_cursor = 0;
            }
            KeyCode::Enter => {
                let command = if self.ui.command_buf.starts_with('/') {
                    self.ui.command_buf.trim().to_string()
                } else {
                    filtered_palette(&self.ui.command_buf)
                        .get(self.ui.palette_cursor)
                        .map(|item| item.cmd.to_string())
                        .unwrap_or_default()
                };
                self.ui.command_buf.clear();
                self.ui.command_mode = false;
                self.ui.palette_cursor = 0;
                if !command.is_empty() {
                    self.execute_command(&command)?;
                }
            }
            KeyCode::Tab => {
                if let Some(item) =
                    filtered_palette(&self.ui.command_buf).get(self.ui.palette_cursor)
                {
                    self.ui.command_buf = format!("{} ", item.cmd);
                    self.ui.palette_cursor = 0;
                }
            }
            KeyCode::Up if self.ui.palette_cursor > 0 => self.ui.palette_cursor -= 1,
            KeyCode::Down => {
                let maximum = filtered_palette(&self.ui.command_buf)
                    .len()
                    .saturating_sub(1);
                self.ui.palette_cursor = (self.ui.palette_cursor + 1).min(maximum);
            }
            KeyCode::Backspace if self.ui.command_buf.is_empty() => self.ui.command_mode = false,
            KeyCode::Backspace => {
                self.ui.command_buf.pop();
                self.ui.palette_cursor = 0;
            }
            KeyCode::Char(character) => {
                self.ui.command_buf.push(character);
                self.ui.palette_cursor = 0;
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn handle_dashboard_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.handle_control_dashboard_key(key)? {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.state = AppState::HelpView,
            KeyCode::Char('/') => {
                self.ui.command_mode = true;
                self.ui.command_buf = "/".into();
                self.ui.palette_cursor = 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_control_dashboard_key(&mut self, key: KeyEvent) -> Result<bool> {
        use crate::app::intent::Intent;
        use crate::app::reducer::reduce_intent;
        use crate::app::route::Route;
        use crate::app::store::WorkFocus;
        use raios_contracts::{Command, Query};

        if self.store.explore_search.is_editing {
            self.handle_explore_search_key(key);
            return Ok(true);
        }

        if self.store.task_composer.is_open {
            self.handle_task_composer_key(key)?;
            return Ok(true);
        }

        match key.code {
            KeyCode::Char('1') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Now)),
            KeyCode::Char('2') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Work)),
            KeyCode::Char('3') => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Explore))
            }
            KeyCode::Char('4') => {
                reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Govern))
            }
            KeyCode::Tab => reduce_intent(&mut self.store, Intent::NextRoute),
            KeyCode::BackTab => reduce_intent(&mut self.store, Intent::PrevRoute),
            KeyCode::Up | KeyCode::Char('k') => self.move_control_cursor(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_control_cursor(true),
            KeyCode::Left | KeyCode::Char('h') => self.move_work_focus(false),
            KeyCode::Right | KeyCode::Char('l') => self.move_work_focus(true),
            KeyCode::Char(' ') if self.store.current_route == Route::Now => {
                self.cycle_operation_panel()
            }
            KeyCode::Char(' ') if self.store.current_route == Route::Work => {
                self.move_work_focus(true)
            }
            KeyCode::Char(' ') => self.set_control_focus(!self.store.right_panel_focus),
            KeyCode::Enter
                if self.store.current_route == Route::Work
                    && self.store.work_focus == WorkFocus::Ocak =>
            {
                self.open_selected_ocak_command();
            }
            KeyCode::Char('s')
                if self.store.current_route == Route::Work
                    && self.store.work_focus == WorkFocus::Projects =>
            {
                self.store.work_sort = self.store.work_sort.next();
                self.select_control_row(0, false);
            }
            KeyCode::Char('n') if self.store.current_route == Route::Work => {
                self.begin_task_composer();
            }
            KeyCode::Char('/') if self.store.current_route == Route::Explore => {
                self.store.explore_search.begin();
            }
            KeyCode::Char('i')
                if self.store.current_route == Route::Work
                    && self.store.work_focus == WorkFocus::Tasks =>
            {
                self.update_selected_work_task("in_progress");
            }
            KeyCode::Char('b')
                if self.store.current_route == Route::Work
                    && self.store.work_focus == WorkFocus::Tasks =>
            {
                self.update_selected_work_task("blocked");
            }
            KeyCode::Char('c')
                if self.store.current_route == Route::Work
                    && self.store.work_focus == WorkFocus::Tasks =>
            {
                self.update_selected_work_task("completed");
            }
            KeyCode::Enter
                if self.store.current_route == Route::Now
                    && self.store.operations.panel
                        == crate::app::operations::OperationPanel::Actions =>
            {
                self.execute_selected_operation_action()?;
            }
            KeyCode::Enter
                if self.store.current_route == Route::Explore && !self.store.right_panel_focus =>
            {
                self.open_selected_explore_result();
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if self.store.current_route != Route::Now {
                    self.store.last_error = Some("Approvals are available only on NOW.".into());
                } else if let Some(approval_id) = self.selected_now_approval_id() {
                    let command = Command::ApproveHandoff {
                        idempotency_key: format!("approve-{approval_id}"),
                        approval_id,
                    };
                    if let Err(problem) = self.client.send_command(command) {
                        self.store.last_error = Some(problem.message);
                    }
                } else {
                    self.store.last_error = Some("Select an approval in ATTENTION first.".into());
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.store.current_route == Route::Govern && self.store.right_panel_focus {
                    self.trigger_selected_cron_job();
                } else if self.store.current_route == Route::Now {
                    if let Some(approval_id) = self.selected_now_approval_id() {
                        let command = Command::RejectHandoff {
                            idempotency_key: format!("reject-{approval_id}"),
                            approval_id,
                            reason: "Rejected by TUI user".into(),
                        };
                        if let Err(problem) = self.client.send_command(command) {
                            self.store.last_error = Some(problem.message);
                        }
                    } else {
                        self.store.last_error =
                            Some("Select an approval in ATTENTION first.".into());
                    }
                } else {
                    self.store.last_error = Some("Reject is available only on NOW.".into());
                }
            }
            KeyCode::Char('p') | KeyCode::Char('P')
                if self.store.current_route == Route::Govern && self.store.right_panel_focus =>
            {
                self.toggle_selected_cron_job();
            }
            KeyCode::Char('g') => {
                if let Err(problem) = self.client.send_query(Query::GetSystemSnapshot) {
                    self.store.last_error = Some(problem.message);
                }
            }
            KeyCode::Esc => {
                if self.store.last_error.take().is_none() {
                    if self.store.current_route == Route::Now {
                        self.set_operation_panel(crate::app::operations::OperationPanel::Attention);
                    } else {
                        self.set_control_focus(false);
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Captures only bounded, read-only workspace-search input for EXPLORE.
    fn handle_explore_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.store.explore_search.cancel(),
            KeyCode::Enter => self.submit_explore_search(),
            KeyCode::Backspace => {
                self.store.explore_search.query.pop();
            }
            KeyCode::Char(character) if self.store.explore_search.query.chars().count() < 240 => {
                self.store.explore_search.query.push(character);
            }
            _ => {}
        }
    }

    /// Sends a safely serialized, read-only daemon search request.
    fn submit_explore_search(&mut self) {
        let query = self.store.explore_search.query.trim();
        if query.is_empty() {
            self.store.last_error = Some("Enter a workspace search query first.".into());
            return;
        }
        if raios_core::security::looks_like_secret(query).is_some() {
            self.store.last_error =
                Some("Search query appears to contain a secret and was not sent.".into());
            return;
        }
        let command = crate::app::daemon_search_command(query);
        let Some(tx_daemon) = &self.tx_daemon else {
            self.store.last_error =
                Some("Daemon is disconnected; workspace search is unavailable.".into());
            return;
        };
        if tx_daemon.send(command).is_err() {
            self.store.last_error = Some("Daemon search request could not be delivered.".into());
            return;
        }
        self.store.explore_search.is_editing = false;
        self.store.explore_search.status = Some("Searching indexed workspace…".into());
    }

    /// Opens a selected Explore match only when it resolves under the local workspace root.
    fn open_selected_explore_result(&mut self) {
        if self.is_remote {
            self.store.last_error =
                Some("Opening remote search paths from the local TUI is disabled.".into());
            return;
        }
        let Some(result) = self.store.explore_results().get(self.store.cursor) else {
            self.store.last_error = Some("Select a search result first.".into());
            return;
        };
        let candidate = std::path::PathBuf::from(&result.file_path);
        let workspace = std::fs::canonicalize(&self.config.dev_ops_path);
        let path = std::fs::canonicalize(&candidate);
        let (Ok(workspace), Ok(path)) = (workspace, path) else {
            self.store.last_error =
                Some("Selected search result is no longer an accessible local file.".into());
            return;
        };
        if !path.starts_with(&workspace) || !path.is_file() {
            self.store.last_error =
                Some("Selected search result is outside the configured workspace.".into());
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        self.open_file_view(raios_runtime::filebrowser::FileEntry::new(name, path));
    }

    /// Schedules one immediate run for the currently selected cron job.
    pub(crate) fn trigger_selected_cron_job(&mut self) {
        let Some(job) = self.store.snapshot.govern.cron_jobs.get(self.store.cursor) else {
            self.store.last_error = Some("No cron job selected.".into());
            return;
        };
        let command = raios_contracts::Command::TriggerCronJob {
            idempotency_key: format!(
                "trigger-{}-snapshot-{}",
                job.id, self.store.snapshot.sequence
            ),
            job_id: job.id.clone(),
        };
        if let Err(problem) = self.client.send_command(command) {
            self.store.last_error = Some(problem.message);
        }
    }

    /// Pauses or resumes the selected cron job through the audited typed command path.
    pub(crate) fn toggle_selected_cron_job(&mut self) {
        let Some(job) = self.store.snapshot.govern.cron_jobs.get(self.store.cursor) else {
            self.store.last_error = Some("No cron job selected.".into());
            return;
        };
        let paused = job.status.eq_ignore_ascii_case("active");
        let command = raios_contracts::Command::ToggleCronJob {
            idempotency_key: format!(
                "toggle-{}-paused-{}-snapshot-{}",
                job.id, paused, self.store.snapshot.sequence
            ),
            job_id: job.id.clone(),
            paused,
        };
        if let Err(problem) = self.client.send_command(command) {
            self.store.last_error = Some(problem.message);
        }
    }

    fn execute_selected_operation_action(&mut self) -> Result<()> {
        use crate::app::intent::Intent;
        use crate::app::operations::OperationActionKind;
        use crate::app::reducer::reduce_intent;
        use crate::app::route::Route;
        use raios_contracts::Query;

        let Some(action) = self.store.operations.selected_action().cloned() else {
            self.store.last_error = Some("No operation action selected.".into());
            return Ok(());
        };

        match action.kind {
            OperationActionKind::RefreshSnapshot => {
                if let Err(problem) = self.client.send_query(Query::GetSystemSnapshot) {
                    self.store.last_error = Some(problem.message);
                } else {
                    self.store
                        .add_log("Requested a fresh control-plane snapshot.");
                }
            }
            OperationActionKind::OpenProjectWorkbench => {
                let project_path = self
                    .store
                    .selected_project()
                    .map(|project| project.path.clone());
                if let Some(project_path) = project_path {
                    self.store.selected_project_path = Some(project_path);
                    reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Work));
                } else {
                    self.store.last_error = Some("No registered project is available.".into());
                }
            }
            OperationActionKind::ReviewApproval => {
                self.set_operation_panel(crate::app::operations::OperationPanel::Attention);
                self.store.cursor = 0;
            }
            OperationActionKind::ReviewBlockedTask => {
                self.set_operation_panel(crate::app::operations::OperationPanel::Attention);
                self.store.cursor = self.store.snapshot.now.approvals.len();
            }
            OperationActionKind::LaunchCodexAgent => {
                let project_path = self
                    .store
                    .selected_project()
                    .map(|project| project.path.clone());
                let Some(project_path) = project_path else {
                    self.store.last_error = Some("No registered project is available.".into());
                    return Ok(());
                };
                let command = raios_contracts::Command::LaunchAgent {
                    agent_name: "codex".into(),
                    project_path,
                    prompt: None,
                    idempotency_key: format!("tui-launch-codex-{}", uuid::Uuid::new_v4()),
                };
                if let Err(problem) = self.client.send_command(command) {
                    self.store.last_error = Some(problem.message);
                } else {
                    self.store
                        .add_log("Requested a tracked Codex session in a separate terminal.");
                }
            }
        }

        Ok(())
    }

    fn handle_task_composer_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.store.task_composer.cancel(),
            KeyCode::Backspace => {
                self.store.task_composer.title.pop();
            }
            KeyCode::Char('+') => {
                self.store.task_composer.priority =
                    self.store.task_composer.priority.saturating_add(10);
            }
            KeyCode::Char('-') => {
                self.store.task_composer.priority =
                    self.store.task_composer.priority.saturating_sub(10);
            }
            KeyCode::Char(character) if self.store.task_composer.title.chars().count() < 240 => {
                self.store.task_composer.title.push(character);
            }
            KeyCode::Enter => self.submit_task_composer(),
            _ => {}
        }
        Ok(())
    }

    /// Opens the personal-task composer only when a trusted project is selected.
    pub(crate) fn begin_task_composer(&mut self) {
        if self.store.selected_project().is_some() {
            self.store.task_composer.begin();
        } else {
            self.store.last_error =
                Some("Select a registered project before creating a task.".into());
        }
    }

    /// Sends the current local draft through the authenticated control-plane client.
    pub(crate) fn submit_task_composer(&mut self) {
        use raios_contracts::Command;

        let title = self.store.task_composer.title.trim().to_owned();
        let project_path = self
            .store
            .selected_project()
            .map(|project| project.path.clone());
        if title.is_empty() {
            self.store.last_error = Some("Task title is required.".into());
            return;
        }
        let Some(project_path) = project_path else {
            self.store.last_error = Some("Selected project is no longer available.".into());
            return;
        };
        let command = Command::CreateTask {
            title,
            project_path: Some(project_path),
            priority: self.store.task_composer.priority,
            idempotency_key: format!("tui-create-task-{}", uuid::Uuid::new_v4()),
        };
        if let Err(problem) = self.client.send_command(command) {
            self.store.last_error = Some(problem.message);
        } else {
            self.store.task_composer.cancel();
            self.store
                .add_log("Submitted a new personal task to the control plane.");
        }
    }

    /// Changes the selected personal task through the authenticated control plane.
    pub(crate) fn update_selected_work_task(&mut self, status: &str) {
        use crate::app::store::WorkFocus;
        use raios_contracts::Command;

        if self.store.work_focus != WorkFocus::Tasks {
            self.store.last_error = Some("Select a task before changing its status.".into());
            return;
        }
        let task_id = self
            .store
            .snapshot
            .work
            .tasks
            .get(self.store.cursor)
            .map(|task| task.id.clone());
        let Some(task_id) = task_id else {
            self.store.last_error = Some("Select a task before changing its status.".into());
            return;
        };
        let command = Command::UpdateTaskStatus {
            task_id,
            status: status.into(),
            idempotency_key: format!("tui-update-task-{}", uuid::Uuid::new_v4()),
        };
        if let Err(problem) = self.client.send_command(command) {
            self.store.last_error = Some(problem.message);
        }
    }
}
