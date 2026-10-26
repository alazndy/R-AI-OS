//! Language servers for the open project: the workbench tells them which files are open and
//! what they contain, and shows what they report (squiggles in the editor, the Problems view
//! and the count in the status line).

use std::time::Duration;

use gpui::{AppContext, Context, Window};
use rwk_core::lsp::{summary, LanguageServers, Launcher, ManagerEvent, Problems};

use super::Workbench;
use crate::problems::{ProblemsEvent, ProblemsView};

/// How long after the last keystroke the server is sent the new text.
const CHANGE_DELAY: Duration = Duration::from_millis(250);

impl Workbench {
    /// Chooses how language servers are started. `None` (the default) turns them off.
    pub fn set_language_server_launcher(&mut self, launcher: Option<Launcher>) {
        self.lsp_launcher = launcher;
    }

    /// What the language servers reported for the open files.
    pub fn problems(&self) -> &Problems {
        &self.problems
    }

    /// The status line text for the problem counts, when there is something to say.
    pub fn problem_summary(&self) -> Option<String> {
        summary(self.problems.counts())
    }

    /// A handle for asking the open project's language servers questions.
    pub fn language_requester(&self) -> Option<rwk_core::lsp::Requester> {
        self.servers.as_ref().map(LanguageServers::requester)
    }

    /// The open Problems view.
    pub fn problems_view(&self) -> Option<&gpui::Entity<ProblemsView>> {
        self.problems_view.as_ref()
    }

    /// Stops the servers of the project being left and starts a manager for the new one.
    pub(super) fn restart_language_servers(&mut self, cx: &mut Context<Self>) {
        self.stop_language_servers(cx);
        self.problems.clear();
        let (Some(launcher), Some(project)) = (self.lsp_launcher.clone(), self.project.clone())
        else {
            return;
        };
        let (servers, events) = LanguageServers::new(project.info().root.clone(), launcher);
        let requester = servers.requester();
        self.editor.update(cx, |editor, cx| {
            editor.set_language_requester(Some(requester), cx)
        });
        self.servers = Some(servers);

        // The manager's channel is blocking; a thread moves its events to an async one.
        let (sender, receiver) = async_channel::unbounded();
        let spawned = std::thread::Builder::new()
            .name("lsp-events".into())
            .spawn(move || {
                for event in events {
                    if sender.send_blocking(event).is_err() {
                        break;
                    }
                }
            });
        if spawned.is_err() {
            self.servers = None;
            return;
        }
        self.lsp_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |workbench, cx| workbench.on_server_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Stops the servers; waiting for them to exit happens off the UI thread.
    pub(super) fn stop_language_servers(&mut self, cx: &mut Context<Self>) {
        self.lsp_task = None;
        self.lsp_sync_task = None;
        self.editor
            .update(cx, |editor, cx| editor.set_language_requester(None, cx));
        if let Some(servers) = self.servers.take() {
            cx.background_executor()
                .spawn(async move { servers.shutdown() })
                .detach();
        }
    }

    fn on_server_event(&mut self, event: ManagerEvent, cx: &mut Context<Self>) {
        match event {
            ManagerEvent::Diagnostics { path, diagnostics } => {
                self.editor.update(cx, |editor, cx| {
                    editor.set_diagnostics(&path, &diagnostics, cx)
                });
                self.problems.set(&path, diagnostics);
                if let Some(view) = self.problems_view.clone() {
                    let list = self.problems.list();
                    view.update(cx, |view, cx| view.set_problems(list, cx));
                }
                cx.notify();
            }
            // A server that is missing or gave up only means no language features for its
            // files; the footer says so, nothing interrupts the user.
            ManagerEvent::Status { server, status } => {
                self.note_server_status(server, status);
                cx.notify();
            }
        }
    }

    /// A tab was opened.
    pub(super) fn lsp_opened(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(servers) = &self.servers else {
            return;
        };
        if let Some(text) = self.editor.read(cx).text_of(path, cx) {
            servers.open(path, &text);
        }
    }

    /// A tab was closed.
    pub(super) fn lsp_closed(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(servers) = &self.servers {
            servers.close(path);
        }
        self.problems.set(path, Vec::new());
        cx.notify();
    }

    /// The active tab was edited: the server gets the text once typing pauses.
    pub(super) fn lsp_edited(&mut self, cx: &mut Context<Self>) {
        if self.servers.is_none() {
            return;
        }
        self.lsp_sync_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHANGE_DELAY).await;
            this.update(cx, |workbench, cx| {
                let editor = workbench.editor.read(cx);
                let Some(path) = editor.path().map(str::to_owned) else {
                    return;
                };
                let Some(text) = editor.text_of(&path, cx) else {
                    return;
                };
                if let Some(servers) = &workbench.servers {
                    servers.change(&path, &text);
                }
            })
            .ok();
        }));
    }

    /// Opens the Problems view.
    pub(super) fn open_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.problems_view.is_some() || self.project.is_none() {
            return;
        }
        let theme = self.theme();
        let list = self.problems.list();
        let view = cx.new(|cx| ProblemsView::new(list, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.problems_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &ProblemsEvent, window, cx| match event {
                ProblemsEvent::Open(path, line) => {
                    this.close_problems(window, cx);
                    this.request_open_file_at(path.clone(), Some(*line), window, cx);
                }
                ProblemsEvent::Close => this.close_problems(window, cx),
            },
        ));
        self.problems_view = Some(view);
        cx.notify();
    }

    /// Closes the Problems view and returns focus to the workbench.
    pub fn close_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.problems_view = None;
        self.problems_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
