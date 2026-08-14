use crossterm::event::{KeyCode, KeyEvent};
use raios_runtime::filebrowser::FileEntry;
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_search_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.state = AppState::Dashboard,
            KeyCode::Up | KeyCode::Char('k') if self.search.cursor > 0 => {
                self.search.cursor -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.search.cursor + 1 < self.search.results.len() =>
            {
                self.search.cursor += 1;
            }
            KeyCode::Enter => {
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
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_and_q_return_to_dashboard() {
        let mut app = App::test_instance();
        app.state = AppState::SearchView;
        app.handle_search_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::SearchView;
        app.handle_search_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn cursor_stays_within_bounds_on_empty_results() {
        let mut app = App::test_instance();
        app.handle_search_view_key(key(KeyCode::Down));
        assert_eq!(app.search.cursor, 0);
        app.handle_search_view_key(key(KeyCode::Up));
        assert_eq!(app.search.cursor, 0);
    }

    #[test]
    fn enter_on_empty_results_is_a_no_op() {
        let mut app = App::test_instance();
        app.state = AppState::SearchView;
        app.handle_search_view_key(key(KeyCode::Enter));
        assert_eq!(app.state, AppState::SearchView);
    }
}
