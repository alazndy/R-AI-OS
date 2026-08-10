use crate::app::operations::OperationPanel;
use crate::app::route::Route;
use crate::app::state::UIState;
use crate::app::store::WorkFocus;
use crate::app::App;

/// Returns the reviewed command prefix associated with an Ocak summary row.
pub(crate) fn ocak_command_prefix(summary_row: usize) -> Option<&'static str> {
    match summary_row {
        0 => Some("/ocak product "),
        1 => Some("/ocak cycle "),
        2 => Some("/ocak change "),
        3 => Some("/ocak support "),
        4 => Some("/ocak quality "),
        5 => Some("/ocak release "),
        _ => None,
    }
}

/// Opens the palette as a draft and deliberately leaves command dispatch to the user.
pub(crate) fn apply_ocak_command_draft(ui: &mut UIState, summary_row: usize) -> bool {
    let Some(prefix) = ocak_command_prefix(summary_row) else {
        return false;
    };
    ui.command_mode = true;
    ui.command_buf = prefix.into();
    ui.palette_cursor = 0;
    true
}

fn bounded_cursor(cursor: usize, count: usize, down: bool) -> usize {
    if count == 0 {
        0
    } else if down {
        (cursor + 1).min(count - 1)
    } else {
        cursor.saturating_sub(1)
    }
}

impl App {
    pub(crate) fn control_item_count(&self) -> usize {
        match self.store.current_route {
            Route::Now => match self.store.operations.panel {
                OperationPanel::Attention => {
                    self.store.snapshot.now.approvals.len()
                        + self.store.snapshot.now.blocked_tasks.len()
                }
                OperationPanel::Project => usize::from(self.store.selected_project().is_some()),
                OperationPanel::Actions => self.store.operations.actions.len(),
            },
            Route::Work => match self.store.work_focus {
                WorkFocus::Projects => self.store.snapshot.work.projects.len(),
                WorkFocus::Ocak => 6,
                WorkFocus::Tasks => self.store.snapshot.work.tasks.len(),
            },
            Route::Explore if self.store.right_panel_focus => {
                self.store.snapshot.explore.recent_logs.len()
            }
            Route::Explore => self.store.explore_results().len(),
            Route::Govern if self.store.right_panel_focus => {
                self.store.snapshot.govern.cron_jobs.len()
            }
            Route::Govern => 0,
        }
    }

    pub(crate) fn control_focus_label(&self) -> &'static str {
        if self.store.current_route == Route::Now {
            return match self.store.operations.panel {
                OperationPanel::Attention => "ATTENTION",
                OperationPanel::Project => "PROJECT",
                OperationPanel::Actions => "NEXT ACTIONS",
            };
        }

