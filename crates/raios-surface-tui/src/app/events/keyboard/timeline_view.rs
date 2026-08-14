use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_timeline_view_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.state = AppState::Dashboard;
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
        app.state = AppState::TimelineView;
        app.handle_timeline_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::TimelineView;
        app.handle_timeline_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn other_keys_are_a_no_op() {
        let mut app = App::test_instance();
        app.state = AppState::TimelineView;
        app.handle_timeline_view_key(key(KeyCode::Down));
        assert_eq!(app.state, AppState::TimelineView);
    }
}
