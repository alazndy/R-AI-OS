use crate::app::state::{AppState, BgMsg};
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent};
use std::thread;

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
                self.health.is_checking = true;
                if let Some(ref tx_daemon) = self.tx_daemon {
                    let _ = tx_daemon.send("{\"command\":\"GetState\"}".into());
                    self.add_activity("System", "Manual health refresh requested", "Info");
                }
            }
            KeyCode::Char('c') => {
                if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
                    if h.git_dirty == Some(true) {
                        let tx = self.tx.clone();
                        let path = h.path.clone();
                        let name = h.name.clone();
                        self.system.sync_status = Some(format!("Committing {}...", name));
                        thread::spawn(move || {
                            let r = crate::core::git::commit(&path, "chore: raios update", true);
                            tx.send(BgMsg::GitActionDone {
                                project: name,
                                action: "commit".into(),
                                ok: r.ok,
                                message: r.message,
                            })
                            .ok();
                        });
                    } else {
                        self.system.sync_status =
                            Some("Nothing to commit (working tree clean)".into());
                    }
                }
            }
            KeyCode::Char('p') => {
                if let Some(h) = self.health.report.get(self.health.cursor).cloned() {
                    let tx = self.tx.clone();
                    let path = h.path.clone();
                    let name = h.name.clone();
                    self.system.sync_status = Some(format!("Pushing {}...", name));
                    thread::spawn(move || {
                        let r = crate::core::git::push(&path);
                        tx.send(BgMsg::GitActionDone {
                            project: name,
                            action: "push".into(),
                            ok: r.ok,
                            message: r.message,
                        })
                        .ok();
                    });
                }
            }
            _ => {}
        }
    }
}
