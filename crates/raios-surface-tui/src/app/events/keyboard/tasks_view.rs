use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_tasks_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.state = AppState::Dashboard,
            KeyCode::Up | KeyCode::Char('k') if self.tasks.cursor > 0 => {
                self.tasks.cursor -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') if self.tasks.cursor + 1 < self.tasks.list.len() => {
                self.tasks.cursor += 1;
            }
            KeyCode::Char(' ') | KeyCode::Char('v') | KeyCode::Char('V') => {
                if let Some(task) = self.tasks.list.get_mut(self.tasks.cursor) {
                    task.completed = !task.completed;
                    let _ = raios_runtime::tasks::save_tasks(
                        &self.config.dev_ops_path,
                        &self.tasks.list,
                    );
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
        app.state = AppState::TasksView;
        app.handle_tasks_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::TasksView;
        app.handle_tasks_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn cursor_stays_within_bounds_on_empty_task_list() {
        let mut app = App::test_instance();
        app.handle_tasks_view_key(key(KeyCode::Down));
        assert_eq!(app.tasks.cursor, 0);
        app.handle_tasks_view_key(key(KeyCode::Up));
        assert_eq!(app.tasks.cursor, 0);
    }

    // The `Space`/`v` completion toggle also calls
    // `raios_runtime::tasks::save_tasks`, which opens the real shared
    // `workspace.db` (path resolved via `raios_core::db::open_db`, only
    // overridable through the process-global `RAIOS_DB_PATH` env var). That
    // makes it unsafe to exercise from a parallel `cargo test` run without
    // risking a write to shared state or a race with other tests. The pure
    // in-memory toggle behavior is covered indirectly by the task struct's
    // own field semantics; persistence itself is exercised by the app at
    // runtime, not here.
}
