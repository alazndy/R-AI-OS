use crate::app::route::Route;
use crate::app::App;

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
            Route::Now if self.store.right_panel_focus => {
                self.store.snapshot.now.blocked_tasks.len()
            }
            Route::Now => self.store.snapshot.now.approvals.len(),
            Route::Work if self.store.right_panel_focus => self.store.snapshot.work.tasks.len(),
            Route::Work => self.store.snapshot.work.projects.len(),
            Route::Explore if self.store.right_panel_focus => {
                self.store.snapshot.explore.recent_logs.len()
            }
            Route::Explore => self.store.snapshot.explore.recent_traces.len(),
            Route::Govern if self.store.right_panel_focus => {
                self.store.snapshot.govern.cron_jobs.len()
            }
            Route::Govern => 0,
        }
    }

    pub(crate) fn control_focus_label(&self) -> &'static str {
        match (self.store.current_route, self.store.right_panel_focus) {
            (Route::Now, false) => "APPROVALS",
            (Route::Now, true) => "BLOCKED TASKS",
            (Route::Work, false) => "PROJECTS",
            (Route::Work, true) => "TASKS",
            (Route::Explore, false) => "TRACES",
            (Route::Explore, true) => "LOGS",
            (Route::Govern, false) => "OVERVIEW",
            (Route::Govern, true) => "SCHEDULER",
        }
    }

    pub(crate) fn set_control_focus(&mut self, right_panel_focus: bool) {
        self.store.right_panel_focus = right_panel_focus;
        self.clamp_control_cursor();
    }

    pub(crate) fn move_control_cursor(&mut self, down: bool) {
        self.store.cursor = bounded_cursor(self.store.cursor, self.control_item_count(), down);
        self.sync_selected_work_project();
    }

    pub(crate) fn select_control_row(&mut self, row: usize, right_panel_focus: bool) {
        self.store.right_panel_focus = right_panel_focus;
        self.store.cursor = row;
        self.clamp_control_cursor();
        self.sync_selected_work_project();
    }

    fn clamp_control_cursor(&mut self) {
        self.store.cursor = self
            .store
            .cursor
            .min(self.control_item_count().saturating_sub(1));
    }

    fn sync_selected_work_project(&mut self) {
        if self.store.current_route != Route::Work {
            return;
        }

        let project_path = if self.store.right_panel_focus {
            self.store
                .snapshot
                .work
                .tasks
                .get(self.store.cursor)
                .and_then(|task| task.project_path.clone())
        } else {
            self.store
                .snapshot
                .work
                .projects
                .get(self.store.cursor)
                .map(|project| project.path.clone())
        };

        if let Some(project_path) = project_path {
            self.store.selected_project_path = Some(project_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bounded_cursor;

    #[test]
    fn cursor_stays_within_the_focused_list() {
        assert_eq!(bounded_cursor(0, 0, true), 0);
        assert_eq!(bounded_cursor(0, 2, false), 0);
        assert_eq!(bounded_cursor(0, 2, true), 1);
        assert_eq!(bounded_cursor(1, 2, true), 1);
    }
}
