use raios_surface_tui::app::state::AppState;
use raios_surface_tui::app::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub mod dashboard;
pub mod editor;
pub mod health;
pub mod project;
pub mod setup;

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
                    self.handle_handover_approval(true);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.handle_handover_approval(false);
                }
                _ => {}
            }
            return Ok(());
        }

        // Constitution save-confirmation modal takes priority over all other input
        if self.constitution.pending_save.is_some() {
            match key.code {
                KeyCode::Enter => self.confirm_constitution_save(),
                KeyCode::Esc => self.cancel_constitution_save(),
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
                    self.launch_agent_for_active("claude");
                }
                KeyCode::Char('o') | KeyCode::Char('O') => {
                    self.launch_agent_for_active("opencode");
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    self.launch_agent_for_active("codex");
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.launch_agent_for_active("antigravity");
                }
                _ => {}
            }
            return Ok(());
        }

        if self.state == AppState::Dashboard
            && self.ui.menu_cursor == 2
            && key.code == KeyCode::Char('f')
        {
            self.run_compliance_auto_fix();
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
}
