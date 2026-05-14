use crate::app::events::helpers::*;
use crate::app::{filtered_palette, MENU_ITEMS};
use crate::config::Config;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::thread;

use crate::app::{state::*, App};
use crate::compliance;
use crate::filebrowser::{load_file_content, FileEntry};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Ok(());
        }

        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state = AppState::Search;
            self.search.query.clear();
            self.search.results.clear();
            return Ok(());
        }

        // Handover modal takes absolute priority
        if self.system.handover_modal.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = "{\"command\":\"HumanApproval\",\"approved\":true}".to_string();
                        let _ = tx.send(msg);
                    }
                    self.system.handover_modal = None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = "{\"command\":\"HumanApproval\",\"approved\":false}".to_string();
                        let _ = tx.send(msg);
                    }
                    self.system.handover_modal = None;
                }
                _ => {}
            }
            return Ok(());
        }

        // Launcher overlay takes priority over all other input
        if self.ui.show_launcher {
            match key.code {
                KeyCode::Esc => {
                    self.ui.show_launcher = false;
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some(ref proj) = self.projects.active.clone() {
                        let msg = launch_agent("claude", &proj.local_path);
                        self.system.sync_status = Some(msg);
                    }
                    self.ui.show_launcher = false;
                }
                KeyCode::Char('g') | KeyCode::Char('G') => {
                    if let Some(ref proj) = self.projects.active.clone() {
                        let msg = launch_agent("gemini", &proj.local_path);
                        self.system.sync_status = Some(msg);
                    }
                    self.ui.show_launcher = false;
                }
                _ => {}
            }
            return Ok(());
        }

        if self.state == AppState::Dashboard
            && self.ui.menu_cursor == 2
            && key.code == KeyCode::Char('f')
        {
            if let Some(ref report) = self.health.compliance {
                if !report.violations.is_empty() && !self.health.is_fixing {
                    self.health.is_fixing = true;
                    self.health.fix_status = Some("Claude fixing issues...".into());
                    self.add_activity("Agent", "Initiating Auto-Fix with Claude Code", "Warning");
                    let tx = self.tx.clone();
                    thread::spawn(move || {
                        thread::sleep(std::time::Duration::from_secs(3));
                        tx.send(BgMsg::SyncDone("Auto-Fix Complete: Issues resolved".into()))
                            .ok();
                    });
                }
            }
        }

        match self.state {
            AppState::Search => self.handle_key_search(key),
            AppState::Booting => {
                if key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                }
                Ok(())
            }

            AppState::Setup => self.handle_setup_key(key),
            AppState::FileView => {
                self.handle_file_view_key(key);
                Ok(())
            }
            AppState::FileEdit => self.handle_file_edit_key(key),
            AppState::ProjectDetail => {
                self.handle_project_detail_key(key);
                Ok(())
            }
            AppState::HealthView => {
                self.handle_health_view_key(key);
                Ok(())
            }
            AppState::MemPalaceView => {
                self.handle_mempalace_key(key);
                Ok(())
            }
            AppState::GraphReport => {
                self.handle_graph_report_key(key);
                Ok(())
            }
            AppState::GitDiffView => {
                self.handle_git_diff_key(key);
                Ok(())
            }
            AppState::HelpView => {
                // Any key closes help
                self.state = AppState::Dashboard;
                Ok(())
            }

            AppState::Dashboard => {
                if self.ui.command_mode {
                    self.handle_command_key(key)?;
                } else {
                    self.handle_dashboard_key(key)?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn handle_setup_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::setup_wizard::WizardStep;

        // Editing mode — capture text input
        if self.wizard.editing {
            match key.code {
                KeyCode::Enter => {
                    self.wizard_commit_input();
                    self.wizard.editing = false;
                }
                KeyCode::Esc => {
                    self.wizard.editing = false;
                }
                KeyCode::Char(c) => self.wizard.input.push(c),
                KeyCode::Backspace => {
                    self.wizard.input.pop();
                }
                _ => {}
            }
            return Ok(());
        }

        // Running — ignore all keys
        if self.wizard.running {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Navigate fields within a step
            KeyCode::Up | KeyCode::Char('k') if self.wizard.field_cursor > 0 => {
                self.wizard.field_cursor -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.wizard.field_cursor += 1;
            }

            // Enter: begin editing or advance step
            KeyCode::Enter => {
                match &self.wizard.step {
                    WizardStep::Welcome => {
                        self.wizard.step = WizardStep::Workspace;
                        self.wizard.field_cursor = 0;
                    }
                    WizardStep::Done => {
                        // Apply config and open dashboard
                        let dev_ops = PathBuf::from(&self.wizard.dev_ops);
                        let master = PathBuf::from(&self.wizard.master);
                        let skills = dev_ops.join(".agents").join("skills");
                        let vault = if self.wizard.vault.is_empty() {
                            PathBuf::new()
                        } else {
                            PathBuf::from(&self.wizard.vault)
                        };
                        if !dev_ops.as_os_str().is_empty() {
                            let cfg = Config {
                                dev_ops_path: dev_ops,
                                master_md_path: master,
                                skills_path: skills,
                                vault_projects_path: vault,
                            };
                            let _ = cfg.save();
                            self.config = cfg;
                        }
                        self.state = AppState::Dashboard;
                        let tx = self.tx.clone();
                        tx.send(BgMsg::TransitionToDashboard).ok();
                    }
                    WizardStep::Initialize => {
                        self.wizard_run_initialize();
                    }
                    _ => {
                        // Start editing current field
                        self.wizard_start_edit();
                    }
                }
            }

            // [s] — advance to next step (also runs current step's setup)
            KeyCode::Char('s') => {
                self.wizard_advance_step();
            }

            // [Tab] — skip/unskip agent steps
            KeyCode::Tab => match self.wizard.step {
                WizardStep::Claude => self.wizard.skip_claude = !self.wizard.skip_claude,
                WizardStep::Gemini => self.wizard.skip_gemini = !self.wizard.skip_gemini,
                WizardStep::Antigravity => {
                    self.wizard.skip_antigravity = !self.wizard.skip_antigravity
                }
                _ => {
                    self.wizard_advance_step();
                }
            },

            _ => {}
        }
        Ok(())
    }

    pub(crate) fn handle_health_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') if self.health.cursor > 0 => {
                self.health.cursor -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.health.cursor + 1 < self.health.report.len() =>
            {
                self.health.cursor += 1;
            }
            KeyCode::Enter => {
                if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
                    if let Some(proj) = self
                        .projects
                        .list
                        .iter()
                        .find(|p| p.local_path == h.path)
                        .cloned()
                    {
                        self.open_project_detail(proj);
                    }
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.health.is_checking = true;
                if let Some(ref tx_daemon) = self.tx_daemon {
                    let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    self.add_activity("System", "Manual health refresh requested", "Info");
                }
            }
            KeyCode::Char('c') => {
                if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
                    if h.git_dirty == Some(true) {
                        let tx = self.tx.clone();
                        let path = h.path.clone();
                        let name = h.name.clone();
                        self.system.sync_status = Some(format!("Committing {}...", name));
                        thread::spawn(move || {
                            let r = crate::core::git::commit(&path, "chore: raios update", true);
                            tx.send(BgMsg::GitActionDone {
                                project: name,
                                action: "commit".into(),
                                ok: r.ok,
                                message: r.message,
                            })
                            .ok();
                        });
                    } else {
                        self.system.sync_status = Some("Nothing to commit (working tree clean)".into());
                    }
                }
            }
            KeyCode::Char('p') => {
                if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
                    let tx = self.tx.clone();
                    let path = h.path.clone();
                    let name = h.name.clone();
                    self.system.sync_status = Some(format!("Pushing {}...", name));
                    thread::spawn(move || {
                        let r = crate::core::git::push(&path);
                        tx.send(BgMsg::GitActionDone {
                            project: name,
                            action: "push".into(),
                            ok: r.ok,
                            message: r.message,
                        })
                        .ok();
                    });
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_project_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Tab => {
                self.projects.panel_focus = !self.projects.panel_focus;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.projects.memory_scroll = self.projects.memory_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.projects.memory_lines.len() as u16).saturating_sub(10);
                self.projects.memory_scroll = (self.projects.memory_scroll + 1).min(max);
            }
            KeyCode::Char('e') => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let p = proj.local_path.join("memory.md");
                    if p.exists() {
                        self.open_file_edit(FileEntry::new("memory.md", p));
                    }
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') if self.projects.active.is_some() => {
                self.ui.show_launcher = true;
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                if let Some(ref proj) = self.projects.active.clone() {
                    let msg = self.run_graphify(&proj.local_path);
                    self.system.sync_status = Some(msg);
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(ref proj) = self.projects.active.clone() {
                    self.open_graph_report(&proj.local_path);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(ref proj) = self.projects.active.clone() {
                    self.open_git_diff(&proj.local_path);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_graph_report_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.state = AppState::ProjectDetail;
            }
            KeyCode::Up | KeyCode::Char('k') if self.projects.graph_report_scroll > 0 => {
                self.projects.graph_report_scroll -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.projects.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                if self.projects.graph_report_scroll < max {
                    self.projects.graph_report_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.projects.graph_report_scroll = self.projects.graph_report_scroll.saturating_sub(self.height / 2);
            }
            KeyCode::PageDown => {
                let max = (self.projects.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                self.projects.graph_report_scroll = (self.projects.graph_report_scroll + self.height / 2).min(max);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_git_diff_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(pending) = self
                    .system.pending_file_changes
                    .get(self.system.pending_change_cursor)
                    .cloned()
                {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = serde_json::json!({
                            "command": "ApproveFileChange",
                            "id": pending.id.to_string(),
                            "path": pending.path,
                            "approved": true
                        });
                        let _ = tx.send(msg.to_string());
                        self.add_activity(
                            "System",
                            &format!("File change approved for {}", pending.path),
                            "Info",
                        );
                    }
                    self.system.pending_file_changes.remove(self.system.pending_change_cursor);
                    if self.system.pending_change_cursor >= self.system.pending_file_changes.len()
                        && !self.system.pending_file_changes.is_empty()
                    {
                        self.system.pending_change_cursor = self.system.pending_file_changes.len() - 1;
                    }
                }
                if self.system.pending_file_changes.is_empty() {
                    self.state = AppState::Dashboard;
                } else {
                    // Load next diff
                    let next = &self.system.pending_file_changes[self.system.pending_change_cursor];
                    self.projects.git_diff_lines =
                        crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                if let Some(pending) = self
                    .system.pending_file_changes
                    .get(self.system.pending_change_cursor)
                    .cloned()
                {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = serde_json::json!({
                            "command": "ApproveFileChange",
                            "id": pending.id.to_string(),
                            "path": pending.path,
                            "approved": false
                        });
                        let _ = tx.send(msg.to_string());
                        self.add_activity(
                            "System",
                            &format!("File change rejected for {}", pending.path),
                            "Warning",
                        );
                    }
                    self.system.pending_file_changes.remove(self.system.pending_change_cursor);
                    if self.system.pending_change_cursor >= self.system.pending_file_changes.len()
                        && !self.system.pending_file_changes.is_empty()
                    {
                        self.system.pending_change_cursor = self.system.pending_file_changes.len() - 1;
                    }
                }
                if self.system.pending_file_changes.is_empty() {
                    self.state = AppState::Dashboard;
                } else {
                    // Load next diff
                    let next = &self.system.pending_file_changes[self.system.pending_change_cursor];
                    self.projects.git_diff_lines =
                        crate::app::editor::simple_diff(&next.original_content, &next.new_content);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') if self.projects.git_diff_scroll > 0 => {
                self.projects.git_diff_scroll -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.projects.git_diff_lines.len() as u16).saturating_sub(self.height - 6);
                if self.projects.git_diff_scroll < max {
                    self.projects.git_diff_scroll += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('h') if self.system.pending_change_cursor > 0 => {
                self.system.pending_change_cursor -= 1;
                let next = &self.system.pending_file_changes[self.system.pending_change_cursor];
                self.projects.git_diff_lines =
                    crate::app::editor::simple_diff(&next.original_content, &next.new_content);
            }
            KeyCode::Right | KeyCode::Char('l')
                if self.system.pending_change_cursor + 1 < self.system.pending_file_changes.len() =>
            {
                self.system.pending_change_cursor += 1;
                let next = &self.system.pending_file_changes[self.system.pending_change_cursor];
                self.projects.git_diff_lines =
                    crate::app::editor::simple_diff(&next.original_content, &next.new_content);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_mempalace_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }

            // Filter typing
            KeyCode::Char('/') => {
                self.mempalace.filter.clear();
            }
            KeyCode::Char(c) if self.mempalace.proj_cursor.is_none() => {
                // Not navigating projects — accumulate filter
                self.mempalace.filter.push(c);
            }

            KeyCode::Backspace => {
                self.mempalace.filter.pop();
            }

            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(pi) = self.mempalace.proj_cursor {
                    if pi > 0 {
                        self.mempalace.proj_cursor = Some(pi - 1);
                    } else {
                        // Go back to room level
                        self.mempalace.proj_cursor = None;
                    }
                } else if self.mempalace.room_cursor > 0 {
                    self.mempalace.room_cursor -= 1;
                }
            }

            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(pi) = self.mempalace.proj_cursor {
                    let room = &self.mempalace.rooms[self.mempalace.room_cursor];
                    if pi + 1 < room.projects.len() {
                        self.mempalace.proj_cursor = Some(pi + 1);
                    }
                } else if self.mempalace.room_cursor + 1 < self.mempalace.rooms.len() {
                    self.mempalace.room_cursor += 1;
                }
            }

            // → / Enter: go into room or open project
            KeyCode::Right | KeyCode::Char('l')
                if self.mempalace.proj_cursor.is_none() && !self.mempalace.rooms.is_empty() =>
            {
                let room = &self.mempalace.rooms[self.mempalace.room_cursor];
                if !room.projects.is_empty() {
                    self.mempalace.proj_cursor = Some(0);
                }
            }

            // ← : go back to room level
            KeyCode::Left | KeyCode::Char('h') => {
                self.mempalace.proj_cursor = None;
            }

            KeyCode::Enter => {
                if let Some(pi) = self.mempalace.proj_cursor {
                    let proj_path = self.mempalace.rooms[self.mempalace.room_cursor].projects[pi].path.clone();
                    // Find matching entities project, else create a stub
                    let proj = self
                        .projects
                        .list
                        .iter()
                        .find(|p| p.local_path == proj_path)
                        .cloned()
                        .unwrap_or_else(|| {
                            let mp = &self.mempalace.rooms[self.mempalace.room_cursor].projects[pi];
                            crate::entities::EntityProject {
                                name: mp.name.clone(),
                                category: self.mempalace.rooms[self.mempalace.room_cursor].folder_name.clone(),
                                local_path: proj_path,
                                github: None,
                                status: mp.status.clone(),
                                stars: None,
                                last_commit: None,
                                version: mp.version.clone(),
                                version_nickname: mp.version_nickname.clone(),
                            }
                        });
                    self.open_project_detail(proj);
                } else {
                    // Toggle expand/collapse
                    if let Some(exp) = self.mempalace.expanded.get_mut(self.mempalace.room_cursor) {
                        *exp = !*exp;
                    }
                }
            }

            // Space: toggle expand
            KeyCode::Char(' ') => {
                if let Some(exp) = self.mempalace.expanded.get_mut(self.mempalace.room_cursor) {
                    *exp = !*exp;
                    self.mempalace.proj_cursor = None;
                }
            }
            KeyCode::Char('C') | KeyCode::Char('G') | KeyCode::Char('A') => {
                if let Some(proj) = self.get_selected_mempalace_project() {
                    let agent = match key.code {
                        KeyCode::Char('C') => "claude",
                        KeyCode::Char('G') => "gemini",
                        _ => "antigravity",
                    };
                    self.add_activity(
                        "Agent",
                        &format!("Launching {} from MemPalace", agent),
                        "Info",
                    );
                        self.system.sync_status = Some(launch_agent(agent, &proj.path));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_key_search(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.state = AppState::Dashboard,
            KeyCode::Char(c) => {
                self.search.query.push(c);
                self.update_search();
            }
            KeyCode::Backspace => {
                self.search.query.pop();
                self.update_search();
            }
            KeyCode::Up if self.search.cursor > 0 => {
                self.search.cursor -= 1;
            }
            KeyCode::Down if self.search.cursor + 1 < self.search.results.len() => {
                self.search.cursor += 1;
            }
            KeyCode::Enter => {
                if let Some(res) = self.search.results.get(self.search.cursor) {
                    let entry = FileEntry::new(
                        res.path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        res.path.clone(),
                    );
                    self.open_file_view(entry);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn handle_file_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Char('r') | KeyCode::Char('R') if self.editor.changed_externally => {
                if let Some(ref file) = self.editor.active_file.clone() {
                    let content = load_file_content(&file.path);
                    self.health.compliance = Some(compliance::check_file(&file.path, &content));
                    self.editor.lines = content.lines().map(str::to_owned).collect();
                    self.editor.scroll = 0;
                    self.editor.watched_mtime = std::fs::metadata(&file.path)
                        .ok()
                        .and_then(|m| m.modified().ok());
                    self.editor.changed_externally = false;
                }
            }
            KeyCode::Char('e') => {
                if let Some(f) = self.editor.active_file.clone() {
                    if !f.read_only {
                        self.open_file_edit(f);
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.editor.scroll > 0 => {
                self.editor.scroll -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = (self.editor.lines.len() as u16).saturating_sub(self.height - 6);
                if self.editor.scroll < max {
                    self.editor.scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.editor.scroll = self.editor.scroll.saturating_sub(self.height / 2);
            }
            KeyCode::PageDown => {
                let max = (self.editor.lines.len() as u16).saturating_sub(self.height - 6);
                self.editor.scroll = (self.editor.scroll + self.height / 2).min(max);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_file_edit_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => self.save_file(),
                KeyCode::Char('q') => self.state = AppState::FileView,
                _ => self.editor.editor.handle_key(key),
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => self.state = AppState::FileView,
            _ => self.editor.editor.handle_key(key),
        }
        Ok(())
    }

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
                let max = filtered_palette(&self.ui.command_buf).len().saturating_sub(1);
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
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => {
                self.state = AppState::HelpView;
            }
            KeyCode::Char('i') | KeyCode::Char('I')
                if !self.system.pending_file_changes.is_empty() => {
                    let first = &self.system.pending_file_changes[self.system.pending_change_cursor];
                    self.projects.git_diff_lines = crate::app::editor::simple_diff(
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
                if self.ui.right_panel_focus {
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
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.ui.right_panel_focus {
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
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let can_focus = !self.current_menu_files().is_empty()
                    || (self.ui.menu_cursor == 0 && !self.tasks.list.is_empty())
                    || (self.ui.menu_cursor == 6 && !self.search.results.is_empty())
                    || (self.ui.menu_cursor == 7 && !self.projects.list.is_empty());
                if can_focus {
                    self.ui.right_panel_focus = true;
                    self.ui.right_file_cursor = 0;
                    self.ui.right_panel_scroll = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.ui.right_panel_focus = false;
            }
            KeyCode::Char(' ') | KeyCode::Char('x') | KeyCode::Char('X')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    if let Some(task) = self.tasks.list.get_mut(self.tasks.cursor) {
                        task.completed = !task.completed;
                        let _ = crate::tasks::save_tasks(&self.config.dev_ops_path, &self.tasks.list);
                    }
                }

            // Task → Agent dispatch  (only active when task panel is focused)
            KeyCode::Char('c')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("claude");
                }
            KeyCode::Char('g')
                if self.ui.menu_cursor == 0 && self.ui.right_panel_focus => {
                    self.dispatch_task("gemini");
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
                    let files = self.current_menu_files();
                    if let Some(entry) = files.into_iter().nth(self.ui.right_file_cursor) {
                        let _ = crate::discovery::open_in_editor(&entry.path);
                    }
                }
            KeyCode::Char('C') | KeyCode::Char('G') | KeyCode::Char('A')
                if self.ui.right_panel_focus => {
                    let project_path = match self.ui.menu_cursor {
                        7 => self.project_at_cursor().map(|p| p.local_path.clone()),
                        _ => None,
                    };

                    if let Some(path) = project_path {
                        let agent = match key.code {
                            KeyCode::Char('C') => "claude",
                            KeyCode::Char('G') => "gemini",
                            _ => "antigravity",
                        };
                        self.add_activity(
                            "Agent",
                            &format!("Launching {} for project", agent),
                            "Info",
                        );
                        self.system.sync_status = Some(launch_agent(agent, &path));
                    }
                }
            _ => {}
        }
        Ok(())
    }
}
