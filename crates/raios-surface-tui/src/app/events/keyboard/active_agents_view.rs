use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_active_agents_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.state = AppState::Dashboard,
            KeyCode::Up | KeyCode::Char('k') if self.system.selected_agent_idx > 0 => {
                self.system.selected_agent_idx -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.system.selected_agent_idx + 1 < self.system.active_agents.len() =>
            {
                self.system.selected_agent_idx += 1;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use raios_runtime::daemon::proxy::AgentProcess;
    use uuid::Uuid;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn fake_agent(name: &str) -> AgentProcess {
        AgentProcess {
            id: Uuid::new_v4(),
            name: name.into(),
            status: "Running".into(),
            started_at: std::time::SystemTime::now(),
            logs: vec![],
        }
    }

    #[test]
    fn esc_and_q_return_to_dashboard() {
        let mut app = App::test_instance();
        app.state = AppState::ActiveAgentsView;
        app.handle_active_agents_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::ActiveAgentsView;
        app.handle_active_agents_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn selection_stays_at_zero_on_empty_agent_list() {
        let mut app = App::test_instance();
        app.handle_active_agents_view_key(key(KeyCode::Down));
        assert_eq!(app.system.selected_agent_idx, 0);
        app.handle_active_agents_view_key(key(KeyCode::Up));
        assert_eq!(app.system.selected_agent_idx, 0);
    }

    #[test]
    fn selection_clamps_at_the_last_and_first_agent() {
        let mut app = App::test_instance();
        app.system.active_agents = vec![fake_agent("a"), fake_agent("b"), fake_agent("c")];

        app.handle_active_agents_view_key(key(KeyCode::Down));
        app.handle_active_agents_view_key(key(KeyCode::Down));
        assert_eq!(app.system.selected_agent_idx, 2);

        // One more Down past the last agent must not go out of bounds.
        app.handle_active_agents_view_key(key(KeyCode::Down));
        assert_eq!(app.system.selected_agent_idx, 2);

        app.handle_active_agents_view_key(key(KeyCode::Up));
        assert_eq!(app.system.selected_agent_idx, 1);

        app.handle_active_agents_view_key(key(KeyCode::Up));
        app.handle_active_agents_view_key(key(KeyCode::Up));
        // One more Up past the first agent must not underflow.
        assert_eq!(app.system.selected_agent_idx, 0);
    }
}