        match (self.store.current_route, self.store.right_panel_focus) {
            (Route::Work, _) => match self.store.work_focus {
                WorkFocus::Projects => "PROJECTS",
                WorkFocus::Ocak => "OCAK",
                WorkFocus::Tasks => "TASKS",
            },
            (Route::Explore, false) => "RESULTS",
            (Route::Explore, true) => "LOGS",
            (Route::Govern, false) => "OVERVIEW",
            (Route::Govern, true) => "SCHEDULER",
            (Route::Now, _) => unreachable!("NOW focus is handled above"),
        }
    }

    pub(crate) fn set_control_focus(&mut self, right_panel_focus: bool) {
        if self.store.current_route == Route::Now {
            self.set_operation_panel(if right_panel_focus {
                OperationPanel::Project
            } else {
                OperationPanel::Attention
            });
            return;
        }
        self.store.right_panel_focus = right_panel_focus;
        if self.store.current_route == Route::Work {
            self.store.work_focus = if right_panel_focus {
                WorkFocus::Tasks
            } else {
                WorkFocus::Projects
            };
        }
        self.clamp_control_cursor();
    }

    /// Moves the Work route focus across Projects, Ocak, and Tasks.
    pub(crate) fn move_work_focus(&mut self, forward: bool) {
        if self.store.current_route != Route::Work {
            self.set_control_focus(forward);
            return;
        }

        self.store.work_focus = match (self.store.work_focus, forward) {
            (WorkFocus::Projects, true) => WorkFocus::Ocak,
            (WorkFocus::Ocak, true) => WorkFocus::Tasks,
            (WorkFocus::Tasks, true) => WorkFocus::Tasks,
            (WorkFocus::Tasks, false) => WorkFocus::Ocak,
            (WorkFocus::Ocak, false) => WorkFocus::Projects,
            (WorkFocus::Projects, false) => WorkFocus::Projects,
        };
        self.store.right_panel_focus = self.store.work_focus != WorkFocus::Projects;
        self.store.cursor = 0;
        self.clamp_control_cursor();
        self.sync_selected_work_project();
    }

    /// Opens the command palette with the selected Ocak action as a draft.
    pub(crate) fn open_selected_ocak_command(&mut self) {
        if self.store.current_route != Route::Work || self.store.work_focus != WorkFocus::Ocak {
            return;
        }
        apply_ocak_command_draft(&mut self.ui, self.store.cursor);
    }

    pub(crate) fn set_operation_panel(&mut self, panel: OperationPanel) {
        self.store.operations.panel = panel;
        self.store.right_panel_focus = panel == OperationPanel::Project;
        self.clamp_control_cursor();
    }

    pub(crate) fn cycle_operation_panel(&mut self) {
        self.set_operation_panel(self.store.operations.panel.next());
    }

    pub(crate) fn move_control_cursor(&mut self, down: bool) {
        if self.store.current_route == Route::Now
            && self.store.operations.panel == OperationPanel::Actions
        {
            self.store.operations.action_cursor = bounded_cursor(
                self.store.operations.action_cursor,
                self.store.operations.actions.len(),
                down,
            );
            return;
        }
        self.store.cursor = bounded_cursor(self.store.cursor, self.control_item_count(), down);
        self.sync_selected_work_project();
    }

    pub(crate) fn select_control_row(&mut self, row: usize, right_panel_focus: bool) {
        self.set_control_focus(right_panel_focus);
        self.store.cursor = row;
        self.clamp_control_cursor();
        self.sync_selected_work_project();
    }

    fn clamp_control_cursor(&mut self) {
        if self.store.current_route == Route::Now
            && self.store.operations.panel == OperationPanel::Actions
        {
            self.store.operations.action_cursor = self
                .store
                .operations
                .action_cursor
                .min(self.store.operations.actions.len().saturating_sub(1));
            return;
        }
        self.store.cursor = self
            .store
            .cursor
            .min(self.control_item_count().saturating_sub(1));
    }

    fn sync_selected_work_project(&mut self) {
        if self.store.current_route != Route::Work {
            return;
        }

        let project_path = match self.store.work_focus {
            WorkFocus::Projects => self
                .store
                .snapshot
                .work
                .projects
                .get(
                    *self
                        .store
                        .work_project_indices()
                        .get(self.store.cursor)
                        .unwrap_or(&0),
                )
                .map(|project| project.path.clone()),
            WorkFocus::Tasks => self
                .store
                .snapshot
                .work
                .tasks
                .get(self.store.cursor)
                .and_then(|task| task.project_path.clone()),
            WorkFocus::Ocak => None,
        };

        if let Some(project_path) = project_path {
            self.store.selected_project_path = Some(project_path);
            self.store.rebuild_operations();
        }
    }

    /// Returns the selected handoff approval only while the NOW attention panel is focused.
    pub(crate) fn selected_now_approval_id(&self) -> Option<String> {
        if self.store.current_route != Route::Now
            || self.store.operations.panel != OperationPanel::Attention
        {
            return None;
        }
        self.store
            .snapshot
            .now
            .approvals
            .get(self.store.cursor)
            .map(|approval| approval.id.clone())
    }

    pub(crate) fn select_operation_action(&mut self, row: usize) {
        self.set_operation_panel(OperationPanel::Actions);
        self.store.operations.action_cursor = row;
        self.clamp_control_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_ocak_command_draft, bounded_cursor, ocak_command_prefix};
    use crate::app::state::UIState;

    #[test]
    fn cursor_stays_within_the_focused_list() {
        assert_eq!(bounded_cursor(0, 0, true), 0);
        assert_eq!(bounded_cursor(0, 2, false), 0);
        assert_eq!(bounded_cursor(0, 2, true), 1);
        assert_eq!(bounded_cursor(1, 2, true), 1);
    }

    #[test]
    fn ocak_summary_rows_map_only_to_supported_command_prefixes() {
        assert_eq!(ocak_command_prefix(0), Some("/ocak product "));
        assert_eq!(ocak_command_prefix(1), Some("/ocak cycle "));
        assert_eq!(ocak_command_prefix(2), Some("/ocak change "));
        assert_eq!(ocak_command_prefix(3), Some("/ocak support "));
        assert_eq!(ocak_command_prefix(4), Some("/ocak quality "));
        assert_eq!(ocak_command_prefix(5), Some("/ocak release "));
        assert_eq!(ocak_command_prefix(6), None);
    }

    #[test]
    fn ocak_selection_only_opens_a_prefilled_palette() {
        let mut ui = UIState::default();

        assert!(apply_ocak_command_draft(&mut ui, 2));
        assert!(ui.command_mode);
        assert_eq!(ui.command_buf, "/ocak change ");
        assert_eq!(ui.palette_cursor, 0);
    }
}
