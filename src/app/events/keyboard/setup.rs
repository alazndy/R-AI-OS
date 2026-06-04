use crate::app::App;
use crate::app::state::{AppState, BgMsg};
use crate::config::Config;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

impl App {
    pub(crate) fn handle_setup_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::setup_wizard::WizardStep;

        // Editing mode — capture text input
        if self.wizard.editing {
            match key.code {
                KeyCode::Enter => {
                    self.wizard_commit_input();
                    self.wizard.editing = false;
                }
                KeyCode::Esc => {
                    self.wizard.editing = false;
                }
                KeyCode::Char(c) => self.wizard.input.push(c),
                KeyCode::Backspace => {
                    self.wizard.input.pop();
                }
                _ => {}
            }
            return Ok(());
        }

        // Running — ignore all keys
        if self.wizard.running {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Navigate fields within a step
            KeyCode::Up | KeyCode::Char('k') if self.wizard.field_cursor > 0 => {
                self.wizard.field_cursor -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.wizard.field_cursor += 1;
            }

            // Enter: begin editing or advance step
            KeyCode::Enter => {
                match &self.wizard.step {
                    WizardStep::Welcome => {
                        self.wizard.step = WizardStep::Workspace;
                        self.wizard.field_cursor = 0;
                    }
                    WizardStep::Done => {
                        // Apply config and open dashboard
                        let dev_ops = PathBuf::from(&self.wizard.dev_ops);
                        let master = PathBuf::from(&self.wizard.master);
                        let skills = dev_ops.join(".agents").join("skills");
                        let vault = if self.wizard.vault.is_empty() {
                            PathBuf::new()
                        } else {
                            PathBuf::from(&self.wizard.vault)
                        };
                        if !dev_ops.as_os_str().is_empty() {
                            let cfg = Config {
                                dev_ops_path: dev_ops,
                                master_md_path: master,
                                skills_path: skills,
                                vault_projects_path: vault,
                            };
                            let _ = cfg.save();
                            self.config = cfg;
                        }
                        self.state = AppState::Dashboard;
                        let tx = self.tx.clone();
                        tx.send(BgMsg::TransitionToDashboard).ok();
                    }
                    WizardStep::Initialize => {
                        self.wizard_run_initialize();
                    }
                    _ => {
                        // Start editing current field
                        self.wizard_start_edit();
                    }
                }
            }

            // [s] — advance to next step (also runs current step's setup)
            KeyCode::Char('s') => {
                self.wizard_advance_step();
            }

            // [Tab] — skip/unskip agent steps
            KeyCode::Tab => match self.wizard.step {
                WizardStep::Claude => self.wizard.skip_claude = !self.wizard.skip_claude,
                WizardStep::Gemini => self.wizard.skip_gemini = !self.wizard.skip_gemini,
                WizardStep::Antigravity => {
                    self.wizard.skip_antigravity = !self.wizard.skip_antigravity
                }
                _ => {
                    self.wizard_advance_step();
                }
            },

            _ => {}
        }
        Ok(())
    }
}
