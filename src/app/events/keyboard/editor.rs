use crate::app::App;
use crate::app::state::AppState;
use crate::filebrowser::{load_file_content, FileEntry};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
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
                    self.health.compliance = Some(crate::compliance::check_file(&file.path, &content));
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
}
