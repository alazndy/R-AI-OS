use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::app::intent::Intent;
use crate::app::reducer::reduce_intent;
use crate::app::route::{dashboard_header_height, Route, LAUNCHER_HEIGHT, TABS_HEIGHT};
use crate::app::state::AppState;
use crate::app::{filtered_palette, App};

impl App {
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

        if self.ui.command_mode {
            self.handle_command_palette_mouse(mouse);
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
                let (row, right_panel_focus) = match self.store.current_route {
                    Route::Now => (
                        mouse.row.saturating_sub(content_top + 1) as usize,
                        mouse.column >= self.width.saturating_mul(60) / 100,
                    ),
                    Route::Work => (
                        mouse.row.saturating_sub(content_top + 1) as usize,
                        mouse.column >= self.width.saturating_mul(40) / 100,
                    ),
                    Route::Explore => {
                        let traces_end =
                            content_top + 3 + launcher_top.saturating_sub(content_top + 3) / 2;
                        if mouse.row < traces_end {
                            (mouse.row.saturating_sub(content_top + 4) as usize, false)
                        } else {
                            (mouse.row.saturating_sub(traces_end + 1) as usize, true)
                        }
                    }
                    Route::Govern => (
                        mouse.row.saturating_sub(content_top + 1) as usize,
                        mouse.column >= self.width / 2,
                    ),
                };
                self.select_control_row(row, right_panel_focus);
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
