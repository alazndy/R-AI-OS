use crossterm::event::{KeyCode, KeyEvent};
use raios_surface_tui::app::state::{AppState, CreatorStep};
use raios_surface_tui::app::App;

impl App {
    pub(crate) fn handle_constitution_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc if self.constitution.item_editing => {
                self.constitution.item_editing = false;
                self.constitution.item_input.clear();
            }
            KeyCode::Esc if self.constitution.creator.active => self.close_creator(),
            KeyCode::Esc | KeyCode::Char('q') => self.state = AppState::Dashboard,
            KeyCode::Char(tab @ '1'..='9')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                let index = (tab as usize) - ('1' as usize);
                if index < self.constitution.tabs.len() {
                    self.load_constitution_tab(index);
                }
            }
            KeyCode::Up | KeyCode::Char('k')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                self.constitution.outline_cursor =
                    self.constitution.outline_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                let max = self.constitution.rows.len().saturating_sub(1);
                self.constitution.outline_cursor = (self.constitution.outline_cursor + 1).min(max);
            }
            KeyCode::Char('r') | KeyCode::Enter
                if !self.constitution.item_editing
                    && !self.constitution.creator.active
                    && !self.constitution.sections.is_empty() =>
            {
                self.jump_to_constitution_raw_edit();
            }
            KeyCode::Char('i')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                self.begin_item_edit();
            }
            KeyCode::Char('n')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                self.begin_item_insert();
            }
            KeyCode::Char('d')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                self.delete_item_at_cursor();
            }
            KeyCode::Char('c')
                if !self.constitution.item_editing && !self.constitution.creator.active =>
            {
                self.open_creator();
            }
            KeyCode::Char('p')
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(false);
            }
            KeyCode::Char('g')
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(true);
            }
            KeyCode::Enter
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::Notes =>
            {
                self.constitution.creator.step = CreatorStep::Preview;
            }
            KeyCode::Char(character)
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.push(character);
            }
            KeyCode::Backspace
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.pop();
            }
            KeyCode::Enter
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::Preview =>
            {
                if self.constitution.creator.target_is_global {
                    self.constitution.creator.step = CreatorStep::ConfirmGlobal;
                } else {
                    self.creator_confirm_save();
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y')
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::ConfirmGlobal =>
            {
                self.creator_confirm_save();
            }
            KeyCode::Char('n') | KeyCode::Char('N')
                if self.constitution.creator.active
                    && self.constitution.creator.step == CreatorStep::ConfirmGlobal =>
            {
                self.constitution.creator.step = CreatorStep::Preview;
            }
            KeyCode::Enter if self.constitution.item_editing => self.commit_item_edit(),
            KeyCode::Char(character) if self.constitution.item_editing => {
                self.constitution.item_input.push(character);
            }
            KeyCode::Backspace if self.constitution.item_editing => {
                self.constitution.item_input.pop();
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
    fn esc_and_q_return_to_dashboard_when_no_modal_is_active() {
        let mut app = App::test_instance();
        app.state = AppState::ConstitutionView;
        app.handle_constitution_view_key(key(KeyCode::Esc));
        assert_eq!(app.state, AppState::Dashboard);

        app.state = AppState::ConstitutionView;
        app.handle_constitution_view_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Dashboard);
    }

    #[test]
    fn esc_while_item_editing_closes_the_editor_not_the_view() {
        let mut app = App::test_instance();
        app.state = AppState::ConstitutionView;
        app.constitution.item_editing = true;
        app.constitution.item_input = "draft".into();

        app.handle_constitution_view_key(key(KeyCode::Esc));

        assert_eq!(app.state, AppState::ConstitutionView);
        assert!(!app.constitution.item_editing);
        assert!(app.constitution.item_input.is_empty());
    }
}
