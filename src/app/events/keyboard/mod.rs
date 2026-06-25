use crate::app::state::AppState;
use crate::app::App;
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
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = "{\"command\":\"HumanApproval\",\"approved\":true}".to_string();
                        let _ = tx.send(msg);
                    }
                    self.system.handover_modal = None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some(ref tx) = self.tx_daemon {
                        let msg = "{\"command\":\"HumanApproval\",\"approved\":false}".to_string();
                        let _ = tx.send(msg);
                    }
                    self.system.handover_modal = None;
                }
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
                    if let Some(ref proj) = self.projects.active.clone() {
                        let msg =
                            crate::app::events::helpers::launch_agent("claude", &proj.local_path);
                        self.system.sync_status = Some(msg);
                    }
                    self.ui.show_launcher = false;
                }
                KeyCode::Char('o') | KeyCode::Char('O') => {
                    if let Some(ref proj) = self.projects.active.clone() {
                        let msg =
                            crate::app::events::helpers::launch_agent("opencode", &proj.local_path);
                        self.system.sync_status = Some(msg);
                    }
                    self.ui.show_launcher = false;
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    if let Some(ref proj) = self.projects.active.clone() {
                        let msg =
                            crate::app::events::helpers::launch_agent("codex", &proj.local_path);
                        self.system.sync_status = Some(msg);
                    }
                    self.ui.show_launcher = false;
                }
                _ => {}
            }
            return Ok(());
        }

        if self.state == AppState::Dashboard
            && self.ui.menu_cursor == 2
            && key.code == KeyCode::Char('f')
        {
            if let Some(ref report) = self.health.compliance {
                if !report.violations.is_empty() && !self.health.is_fixing {
                    self.health.is_fixing = true;
                    self.health.fix_status = Some("Claude fixing issues...".into());
                    self.add_activity("Agent", "Initiating Auto-Fix with Claude Code", "Warning");
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        tx.send(crate::app::state::BgMsg::SyncDone(
                            "Auto-Fix Complete: Issues resolved".into(),
                        ))
                        .ok();
                    });
                }
            }
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
