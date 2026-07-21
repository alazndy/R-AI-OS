use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;

impl App {
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
                self.refresh_health();
            }
            KeyCode::Char('c') => {
                self.commit_project_at_health_cursor();
            }
            KeyCode::Char('p') => {
                self.push_project_at_health_cursor();
            }
            _ => {}
        }
    }
}
