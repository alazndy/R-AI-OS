use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::{filtered_palette, App, MENU_ITEMS};
use raios_runtime::filebrowser::FileEntry;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub(crate) fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.ui.command_mode = false;
                self.ui.command_buf.clear();
                self.ui.palette_cursor = 0;
            }

            KeyCode::Enter => {
                // If user typed a command (buf starts with '/'), run it directly.
                // Otherwise use the palette cursor to pick a command.
                let cmd = if self.ui.command_buf.starts_with('/') {
                    self.ui.command_buf.trim().to_string()
                } else {
                    let filtered = filtered_palette(&self.ui.command_buf);
                    filtered
                        .get(self.ui.palette_cursor)
                        .map(|item| item.cmd.to_string())
                        .unwrap_or_default()
                };
                self.ui.command_buf.clear();
                self.ui.command_mode = false;
                self.ui.palette_cursor = 0;
                if !cmd.is_empty() {
                    self.execute_command(&cmd)?;
                }
            }

            // Tab fills the selected palette item into the buffer
            KeyCode::Tab => {
                let filtered = filtered_palette(&self.ui.command_buf);
                if let Some(item) = filtered.get(self.ui.palette_cursor) {
                    self.ui.command_buf = format!("{} ", item.cmd);
                    self.ui.palette_cursor = 0;
                }
            }

            KeyCode::Up if self.ui.palette_cursor > 0 => {
                self.ui.palette_cursor -= 1;
            }

            KeyCode::Down => {
                let max = filtered_palette(&self.ui.command_buf)
                    .len()
                    .saturating_sub(1);
                if self.ui.palette_cursor < max {
                    self.ui.palette_cursor += 1;
                }
            }

            KeyCode::Backspace => {
                if self.ui.command_buf.is_empty() {
                    self.ui.command_mode = false;
                } else {
                    self.ui.command_buf.pop();
                    self.ui.palette_cursor = 0;
                }
            }

            KeyCode::Char(c) => {
                self.ui.command_buf.push(c);
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

        // The four control-plane routes own dashboard interaction. Keep only
        // global dashboard affordances from the legacy surface while its
        // detailed screens are retired behind the command palette.
        if !matches!(key.code, KeyCode::Char('q' | '?' | '/')) {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => {
                self.state = AppState::HelpView;
            }
            KeyCode::Char('i') | KeyCode::Char('I')
                if !self.system.pending_file_changes.is_empty() => {
                    let first = &self.system.pending_file_changes[self.system.pending_change_cursor];
                    self.projects.git_diff_lines = raios_surface_tui::app::editor::simple_diff(
                        &first.original_content,
                        &first.new_content,
                    );
                    self.state = AppState::GitDiffView;
                }
            KeyCode::Char('L')
                // Uppercase L = launcher (lowercase l = vim right)
                if self.ui.menu_cursor == 7 && self.ui.right_panel_focus => {
                    if let Some(proj) = self.project_at_cursor().cloned() {
                        self.projects.active = Some(proj);
                        self.ui.show_launcher = true;
                    }
                }
            KeyCode::Char('s')
                // Cycle sort mode on All Projects view
                if self.ui.menu_cursor == 7 => {
                    self.projects.sort = self.projects.sort.next();
                    self.projects.cursor = 0;
                }
            // ── Extensions panel keyboard (menu_cursor == 15) ────────────────
            // All Extensions-specific keys require right_panel_focus, same as every
            // other panel — otherwise Up/Down/Tab/etc. hijack the menu-navigation
            // keys and the user gets stuck unable to move off the panel.
            KeyCode::Tab if self.ui.menu_cursor == 15 && self.ui.right_panel_focus => {
                self.ext.focus = if self.ext.focus == raios_surface_tui::app::state::ExtFocus::Commands {
                    raios_surface_tui::app::state::ExtFocus::Config
                } else {
                    raios_surface_tui::app::state::ExtFocus::Commands
                };
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 15
                    && self.ui.right_panel_focus
                    && self.ext.focus == raios_surface_tui::app::state::ExtFocus::Commands
                    && !self.ext.extensions.is_empty() =>
            {
                self.run_ext_cmd();
            }
            KeyCode::Char('e')
                if self.ui.menu_cursor == 15
                    && self.ui.right_panel_focus
                    && self.ext.focus == raios_surface_tui::app::state::ExtFocus::Config
                    && !self.ext.editing =>
            {
                if let Some(ext) = self.ext.extensions.get(self.ext.ext_cursor) {
                    if let Some(field) = ext.config_schema.get(self.ext.cfg_cursor) {
                        self.ext.input = if field.masked { String::new() } else { field.value.clone() };
                        self.ext.editing = true;
                    }
                }
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 15
                    && self.ui.right_panel_focus
                    && self.ext.focus == raios_surface_tui::app::state::ExtFocus::Config
                    && self.ext.editing =>
            {
                self.save_ext_config_field();
            }
            KeyCode::Esc if self.ui.menu_cursor == 15 && self.ext.editing => {
                self.ext.editing = false;
                self.ext.input.clear();
            }
            KeyCode::Char(c) if self.ui.menu_cursor == 15 && self.ext.editing => {
                self.ext.input.push(c);
            }
            KeyCode::Backspace if self.ui.menu_cursor == 15 && self.ext.editing => {
                self.ext.input.pop();
            }
            KeyCode::Left
                if self.ui.menu_cursor == 15
                    && self.ui.right_panel_focus
                    && !self.ext.editing
                    && self.ext.ext_cursor > 0 =>
            {
                self.ext.ext_cursor -= 1;
                self.ext.cmd_cursor = 0;
                self.ext.cfg_cursor = 0;
            }
            KeyCode::Right
                if self.ui.menu_cursor == 15
                    && self.ui.right_panel_focus
                    && !self.ext.editing
                    && self.ext.ext_cursor + 1 < self.ext.extensions.len() =>
            {
                self.ext.ext_cursor += 1;
                self.ext.cmd_cursor = 0;
                self.ext.cfg_cursor = 0;
            }
            // Constitution item-editing Esc cancels the inline edit/insert first —
            // must precede the generic "back out of panel" Esc arm below, same
            // precedence pattern as the Extensions ext.editing Esc arm above.
            KeyCode::Esc if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_editing = false;
                self.constitution.item_input.clear();
            }
            // Creator-mode Esc closes the creator at any step — must precede the generic
            // "back out of panel" Esc arm below for the same reason as the item-editing Esc
            // arm just above (same precedence bug/fix Task 7 documented): that catch-all is
            // guarded only by `right_panel_focus`, which is always true while the creator is
            // active, so it would otherwise swallow every Esc before this arm is ever reached.
            KeyCode::Esc if self.ui.menu_cursor == 1 && self.constitution.creator.active => {
                self.close_creator();
            }
            // Esc always backs out of a focused right panel to the menu — universal
            // "go back" regardless of which panel (Extensions, Tasks, files, ...).
            KeyCode::Esc if self.ui.right_panel_focus => {
                self.ui.right_panel_focus = false;
            }
            // ── End Extensions ───────────────────────────────────────────────
            // ── Constitution panel keyboard (menu_cursor == 1) ───────────────
            KeyCode::Char(n @ '1'..='9')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                let idx = (n as usize) - ('1' as usize);
                if idx < self.constitution.tabs.len() {
                    self.load_constitution_tab(idx);
                }
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                if self.constitution.outline_cursor > 0 {
                    self.constitution.outline_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                let max = self.constitution.rows.len().saturating_sub(1);
                if self.constitution.outline_cursor < max {
                    self.constitution.outline_cursor += 1;
                }
            }
            KeyCode::Char('r') | KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active
                    && !self.constitution.sections.is_empty() =>
            {
                self.jump_to_constitution_raw_edit();
            }
            KeyCode::Char('i')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.begin_item_edit();
            }
            KeyCode::Char('n')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.begin_item_insert();
            }
            KeyCode::Char('d')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.delete_item_at_cursor();
            }
            KeyCode::Char('c')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.open_creator();
            }
            KeyCode::Char('p')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(false);
            }
            KeyCode::Char('g')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(true);
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.step = raios_surface_tui::app::state::CreatorStep::Preview;
            }
            KeyCode::Char(ch)
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.push(ch);
            }
            KeyCode::Backspace
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.pop();
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Preview =>
            {
                // Global writes get an extra explicit y/N gate below — the design's
                // "single file every agent reads" deserves more than a plain Enter.
                if self.constitution.creator.target_is_global {
                    self.constitution.creator.step =
                        raios_surface_tui::app::state::CreatorStep::ConfirmGlobal;
                } else {
                    self.creator_confirm_save();
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ConfirmGlobal =>
            {
                self.creator_confirm_save();
            }
            KeyCode::Char('n') | KeyCode::Char('N')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ConfirmGlobal =>
            {
                self.constitution.creator.step = raios_surface_tui::app::state::CreatorStep::Preview;
            }
            KeyCode::Enter if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.commit_item_edit();
            }
            KeyCode::Char(c) if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_input.push(c);
            }
            KeyCode::Backspace if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_input.pop();
            }
            // ── End Constitution ──────────────────────────────────────────────
            KeyCode::Char('/') | KeyCode::Tab => {
                self.ui.command_mode = true;
                self.ui.palette_cursor = 0;
                // '/' starts with a slash so typed commands work; Tab shows full palette
                if key.code == KeyCode::Char('/') {
                    self.ui.command_buf = "/".into();
                } else {
                    self.ui.command_buf.clear();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.ui.menu_cursor == 15 && self.ui.right_panel_focus && !self.ext.editing {
                    if self.ext.focus == raios_surface_tui::app::state::ExtFocus::Commands {
                        if self.ext.cmd_cursor > 0 { self.ext.cmd_cursor -= 1; }
                    } else if self.ext.cfg_cursor > 0 {
                        self.ext.cfg_cursor -= 1;
                    }
                } else if self.ui.right_panel_focus {
                    match self.ui.menu_cursor {
                        0 => {
                            if self.tasks.cursor > 0 {
                                self.tasks.cursor -= 1;
                            }
                        }
                        6 => {
                            if self.search.cursor > 0 {
                                self.search.cursor -= 1;
                            }
                        }
                        7 => {
                            if self.projects.cursor > 0 {
                                self.projects.cursor -= 1;
                            }
                        }
                        _ => {
                            if self.ui.right_file_cursor > 0 {
                                self.ui.right_file_cursor -= 1;
                            }
                        }
                    }
                } else if self.ui.menu_cursor > 0 {
                    self.ui.menu_cursor -= 1;
                    self.ui.right_file_cursor = 0;
                    self.projects.cursor = 0;
                    self.search.cursor = 0;
                    self.ui.right_panel_scroll = 0;
                    self.ui.right_panel_focus = false;
                    if self.ui.menu_cursor == 1 && self.constitution.tabs.is_empty() {
                        self.refresh_constitution_tabs();
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.ui.menu_cursor == 15 && self.ui.right_panel_focus && !self.ext.editing {
                    if self.ext.focus == raios_surface_tui::app::state::ExtFocus::Commands {
                        let max = self.ext.extensions.get(self.ext.ext_cursor)
                            .map(|e| e.commands.len().saturating_sub(1))
                            .unwrap_or(0);
                        if self.ext.cmd_cursor < max { self.ext.cmd_cursor += 1; }
                    } else {
                        let max = self.ext.extensions.get(self.ext.ext_cursor)
                            .map(|e| e.config_schema.len().saturating_sub(1))
                            .unwrap_or(0);
                        if self.ext.cfg_cursor < max { self.ext.cfg_cursor += 1; }
                    }
                } else if self.ui.right_panel_focus {
                    match self.ui.menu_cursor {
                        0 => {
                            let max = self.tasks.list.len().saturating_sub(1);
                            if self.tasks.cursor < max {
                                self.tasks.cursor += 1;
                            }
                        }
                        6 => {
                            let max = self.search.results.len().saturating_sub(1);
                            if self.search.cursor < max {
                                self.search.cursor += 1;
                            }
                        }
                        7 => {
                            let max = self.projects.list.len().saturating_sub(1);
                            if self.projects.cursor < max {
                                self.projects.cursor += 1;
                            }
                        }
                        _ => {
                            let max = self.current_menu_files().len().saturating_sub(1);
                            if self.ui.right_file_cursor < max {
                                self.ui.right_file_cursor += 1;
                            }
                        }
                    }
                } else if self.ui.menu_cursor < MENU_ITEMS.len() - 1 {
                    self.ui.menu_cursor += 1;
                    self.ui.right_file_cursor = 0;
                    self.projects.cursor = 0;
                    self.search.cursor = 0;
                    self.ui.right_panel_scroll = 0;
                    self.ui.right_panel_focus = false;
                    // Lazy-load extensions when navigating to the Extensions panel
                    if self.ui.menu_cursor == 15 && !self.ext.loaded {
                        self.load_extensions();
                    }
                    if self.ui.menu_cursor == 1 && self.constitution.tabs.is_empty() {
                        self.refresh_constitution_tabs();
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let can_focus = !self.current_menu_files().is_empty()
                    || (self.ui.menu_cursor == 0 && !self.tasks.list.is_empty())
                    || (self.ui.menu_cursor == 1 && !self.constitution.tabs.is_empty())
                    || (self.ui.menu_cursor == 6 && !self.search.results.is_empty())
                    || (self.ui.menu_cursor == 7 && !self.projects.list.is_empty())
                    || (self.ui.menu_cursor == 15 && !self.ext.extensions.is_empty());
                if can_focus {
                    self.ui.right_panel_focus = true;
                    self.ui.right_file_cursor = 0;
                    self.ui.right_panel_scroll = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.ui.right_panel_focus = false;
            }
            KeyCode::Char(' ') | KeyCode::Char('v') | KeyCode::Char('V')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    if let Some(task) = self.tasks.list.get_mut(self.tasks.cursor) {
                        task.completed = !task.completed;
                        let _ = raios_runtime::tasks::save_tasks(&self.config.dev_ops_path, &self.tasks.list);
                    }
                }

            // Task → Agent dispatch  (only active when task panel is focused)
            KeyCode::Char('c')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("claude");
                }
            KeyCode::Char('x')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("codex");
                }
            KeyCode::Char('o')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("opencode");
                }
            KeyCode::Char('a')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("antigravity");
                }
            KeyCode::Enter
                if self.ui.right_panel_focus => {
                    match self.ui.menu_cursor {
                        6 => {
                            if let Some(result) = self.search.results.get(self.search.cursor) {
                                let name = result
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned();
                                self.open_file_view(FileEntry::new(name, result.path.clone()));
                            }
                        }
                        7 => {
                            if let Some(proj) = self.project_at_cursor().cloned() {
                                self.open_project_detail(proj);
                            }
                        }
                        _ => {
                            let files = self.current_menu_files();
                            if let Some(entry) = files.into_iter().nth(self.ui.right_file_cursor) {
                                self.open_file_view(entry);
                            }
                        }
                    }
                }
            KeyCode::Char('e')
                if self.ui.right_panel_focus => {
                    let files = self.current_menu_files();
                    if let Some(entry) = files.into_iter().nth(self.ui.right_file_cursor) {
                        if !entry.read_only {
                            self.open_file_edit(entry);
                        }
                    }
                }
            KeyCode::Char('o')
                if self.ui.right_panel_focus => {
                    self.open_current_file_in_editor();
                }
            KeyCode::Char('C') | KeyCode::Char('O') | KeyCode::Char('A')
                if self.ui.right_panel_focus => {
                    let agent = match key.code {
                        KeyCode::Char('C') => "claude",
                        KeyCode::Char('O') => "opencode",
                        _ => "antigravity",
                    };
                    self.launch_agent_for_selected_project(agent);
                }
            _ => {}
        }
        Ok(())
    }

    /// Handles the live control-plane dashboard before legacy dashboard shortcuts.
    /// Returning `true` consumes the key so the retired 16-item menu cannot mutate
    /// state behind the four typed routes.
    fn handle_control_dashboard_key(&mut self, key: KeyEvent) -> Result<bool> {
        use crate::app::intent::Intent;
        use crate::app::reducer::reduce_intent;
        use crate::app::route::Route;
        use raios_contracts::{Command, Query};

        match key.code {
            KeyCode::Char('1') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Now)),
            KeyCode::Char('2') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Work)),
            KeyCode::Char('3') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Explore)),
            KeyCode::Char('4') => reduce_intent(&mut self.store, Intent::SwitchRoute(Route::Govern)),
            KeyCode::Tab => reduce_intent(&mut self.store, Intent::NextRoute),
            KeyCode::BackTab => reduce_intent(&mut self.store, Intent::PrevRoute),
            KeyCode::Up | KeyCode::Char('k') => reduce_intent(&mut self.store, Intent::CursorUp),
            KeyCode::Down | KeyCode::Char('j') => reduce_intent(&mut self.store, Intent::CursorDown),
            KeyCode::Left | KeyCode::Char('h') => reduce_intent(&mut self.store, Intent::CursorLeft),
            KeyCode::Right | KeyCode::Char('l') => reduce_intent(&mut self.store, Intent::CursorRight),
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
            KeyCode::Esc => self.store.last_error = None,
            _ => return Ok(false),
        }

        Ok(true)
    }
}
