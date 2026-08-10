//! Mouse event handling for tab clicks and list scrolling.

use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::intent::Intent;
use crate::app::operations::OperationPanel;
use crate::app::reducer::reduce_intent;
use crate::app::route::{dashboard_header_height, Route, LAUNCHER_HEIGHT, TABS_HEIGHT};
use crate::app::state::AppState;
use crate::app::{filtered_palette, App};
use crate::ui::routes::explore::explore_route_layout;
use crate::ui::routes::govern::{govern_cron_action_at, govern_route_layout, GovernCronAction};
use crate::ui::routes::work::{
    task_composer_action_at, work_route_layout, work_task_action_at, TaskComposerMouseAction,
    WorkTaskAction,
};

impl App {
    /// Handles mouse clicks and scroll wheel events in the TUI dashboard.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.state != AppState::Dashboard
            || self.ui.show_launcher
            || self.system.handover_modal.is_some()
            || self.constitution.pending_save.is_some()
        {
            return Ok(());
        }

        let header_height = dashboard_header_height(self.height);
        let tabs_top = header_height;
        let content_top = tabs_top + TABS_HEIGHT;
        let launcher_top = self.height.saturating_sub(LAUNCHER_HEIGHT);
        let content_area = Rect::new(
            0,
            content_top,
            self.width,
            launcher_top.saturating_sub(content_top),
        );

        if self.ui.command_mode {
            self.handle_command_palette_mouse(mouse);
            return Ok(());
        }

        if self.store.task_composer.is_open {
            self.handle_task_composer_mouse(mouse, content_area);
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
                if (tabs_top..tabs_top + TABS_HEIGHT.saturating_sub(1)).contains(&mouse.row) =>
            {
                if let Some(route) = Route::tab_at_column(mouse.column) {
                    reduce_intent(&mut self.store, Intent::SwitchRoute(route));
                }
            }
            MouseEventKind::Down(MouseButton::Left) if mouse.row >= launcher_top => {
                self.ui.command_mode = true;
                self.ui.command_buf.clear();
                self.ui.palette_cursor = 0;
            }
            MouseEventKind::Down(MouseButton::Left)
                if (content_top..launcher_top).contains(&mouse.row) =>
            {
                if self.store.current_route == Route::Now {
                    let action_top =
                        launcher_top.saturating_sub(crate::ui::routes::now::NOW_ACTIONS_HEIGHT);
                    if mouse.row >= action_top {
                        self.select_operation_action(
                            mouse.row.saturating_sub(action_top + 1) as usize
                        );
                    } else if mouse.row >= content_top + crate::ui::routes::now::NOW_SUMMARY_HEIGHT
                    {
                        let project_panel = mouse.column >= self.width / 2;
                        self.set_operation_panel(if project_panel {
                            OperationPanel::Project
                        } else {
                            OperationPanel::Attention
                        });
                        if !project_panel {
                            self.store.cursor = mouse.row.saturating_sub(
                                content_top + crate::ui::routes::now::NOW_SUMMARY_HEIGHT + 1,
                            ) as usize;
                            self.select_control_row(self.store.cursor, false);
                        }
                    }
                    return Ok(());
                }
                if self.store.current_route == Route::Work {
                    self.handle_work_mouse(mouse, content_area);
                    return Ok(());
                }
                if self.store.current_route == Route::Explore {
                    self.handle_explore_mouse(mouse, content_area);
                    return Ok(());
                }
                if self.store.current_route == Route::Govern {
                    self.handle_govern_mouse(mouse, content_area);
                    return Ok(());
                }
            }
            MouseEventKind::ScrollUp if (content_top..launcher_top).contains(&mouse.row) => {
                self.move_control_cursor(false);
            }
            MouseEventKind::ScrollDown if (content_top..launcher_top).contains(&mouse.row) => {
                self.move_control_cursor(true);
            }
            _ => {}
        }

