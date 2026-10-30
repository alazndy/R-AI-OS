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

    /// Says in the toast which parameter the call under the cursor expects next.
    pub(super) fn signature_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(requester) = self.language_requester() else {
            self.say("Bu dosya için dil sunucusu yok", cx);
            return;
        };
        let Some((path, line, character)) = self.editor.read(cx).cursor_at(cx) else {
            return;
        };
        let answer = crate::editor::lsp::ask_with_timeout(
            &requester,
            &path,
            "textDocument/signatureHelp",
            Some((line, character)),
            serde_json::Value::Null,
            Duration::from_secs(5),
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            this.update(cx, |workbench, cx| match answer {
                Ok(Ok(value)) => match rwk_core::lsp::signature::signature_line(&value) {
                    Some(line) => workbench.say(line, cx),
                    None => workbench.say("Burada parametre ipucu yok", cx),
                },
                _ => workbench.say("Dil sunucusu yanıt vermedi", cx),
            })
            .ok();
        })
        .detach();
    }

    /// Starts the servers again, those that gave up included.
    pub(super) fn restart_servers(&mut self, cx: &mut Context<Self>) {
        let Some(servers) = &self.servers else {
            self.say("Dil sunucuları kapalı", cx);
            return;
        };
        servers.restart();
        self.say("Dil sunucuları yeniden başlatılıyor…", cx);
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

impl Workbench {
    /// Shows a list of places under `title`; choosing one opens it.
    fn show_locations(
        &mut self,
        title: String,
        places: Vec<rwk_core::lsp::symbols::Place>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = places
            .into_iter()
            .map(|place| rwk_core::lsp::Problem {
                path: place.path,
                line: place.line,
                column: place.column,
                severity: rwk_core::lsp::Severity::Info,
                message: place.label,
                source: None,
            })
            .collect();
        let theme = self.theme();
        let view = cx.new(|cx| ProblemsView::titled(title, list, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.locations_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &ProblemsEvent, window, cx| match event {
                ProblemsEvent::Open(path, line) => {
                    // Terminal hits have no file: the line is the place in the result list.
                    if let Some(projects) = this.recent_choices.take() {
                        this.close_locations(window, cx);
                        if let Some(root) = projects.get(line.saturating_sub(1)) {
                            this.open_project(root.clone(), window, cx);
                        }
                        return;
                    }
                    if let Some(hits) = this.terminal_hits.take() {
                        this.close_locations(window, cx);
                        if let Some(grid_line) = hits.get(line.saturating_sub(1)) {
                            let grid_line = *grid_line;
                            this.with_selected_terminal(cx, |view, cx| {
                                view.scroll_to_line(grid_line, cx)
                            });
                        }
                        return;
                    }
                    this.close_locations(window, cx);
                    this.request_open_file_at(path.clone(), Some(*line), window, cx);
                }
                ProblemsEvent::Close => this.close_locations(window, cx),
            },
        ));
        self.locations_view = Some(view);
        cx.notify();
    }

    /// Closes the places list and returns focus to the workbench.
    pub fn close_locations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_hits = None;
        self.recent_choices = None;
        self.locations_view = None;
        self.locations_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The places list on screen.
    pub fn locations_view(&self) -> Option<&gpui::Entity<ProblemsView>> {
        self.locations_view.as_ref()
    }

    /// Lists where the symbol under the cursor is used.
    pub(super) fn find_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(requester), Some(project)) = (self.language_requester(), self.project.clone())
        else {
            self.say("Bu dosya için dil sunucusu yok", cx);
            return;
        };
        let Some((path, line, character, word)) = self.editor.read(cx).symbol_at_cursor(cx) else {
            self.say("İmleç bir sembolün üzerinde değil", cx);
            return;
        };
        self.say("Kullanılan yerler aranıyor…", cx);
        let answer = crate::editor::lsp::ask_with_timeout(
            &requester,
            &path,
            "textDocument/references",
            Some((line, character)),
            serde_json::json!({"context": {"includeDeclaration": true}}),
            Duration::from_secs(10),
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            let places = match answer {
                Ok(Ok(value)) => {
                    rwk_core::lsp::symbols::references(value, |uri| requester.relative_path(uri))
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Err(rwk_core::error::AppError::new(
                    rwk_core::error::ErrorCode::NotFound,
                    "Dil sunucusu yanıt vermedi",
                )),
            };
            // The lines are read on the background executor, each file once.
            let filled = match places {
                Ok(places) => Ok(cx
                    .background_executor()
                    .spawn(async move {
                        let mut files: std::collections::HashMap<String, Vec<String>> =
                            std::collections::HashMap::new();
                        places
                            .into_iter()
                            .map(|mut place| {
                                let lines = files.entry(place.path.clone()).or_insert_with(|| {
                                    project
                                        .read_document(&place.path)
                                        .map(|document| {
                                            document.content.lines().map(str::to_owned).collect()
                                        })
                                        .unwrap_or_default()
                                });
                                place.label = lines
                                    .get(place.line - 1)
                                    .map(|text| text.trim().to_owned())
                                    .unwrap_or_default();
                                place
                            })
                            .collect::<Vec<_>>()
                    })
                    .await),
                Err(error) => Err(error),
            };
            this.update_in(cx, |workbench, window, cx| match filled {
                Ok(places) if places.is_empty() => workbench.say("Kullanıldığı yer bulunamadı", cx),
                Ok(places) => {
                    workbench.show_locations(format!("KULLANIMLAR · {word}"), places, window, cx)
                }
                Err(error) => workbench.say(error.message, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Lists the symbols of the open file.
    pub(super) fn show_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(requester), Some(path)) = (
            self.language_requester(),
            self.editor.read(cx).path().map(str::to_owned),
        ) else {
            self.say("Bu dosya için dil sunucusu yok", cx);
            return;
        };
        let answer = crate::editor::lsp::ask_with_timeout(
            &requester,
            &path,
            "textDocument/documentSymbol",
            None,
            serde_json::json!({}),
            Duration::from_secs(10),
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            this.update_in(cx, |workbench, window, cx| {
                let places = match answer {
                    Ok(Ok(value)) => rwk_core::lsp::symbols::outline(value, &path),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(rwk_core::error::AppError::new(
                        rwk_core::error::ErrorCode::NotFound,
                        "Dil sunucusu yanıt vermedi",
                    )),
                };
                match places {
                    Ok(places) if places.is_empty() => workbench.say("Dosyada sembol yok", cx),
                    Ok(places) => {
                        workbench.show_locations(format!("SEMBOLLER · {path}"), places, window, cx)
                    }
                    Err(error) => workbench.say(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks for text and lists the project's symbols that match it.
    pub(super) fn open_symbol_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::rename::{PromptPurpose, RenamePromptEvent, RenameTerminalPrompt};
        if self.rename_prompt.is_some() {
            return;
        }
        if self.language_requester().is_none() {
            self.say("Bu proje için dil sunucusu yok", cx);
            return;
        }
        let theme = self.theme();
        let prompt =
            cx.new(|cx| RenameTerminalPrompt::entry(PromptPurpose::SearchSymbols, "", window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::EntryName(query) => {
                    this.close_rename_prompt(window, cx);
                    this.search_symbols(query.clone(), window, cx);
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::NewTask(_) | RenamePromptEvent::Rename(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    /// Asks the open file's language server for the symbols named like `query`.
    fn search_symbols(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(requester) = self.language_requester() else {
            self.say("Bu proje için dil sunucusu yok", cx);
            return;
        };
        // The server is the one of the open file, else of the project's manifest.
        let open_file = self.editor.read(cx).path().map(str::to_owned);
        let Some(hint) = open_file.or_else(|| requester.language_hint().map(str::to_owned)) else {
            self.say("Bu proje için dil sunucusu yok", cx);
            return;
        };
        self.say("Semboller aranıyor…", cx);
        let answer = crate::editor::lsp::ask_workspace_with_timeout(
            &requester,
            &hint,
            "workspace/symbol",
            serde_json::json!({"query": query}),
            Duration::from_secs(10),
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            this.update_in(cx, |workbench, window, cx| {
                let places = match answer {
                    Ok(Ok(value)) => rwk_core::lsp::symbols::workspace_symbols(value, |uri| {
                        requester.relative_path(uri)
                    }),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(rwk_core::error::AppError::new(
                        rwk_core::error::ErrorCode::NotFound,
                        "Dil sunucusu yanıt vermedi",
                    )),
                };
                match places {
                    Ok(places) if places.is_empty() => workbench.say("Eşleşen sembol yok", cx),
                    Ok(places) => {
                        workbench.show_locations(format!("SEMBOL · {query}"), places, window, cx)
                    }
                    Err(error) => workbench.say(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks for text and lists the lines of the selected terminal's output that contain it.
    pub(super) fn open_terminal_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::rename::{PromptPurpose, RenamePromptEvent, RenameTerminalPrompt};
        if self.rename_prompt.is_some() || self.terminals.get(self.selected).is_none() {
            return;
        }
        let theme = self.theme();
        let prompt =
            cx.new(|cx| RenameTerminalPrompt::entry(PromptPurpose::SearchTerminal, "", window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::EntryName(query) => {
                    this.close_rename_prompt(window, cx);
                    this.search_terminal(query, window, cx);
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::NewTask(_) | RenamePromptEvent::Rename(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    fn search_terminal(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.terminals.get(self.selected) else {
            return;
        };
        let found = pane.view.read(cx).find_lines(query, 500);
        if found.is_empty() {
            self.say("Terminal çıktısında bulunamadı", cx);
            return;
        }
        let places = found
            .iter()
            .enumerate()
            .map(|(index, (_, text))| rwk_core::lsp::symbols::Place {
                path: String::new(),
                line: index + 1,
                column: 1,
                label: text.trim().to_owned(),
            })
            .collect();
        self.show_locations(format!("TERMİNAL · {query}"), places, window, cx);
        // Set after the list opens: opening a list resets any earlier terminal results.
        self.terminal_hits = Some(found.into_iter().map(|(line, _)| line).collect());
    }

    /// The recently opened projects, newest first, as stored.
    pub fn recent_projects(&self) -> &[std::path::PathBuf] {
        &self.recent_projects
    }

    /// Sets the stored list (at start-up; nothing is announced).
    pub fn set_recent_projects(&mut self, projects: Vec<std::path::PathBuf>) {
        self.recent_projects = projects;
    }

    /// Puts `root` first in the recent list and tells the application to store it.
    pub(super) fn remember_recent(&mut self, root: &std::path::Path, cx: &mut Context<Self>) {
        self.recent_projects.retain(|known| known != root);
        self.recent_projects.insert(0, root.to_path_buf());
        self.recent_projects.truncate(10);
        cx.emit(super::WorkbenchEvent::RecentProjectsChanged(
            self.recent_projects.clone(),
        ));
    }

    /// Lists the recent projects; choosing one opens it (through the usual unsaved-edits and
    /// running-sessions questions).
    pub(super) fn show_recent_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .project
            .as_ref()
            .map(|project| project.info().root.clone());
        let choices: Vec<std::path::PathBuf> = self
            .recent_projects
            .iter()
            .filter(|root| Some(*root) != current.as_ref() && root.is_dir())
            .cloned()
            .collect();
        if choices.is_empty() {
            self.say("Başka son açılan proje yok", cx);
            return;
        }
        let places = choices
            .iter()
            .enumerate()
            .map(|(index, root)| rwk_core::lsp::symbols::Place {
                path: String::new(),
                line: index + 1,
                column: 1,
                label: root.display().to_string(),
            })
            .collect();
        self.show_locations("SON PROJELER".to_owned(), places, window, cx);
        self.recent_choices = Some(choices);
    }
}
