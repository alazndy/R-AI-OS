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
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if self.store.current_route != Route::Now {
                    self.store.last_error = Some("Approvals are available only on NOW.".into());
                } else if let Some(approval_id) = self
                    .store
                    .snapshot
                    .now
                    .approvals
                    .get(self.store.cursor)
                    .map(|approval| approval.id.clone())
                {
                    let command = Command::ApproveHandoff {
                        idempotency_key: format!("approve-{approval_id}"),
                        approval_id,
                    };
                    if let Err(problem) = self.client.send_command(command) {
                        self.store.last_error = Some(problem.message);
                    }
                } else {
                    self.store.last_error = Some("No approval selected.".into());
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.store.current_route == Route::Govern {
                    if let Some(job_id) = self
                        .store
                        .snapshot
                        .govern
                        .cron_jobs
                        .get(self.store.cursor)
                        .map(|job| job.id.clone())
                    {
                        let command = Command::TriggerCronJob {
                            idempotency_key: format!("trigger-{job_id}"),
                            job_id,
                        };
                        if let Err(problem) = self.client.send_command(command) {
                            self.store.last_error = Some(problem.message);
                        }
                    } else {
                        self.store.last_error = Some("No cron job selected.".into());
                    }
                } else if self.store.current_route == Route::Now {
                    if let Some(approval_id) = self
                        .store
                        .snapshot
                        .now
                        .approvals
                        .get(self.store.cursor)
                        .map(|approval| approval.id.clone())
                    {
                        let command = Command::RejectHandoff {
                            idempotency_key: format!("reject-{approval_id}"),
                            approval_id,
                            reason: "Rejected by TUI user".into(),
                        };
                        if let Err(problem) = self.client.send_command(command) {
                            self.store.last_error = Some(problem.message);
                        }
                    } else {
                        self.store.last_error = Some("No approval selected.".into());
                    }
                } else {
                    self.store.last_error = Some("Reject is available only on NOW.".into());
                }
            }
            KeyCode::Char('g') => {
                if let Err(problem) = self.client.send_query(Query::GetSystemSnapshot) {
                    self.store.last_error = Some(problem.message);
                }
            }
            KeyCode::Esc => {
                if self.store.last_error.take().is_none() {
                    self.set_control_focus(false);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