        Ok(())
    }

    /// Handles visible WORK controls using the same geometry as the renderer.
    fn handle_work_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
            return;
        };
        let layout = work_route_layout(area);

        if let Some(action) = work_task_action_at(layout, mouse.column, mouse.row) {
            match action {
                WorkTaskAction::Create => self.begin_task_composer(),
                WorkTaskAction::SetInProgress => self.update_selected_work_task("in_progress"),
                WorkTaskAction::SetBlocked => self.update_selected_work_task("blocked"),
                WorkTaskAction::SetCompleted => self.update_selected_work_task("completed"),
            }
            return;
        }

        if rect_contains(layout.projects, mouse.column, mouse.row) {
            self.select_control_row(
                mouse
                    .row
                    .saturating_sub(layout.projects.y.saturating_add(1)) as usize,
                false,
            );
        } else if rect_contains(layout.tasks, mouse.column, mouse.row) {
            self.select_control_row(
                mouse.row.saturating_sub(layout.tasks.y.saturating_add(1)) as usize,
                true,
            );
        }
    }

    /// Keeps Explore click targets aligned with the renderer's shared layout.
    fn handle_explore_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
            return;
        };
        let layout = explore_route_layout(area);

        if rect_contains(layout.search, mouse.column, mouse.row) {
            self.store.explore_search.begin();
        } else if rect_contains(layout.results, mouse.column, mouse.row) {
            let row = mouse.row.saturating_sub(layout.results.y.saturating_add(1)) as usize;
            self.select_control_row(row, false);
        } else if rect_contains(layout.logs, mouse.column, mouse.row) {
            let row = mouse.row.saturating_sub(layout.logs.y.saturating_add(1)) as usize;
            self.select_control_row(row, true);
        }
    }

    /// Keeps GOVERN scheduler clicks aligned with the visible action strip.
    fn handle_govern_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
            return;
        };
        let layout = govern_route_layout(area);
        if let Some(action) = govern_cron_action_at(layout, mouse.column, mouse.row) {
            match action {
                GovernCronAction::Trigger => self.trigger_selected_cron_job(),
                GovernCronAction::TogglePause => self.toggle_selected_cron_job(),
            }
            return;
        }
        if rect_contains(layout.scheduler, mouse.column, mouse.row) {
            let row = mouse
                .row
                .saturating_sub(layout.scheduler.y.saturating_add(1))
                as usize;
            self.select_control_row(row, true);
        }
    }

    /// Handles only explicit composer buttons; typing remains keyboard-driven.
    fn handle_task_composer_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
            return;
        };
        match task_composer_action_at(area, mouse.column, mouse.row) {
            Some(TaskComposerMouseAction::DecreasePriority) => {
                self.store.task_composer.priority =
                    self.store.task_composer.priority.saturating_sub(10);
            }
            Some(TaskComposerMouseAction::IncreasePriority) => {
                self.store.task_composer.priority =
                    self.store.task_composer.priority.saturating_add(10);
            }
            Some(TaskComposerMouseAction::Cancel) => self.store.task_composer.cancel(),
            Some(TaskComposerMouseAction::Submit) => self.submit_task_composer(),
            None => {}
        }
    }

    fn handle_command_palette_mouse(&mut self, mouse: MouseEvent) {
        let palette = filtered_palette(&self.ui.command_buf);
        if palette.is_empty() {
            return;
        }

        let popup_height = (palette.len() as u16 + 2).min(13);
        let popup_width = 68u16.min(self.width.saturating_sub(4));
        let popup_x = self.width.saturating_sub(popup_width) / 2;
        let popup_y = self.height.saturating_sub(popup_height + 3);
        let inner_top = popup_y + 1;
        let inner_bottom = popup_y + popup_height.saturating_sub(1);
        let inside_popup = (popup_x..popup_x + popup_width).contains(&mouse.column)
            && (inner_top..inner_bottom).contains(&mouse.row);
        let visible_rows = popup_height.saturating_sub(2) as usize;
        let start = self
            .ui
            .palette_cursor
            .saturating_sub(visible_rows.saturating_sub(1));

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside_popup => {
                let selected = start + mouse.row.saturating_sub(inner_top) as usize;
                if selected < palette.len() {
                    self.ui.palette_cursor = selected;
                }
            }
            MouseEventKind::ScrollUp if inside_popup => {
                self.ui.palette_cursor = self.ui.palette_cursor.saturating_sub(1);
            }
            MouseEventKind::ScrollDown if inside_popup => {
                self.ui.palette_cursor = (self.ui.palette_cursor + 1).min(palette.len() - 1);
            }
            _ => {}
        }
    }
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}
