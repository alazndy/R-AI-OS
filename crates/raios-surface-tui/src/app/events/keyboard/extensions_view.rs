use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::{AppState, ExtFocus};
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_extensions_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc if self.ext.editing => {
                self.ext.editing = false;
                self.ext.input.clear();
            }
            KeyCode::Esc | KeyCode::Char('q') => self.state = AppState::Dashboard,
            KeyCode::Tab => {
                self.ext.focus = if self.ext.focus == ExtFocus::Commands {
                    ExtFocus::Config
                } else {
                    ExtFocus::Commands
                };
            }
            KeyCode::Up | KeyCode::Char('k') if !self.ext.editing => {
                if self.ext.focus == ExtFocus::Commands {
                    self.ext.cmd_cursor = self.ext.cmd_cursor.saturating_sub(1);
                } else {
                    self.ext.cfg_cursor = self.ext.cfg_cursor.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') if !self.ext.editing => {
                let max = self
                    .ext
                    .extensions
                    .get(self.ext.ext_cursor)
                    .map(|extension| match self.ext.focus {
                        ExtFocus::Commands => extension.commands.len().saturating_sub(1),
                        ExtFocus::Config => extension.config_schema.len().saturating_sub(1),
                    })
                    .unwrap_or(0);
                if self.ext.focus == ExtFocus::Commands {
                    self.ext.cmd_cursor = (self.ext.cmd_cursor + 1).min(max);
                } else {
                    self.ext.cfg_cursor = (self.ext.cfg_cursor + 1).min(max);
                }
            }
            KeyCode::Left if !self.ext.editing && self.ext.ext_cursor > 0 => {
                self.ext.ext_cursor -= 1;
                self.ext.cmd_cursor = 0;
                self.ext.cfg_cursor = 0;
            }
            KeyCode::Right
                if !self.ext.editing && self.ext.ext_cursor + 1 < self.ext.extensions.len() =>
            {
                self.ext.ext_cursor += 1;
                self.ext.cmd_cursor = 0;
                self.ext.cfg_cursor = 0;
            }
            KeyCode::Enter if self.ext.focus == ExtFocus::Commands && !self.ext.editing => {
                self.run_ext_cmd();
            }
            KeyCode::Char('e') if self.ext.focus == ExtFocus::Config && !self.ext.editing => {
                if let Some(extension) = self.ext.extensions.get(self.ext.ext_cursor) {
                    if let Some(field) = extension.config_schema.get(self.ext.cfg_cursor) {
                        self.ext.input = if field.masked {
                            String::new()
                        } else {
                            field.value.clone()
                        };
                        self.ext.editing = true;
                    }
                }
            }
            KeyCode::Enter if self.ext.focus == ExtFocus::Config && self.ext.editing => {
                self.save_ext_config_field();
            }
            KeyCode::Char(character) if self.ext.editing => self.ext.input.push(character),
            KeyCode::Backspace if self.ext.editing => {
                self.ext.input.pop();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use raios_surface_tui::app::App;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_and_q_return_to_dashboard_when_not_editing() {
        let mut app = App::test_instance();
        app.state = AppState::ExtensionsView;
        app.handle_extensions_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::ExtensionsView;
        app.handle_extensions_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn esc_while_editing_a_config_field_closes_the_editor_not_the_view() {
        let mut app = App::test_instance();
        app.state = AppState::ExtensionsView;
        app.ext.editing = true;
        app.ext.input = "draft".into();

        app.handle_extensions_view_key(key(KeyCode::Esc));

        assert_eq!(app.state, AppState::ExtensionsView);
        assert!(!app.ext.editing);
        assert!(app.ext.input.is_empty());
    }

    #[test]
    fn tab_toggles_focus_between_commands_and_config() {
        let mut app = App::test_instance();
        assert_eq!(app.ext.focus, ExtFocus::Commands);
        app.handle_extensions_view_key(key(KeyCode::Tab));
        assert_eq!(app.ext.focus, ExtFocus::Config);
        app.handle_extensions_view_key(key(KeyCode::Tab));
        assert_eq!(app.ext.focus, ExtFocus::Commands);
    }
}
