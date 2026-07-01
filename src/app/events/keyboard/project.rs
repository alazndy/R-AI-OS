use crate::app::state::AppState;
use crate::app::App;
use crate::filebrowser::FileEntry;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
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
                self.run_graphify_on_active();
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
                let max =
                    (self.projects.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                if self.projects.graph_report_scroll < max {
                    self.projects.graph_report_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                self.projects.graph_report_scroll = self
                    .projects
                    .graph_report_scroll
                    .saturating_sub(self.height / 2);
            }
            KeyCode::PageDown => {
                let max =
                    (self.projects.graph_report_lines.len() as u16).saturating_sub(self.height - 6);
                self.projects.graph_report_scroll =
                    (self.projects.graph_report_scroll + self.height / 2).min(max);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_git_diff_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.handle_file_change_approval(true);
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.handle_file_change_approval(false);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') if self.projects.git_diff_scroll > 0 => {
                self.projects.git_diff_scroll -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max =
                    (self.projects.git_diff_lines.len() as u16).saturating_sub(self.height - 6);
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
                if self.system.pending_change_cursor + 1
                    < self.system.pending_file_changes.len() =>
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
                    let proj_path = self.mempalace.rooms[self.mempalace.room_cursor].projects[pi]
                        .path
                        .clone();
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
                                category: self.mempalace.rooms[self.mempalace.room_cursor]
                                    .folder_name
                                    .clone(),
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
            KeyCode::Char('C') | KeyCode::Char('O') | KeyCode::Char('A') => {
                let agent = match key.code {
                    KeyCode::Char('C') => "claude",
                    KeyCode::Char('O') => "opencode",
                    _ => "antigravity",
                };
                self.launch_agent_from_mempalace(agent);
            }
            _ => {}
        }
    }
}
