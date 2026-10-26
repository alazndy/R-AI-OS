//! The editor view: several open files as tabs, each with its own edit/save state, and the
//! read-only Git diff of the active one.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
};
use gpui_base::input::{Editor, EditorState, InputEvent, InputHighlighter, Position};
use rwk_core::documents::Document;
use rwk_core::error::{AppError, ErrorCode};
use rwk_core::project::ProjectRoot;
use rwk_core::workbench::{
    side_by_side, Appearance, DocumentSession, SaveBlocked, SaveTicket, SavedTab, Skin,
    SwitchDecision, Theme, Tone, MAX_SAVED_TABS,
};

use super::diff::DiffHighlighter;
use super::highlight::{
    highlight_enabled, highlighter_factory, language_for_path, HIGHLIGHT_LIMIT_BYTES,
};
use super::styles::editor_style;

/// What the editor tells the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    /// The text changed.
    Edited,
    /// A file was loaded into the editor (relative path).
    Opened(String),
    /// A save succeeded.
    Saved(Document),
    /// A save failed for a reason other than a conflict.
    SaveFailed(AppError),
    /// The file changed on disk; the edits are kept.
    Conflict,
    /// The user asked to close a tab through its strip button; the workbench decides (a dirty
    /// tab needs an answer about its edits first).
    CloseRequested(String),
    /// A tab was closed.
    Closed(String),
    /// The active tab changed.
    Activated(String),
    /// A definition lies in another project file; the workbench opens it.
    OpenLocation {
        /// Relative path of the file.
        path: String,
        /// 1-based line.
        line: usize,
    },
    /// The user right-clicked a tab; it is active now.
    TabContextMenu {
        /// Relative path of the tab.
        path: String,
        /// Where the pointer was.
        position: gpui::Point<gpui::Pixels>,
    },
    /// The user right-clicked the text; the cursor is under the pointer.
    TextContextMenu {
        /// Where the pointer was.
        position: gpui::Point<gpui::Pixels>,
        /// Some text is selected.
        selection: bool,
        /// The text can be edited (not a read-only diff or a disabled view).
        editable: bool,
    },
}

/// An edit the text menu can run on the focused editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    /// Cut the selection.
    Cut,
    /// Copy the selection.
    Copy,
    /// Paste the clipboard.
    Paste,
    /// Select everything.
    SelectAll,
    /// Undo the last edit.
    Undo,
    /// Redo the undone edit.
    Redo,
}

/// Builds an editor state with the settings every editor in the workspace uses: soft wrap off
/// (spec section 2: it costs about 0.26 ms per line), line numbers and folding on.
fn new_editor_state(
    theme: Theme,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) -> EditorState {
    let mut state = EditorState::new(window, cx)
        .soft_wrap(false)
        .line_number(true)
        .folding(true);
    state.set_editor_style(editor_style(theme));
    state
}

/// One open file: its editor state, its guard and its optional Git diff.
struct Tab {
    state: Entity<EditorState>,
    session: DocumentSession,
    /// False when the file is above the highlight limit.
    highlighting: bool,
    diff: Option<Entity<EditorState>>,
    /// The right column while the diff is shown side by side (`diff` is then the left one).
    diff_right: Option<Entity<EditorState>>,
    diff_raw: String,
    diff_visible: bool,
    _subscription: Subscription,
    /// Repaints the status bar when the cursor moves.
    _cursor: Subscription,
}

impl Tab {
    /// Path of the tab's file, relative to its project.
    fn path(&self) -> Option<&str> {
        self.session.path()
    }
}

/// How long saving waits for the formatter before saving the text as it is.
const FORMAT_ON_SAVE_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// The editor: a strip of open files over one editor, plus the diff of the active file.
pub struct EditorView {
    theme: Theme,
    tabs: Vec<Tab>,
    active: usize,
    diff_split: bool,
    /// The file shown read-only beside the active one, when the editor is split.
    split: Option<String>,
    /// The message under the editor (last action's outcome).
    status: Option<SharedString>,
    /// Git marks by project-relative path, shown on the tabs.
    badges: HashMap<String, rwk_core::git::Badge>,
    /// Bumped by every read request; a read that finishes for a superseded request is dropped.
    next_request: u64,
    /// In-flight read per path; a closed tab's entry is removed so its read cannot reopen it.
    requests: HashMap<String, u64>,
    diff_generation: u64,
    /// Bumped by every restore; a restore that finishes after a project switch is dropped.
    restore_generation: u64,
    /// How tabs reach the project's language servers; `None` means no language features.
    requester: Option<rwk_core::lsp::Requester>,
}

impl EventEmitter<EditorEvent> for EditorView {}

impl EditorView {
    /// An empty editor.
    pub fn new(tone: Tone, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::themed(
            Theme::of(Appearance {
                skin: Skin::Standard,
                tone,
            }),
            window,
            cx,
        )
    }

    /// An editor painted with `theme`.
    pub fn themed(theme: Theme, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            theme,
            tabs: Vec::new(),
            active: 0,
            diff_split: false,
            status: None,
            badges: HashMap::new(),
            next_request: 0,
            requests: HashMap::new(),
            diff_generation: 0,
            restore_generation: 0,
            requester: None,
            split: None,
        }
    }

    /// The text in the active editor.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.tabs
            .get(self.active)
            .map(|tab| tab.state.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// Replaces the git marks shown on the tabs.
    pub fn set_badges(
        &mut self,
        badges: HashMap<String, rwk_core::git::Badge>,
        cx: &mut Context<Self>,
    ) {
        if self.badges != badges {
            self.badges = badges;
            cx.notify();
        }
    }

    /// The git mark a tab shows, when its file has one.
    pub fn badge(&self, path: &str) -> Option<rwk_core::git::Badge> {
        self.badges.get(path).copied()
    }

    /// The 1-based line of the cursor, when a document is open.
    pub fn cursor_line(&self, cx: &gpui::App) -> Option<usize> {
        let tab = self.tabs.get(self.active)?;
        tab.path()?;
        let position = tab.state.read(cx).cursor_position();
        Some(usize::try_from(position.line).unwrap_or(usize::MAX) + 1)
    }

    /// Path of the active file, relative to its project.
    pub fn path(&self) -> Option<&str> {
        self.tabs.get(self.active).and_then(Tab::path)
    }

    /// Paths of every open tab, in strip order.
    pub fn paths(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter_map(|tab| tab.path().map(str::to_owned))
            .collect()
    }

    /// True when `path` is open in a tab.
    pub fn contains(&self, path: &str) -> bool {
        self.position_of(path).is_some()
    }

    /// True when the active file has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.tabs
            .get(self.active)
            .is_some_and(|tab| tab.session.is_dirty())
    }

    /// Paths of the tabs with unsaved edits, in strip order: what a guard must ask about.
    pub fn dirty_paths(&self) -> Vec<String> {
        self.tabs
            .iter()
            .filter(|tab| tab.session.is_dirty())
            .filter_map(|tab| tab.path().map(str::to_owned))
            .collect()
    }

    /// True when the last save of the active file found it changed on disk.
    pub fn has_conflict(&self) -> bool {
        self.tabs
            .get(self.active)
            .is_some_and(|tab| tab.session.has_conflict())
    }

    /// The message shown under the editor.
    pub fn status(&self) -> Option<String> {
        self.status.as_ref().map(ToString::to_string)
    }

    /// False when the active file is above the highlight limit.
    pub fn highlighting_enabled(&self) -> bool {
        self.tabs
            .get(self.active)
            .is_none_or(|tab| tab.highlighting)
    }

    /// Whether the window or project may close now.
    pub fn request_leave(&self) -> SwitchDecision {
        if self.dirty_paths().is_empty() {
            SwitchDecision::Allowed
        } else {
            SwitchDecision::NeedsDecision
        }
    }

    fn position_of(&self, path: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.path() == Some(path))
    }

    fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(message.into());
        cx.notify();
    }

    /// Inserts text at the cursor, as typing does.
    pub fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_owned();
        if let Some(tab) = self.tabs.get(self.active) {
            tab.state
                .update(cx, |state, cx| state.insert(text.as_str(), window, cx));
        }
    }

    /// Closes every tab (a project switch discards them all).
    pub fn clear(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.requests.clear();
        self.split = None;
        self.active = 0;
        self.status = None;
        self.diff_generation += 1;
        cx.notify();
    }

    /// Closes the tab of `path`. The caller has already answered any unsaved-edits question:
    /// closing drops the edits with the tab.
    pub fn close_tab(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        self.requests.remove(path);
        let was_active = index == self.active;
        self.tabs.remove(index);
        if index < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if was_active {
            self.focus(window, cx);
        }
        self.fix_split(cx);
        cx.emit(EditorEvent::Closed(path.to_owned()));
        cx.notify();
    }

    /// Makes the tab of `path` the active one.
    pub fn activate_tab(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        // Activating the file on the right trades places with the one that was active.
        if self.split.as_deref() == Some(path) {
            self.split = self.path().map(str::to_owned);
        }
        self.active = index;
        self.fix_split(cx);
        self.focus(window, cx);
        cx.emit(EditorEvent::Activated(path.to_owned()));
        cx.notify();
    }

    /// Runs `edit` on the active editor, which takes focus first so the action reaches it.
    pub fn edit(&self, edit: TextEdit, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_base::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        tab.state.update(cx, |state, cx| state.focus(window, cx));
        let action: Box<dyn gpui::Action> = match edit {
            TextEdit::Cut => Box::new(Cut),
            TextEdit::Copy => Box::new(Copy),
            TextEdit::Paste => Box::new(Paste),
            TextEdit::SelectAll => Box::new(SelectAll),
            TextEdit::Undo => Box::new(Undo),
            TextEdit::Redo => Box::new(Redo),
        };
        window.dispatch_action(action, cx);
    }

    /// Gives the active editor keyboard focus.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.state.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    /// Switches the tone: editor colours and syntax colours, without recreating the editors.
    pub fn set_tone(&mut self, tone: Tone, cx: &mut Context<Self>) {
        self.set_theme(
            Theme::of(Appearance {
                skin: Skin::Standard,
                tone,
            }),
            cx,
        );
    }

    /// Switches skin and tone together.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        let style = editor_style(theme);
        for tab in &self.tabs {
            tab.state
                .update(cx, |state, _| state.set_editor_style(style.clone()));
            if let Some(right) = &tab.diff_right {
                right.update(cx, |state, _| state.set_editor_style(style.clone()));
            }
            if let Some(diff) = &tab.diff {
                diff.update(cx, |state, _| state.set_editor_style(style.clone()));
            }
        }
        cx.notify();
    }

    /// Loads a document into its tab: a new tab when the path is not open, a reload when it is.
    fn install_document(
        &mut self,
        document: Document,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let enabled = highlight_enabled(document.content.len());
        let language = language_for_path(&document.path);
        let content = document.content.clone();
        let path = document.path.clone();

        let index = match self.position_of(&path) {
            Some(index) => index,
            None => {
                let state = cx.new(|cx| new_editor_state(self.theme, window, cx));
                let view = cx.entity().downgrade();
                state.update(cx, |state, _| {
                    state.on_context_menu(std::rc::Rc::new(
                        move |_menu, capabilities, position, _window, cx| {
                            let _ = view.update(cx, |_, cx| {
                                cx.emit(EditorEvent::TextContextMenu {
                                    position,
                                    selection: capabilities.has_selection(),
                                    editable: capabilities.is_editable(),
                                });
                            });
                        },
                    ));
                });
                if let Some(requester) = &self.requester {
                    self.attach_language_features(&state, &path, requester, cx);
                }
                let tab_path = path.clone();
                let subscription =
                    cx.subscribe(&state, move |this, _state, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.on_edited(&tab_path, cx);
                        }
                    });
                let cursor = cx.observe(&state, |_, _, cx| cx.notify());
                self.tabs.push(Tab {
                    state,
                    session: DocumentSession::new(),
                    highlighting: true,
                    diff: None,
                    diff_right: None,
                    diff_raw: String::new(),
                    diff_visible: false,
                    _subscription: subscription,
                    _cursor: cursor,
                });
                self.tabs.len() - 1
            }
        };

        let tab = &mut self.tabs[index];
        tab.state.update(cx, |state, cx| {
            state.set_highlighter_factory(highlighter_factory(), cx);
            // An unknown language name has no grammar, so the factory answers `None`.
            let name = match (enabled, language) {
                (true, Some(language)) => language,
                _ => "plain",
            };
            state.set_highlighter(name, cx);
            state.set_value(content.as_str(), window, cx);
            if let Some(line) = line {
                // A search hit's line is 1-based; an editor position is 0-based.
                let line = u32::try_from(line.saturating_sub(1)).unwrap_or(u32::MAX);
                state.set_cursor_position(Position::new(line, 0), window, cx);
            }
        });
        tab.highlighting = enabled;
        tab.session.opened(&document);
        tab.diff_visible = false;
        self.status = (!enabled).then(|| {
            SharedString::from(format!(
                "Büyük dosya ({} KiB üstü): renklendirme kapalı",
                HIGHLIGHT_LIMIT_BYTES / 1024
            ))
        });
        self.active = index;
        self.fix_split(cx);
        cx.emit(EditorEvent::Opened(path));
        cx.notify();
        // The editor lays a new text out during paint; without input nothing else would ask for
        // the frame that shows it.
        window.refresh();
        // The next keystroke belongs to the file that was just opened.
        self.focus(window, cx);
    }

    fn on_edited(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        self.tabs[index].session.edited();
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// Loads a document directly into its tab (tests and the bench use this; the workbench goes
    /// through [`Self::open_path_at`], which reads on the background executor).
    pub fn open_document(
        &mut self,
        document: Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.install_document(document, None, window, cx);
    }

    /// Reads `path` on the background executor and opens it (a new tab, or a reload of its own
    /// tab). A file that cannot be edited (binary, not UTF-8, over 2 MiB) leaves every tab
    /// untouched and shows a message. Reloading a tab with unsaved edits is refused: callers
    /// ask the user first.
    pub fn open_path(
        &mut self,
        project: Arc<ProjectRoot>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_path_at(project, path, None, window, cx);
    }

    /// Like [`Self::open_path`], with the cursor left on `line` (1-based) once it is loaded.
    pub fn open_path_at(
        &mut self,
        project: Arc<ProjectRoot>,
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.position_of(&path) {
            if self.tabs[index].session.is_dirty() {
                self.set_status(
                    "Kaydedilmemiş değişiklikler var; önce kaydedin veya vazgeçin",
                    cx,
                );
                return;
            }
        } else if self.tabs.len() >= MAX_SAVED_TABS {
            self.set_status(format!("Sekme sınırına ulaşıldı ({MAX_SAVED_TABS})"), cx);
            return;
        }
        self.next_request += 1;
        let request = self.next_request;
        self.requests.insert(path.clone(), request);
        cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { project.read_document(&read_path) })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.requests.get(&path) != Some(&request) {
                    return;
                }
                view.requests.remove(&path);
                match result {
                    Ok(document) => view.install_document(document, line, window, cx),
                    Err(error) => view.set_status(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Path and 1-based cursor line of every tab, in strip order: what the saved workspace
    /// stores so a project reopens as it was left.
    pub fn tabs_snapshot(&self, cx: &gpui::App) -> Vec<SavedTab> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let path = tab.path()?.to_owned();
                let position = tab.state.read(cx).cursor_position();
                let line = usize::try_from(position.line).unwrap_or(usize::MAX) + 1;
                Some(SavedTab { path, line })
            })
            .collect()
    }

    /// Connects every tab (and the ones opened later) to the project's language servers, or
    /// disconnects them with `None`.
    pub fn set_language_requester(
        &mut self,
        requester: Option<rwk_core::lsp::Requester>,
        cx: &mut Context<Self>,
    ) {
        self.requester = requester;
        let tabs: Vec<(Entity<EditorState>, String)> = self
            .tabs
            .iter()
            .filter_map(|tab| Some((tab.state.clone(), tab.path()?.to_owned())))
            .collect();
        for (state, path) in tabs {
            match self.requester.clone() {
                Some(requester) => self.attach_language_features(&state, &path, &requester, cx),
                None => state.update(cx, |state, _| super::lsp::detach(state)),
            }
        }
    }

    fn attach_language_features(
        &self,
        state: &Entity<EditorState>,
        path: &str,
        requester: &rwk_core::lsp::Requester,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        let open: super::lsp::OpenLocation = Rc::new(move |path, line, cx| {
            let _ = view.update(cx, |_, cx| {
                cx.emit(EditorEvent::OpenLocation { path, line })
            });
        });
        state.update(cx, |state, _| {
            super::lsp::attach(state, path, requester, open)
        });
    }

    /// The symbol under the cursor: path, 0-based line and column, and its name.
    pub fn symbol_at_cursor(&self, cx: &gpui::App) -> Option<(String, u32, u32, String)> {
        let tab = self.tabs.get(self.active)?;
        let path = tab.path()?.to_owned();
        let state = tab.state.read(cx);
        let position = state.cursor_position();
        let text = state.value().to_string();
        let line = text.lines().nth(usize::try_from(position.line).ok()?)?;
        let word = rwk_core::lsp::edits::word_at(line, position.character)?;
        Some((path, position.line, position.character, word))
    }

    /// Renames the symbol under the cursor to `new_name` in every file the language server
    /// names. All files are prepared before any is written, so a refused or broken answer
    /// changes nothing; open tabs take the edits as unsaved changes, other files are written.
    pub fn rename_symbol(
        &mut self,
        project: Arc<ProjectRoot>,
        new_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(requester), Some((path, line, character, _))) =
            (self.requester.clone(), self.symbol_at_cursor(cx))
        else {
            self.set_status("İmleç bir sembolün üzerinde değil", cx);
            return;
        };
        let Some(before) = self.text_of(&path, cx) else {
            return;
        };
        self.set_status("Yeniden adlandırılıyor…", cx);
        let answer = super::lsp::rename_request(
            &requester,
            &path,
            line,
            character,
            &new_name,
            std::time::Duration::from_secs(10),
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            this.update_in(cx, |view, window, cx| {
                if view.text_of(&path, cx).as_deref() != Some(before.as_str()) {
                    view.set_status("Metin değişti; yeniden adlandırma uygulanmadı", cx);
                    return;
                }
                let outcome = match answer {
                    Ok(Ok(value)) => {
                        view.apply_workspace_edit(&project, &requester, value, window, cx)
                    }
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(AppError::new(
                        ErrorCode::NotFound,
                        "Dil sunucusu yanıt vermedi",
                    )),
                };
                match outcome {
                    Ok((0, _)) => view.set_status("Sunucu bir değişiklik önermedi", cx),
                    Ok((files, edits)) => {
                        view.set_status(format!("{files} dosyada {edits} değişiklik"), cx)
                    }
                    Err(error) => view.set_status(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Applies a server's workspace edit; returns how many files and edits it touched.
    fn apply_workspace_edit(
        &mut self,
        project: &ProjectRoot,
        requester: &rwk_core::lsp::Requester,
        answer: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> rwk_core::error::Result<(usize, usize)> {
        let files =
            rwk_core::lsp::edits::workspace_edit(answer, |uri| requester.relative_path(uri))?;
        // Prepare the files that are not open first: nothing is written unless all are fine.
        let mut rewrites = Vec::new();
        for file in &files {
            if self.position_of(&file.path).is_none() {
                let document = project.read_document(&file.path)?;
                let content = rwk_core::lsp::edits::apply(&document.content, &file.edits)?;
                rewrites.push(Document {
                    content,
                    ..document
                });
            }
        }
        let edits = files.iter().map(|file| file.edits.len()).sum();
        for document in rewrites {
            project.save_document(document)?;
        }
        for file in &files {
            if let Some(index) = self.position_of(&file.path) {
                self.tabs[index].state.update(cx, |state, cx| {
                    state.apply_lsp_edits(&file.edits, window, cx)
                });
            }
        }
        Ok((files.len(), edits))
    }

    /// Formats the active file with its language server. The answer is dropped, with a message,
    /// when the text changed while the server worked: its positions would no longer fit.
    pub fn format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.format_active(None, window, cx);
    }

    /// Formats the active file, then saves it. A file without a language server, a slow or
    /// failing server and edits made while it worked all end in a plain save: saving never
    /// waits long and never fails because of the formatter.
    pub fn save_formatted(
        &mut self,
        project: Arc<ProjectRoot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.requester.is_none() {
            self.save(project, cx);
            return;
        }
        self.format_active(Some(project), window, cx);
    }

    fn format_active(
        &mut self,
        then_save: Option<Arc<ProjectRoot>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(requester), Some(path)) =
            (self.requester.clone(), self.path().map(str::to_owned))
        else {
            self.set_status("Bu dosya için dil sunucusu yok", cx);
            return;
        };
        let Some(before) = self.text_of(&path, cx) else {
            return;
        };
        let timeout = if then_save.is_some() {
            FORMAT_ON_SAVE_WAIT
        } else {
            std::time::Duration::from_secs(5)
        };
        let answer = super::lsp::format_request(&requester, &path, timeout);
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.recv().await;
            this.update_in(cx, |view, window, cx| {
                let Some(index) = view.position_of(&path) else {
                    return;
                };
                if view.text_of(&path, cx).as_deref() != Some(before.as_str()) {
                    view.set_status("Metin değişti; biçimlendirme uygulanmadı", cx);
                    if let Some(project) = then_save {
                        view.save_soon(path.clone(), project, cx);
                    }
                    return;
                }
                let edits = match answer {
                    Ok(Ok(value)) => super::lsp::formatting_edits(value),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(AppError::new(
                        ErrorCode::NotFound,
                        "Dil sunucusu yanıt vermedi",
                    )),
                };
                match edits {
                    Ok(edits) if edits.is_empty() => view.set_status("Zaten biçimli", cx),
                    Ok(edits) => {
                        view.tabs[index]
                            .state
                            .update(cx, |state, cx| state.apply_lsp_edits(&edits, window, cx));
                        view.set_status("Biçimlendirildi", cx);
                    }
                    Err(error) => view.set_status(error.message, cx),
                }
                if let Some(project) = then_save {
                    view.save_soon(path.clone(), project, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Saves `path` once the edits just applied have been announced: the edit events arrive after
    /// the update that made them, and a save that began before them would find its file dirty
    /// again as soon as it finished.
    fn save_soon(&self, path: String, project: Arc<ProjectRoot>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            this.update(cx, |view, cx| view.save_path(&path, project, cx))
                .ok();
        })
        .detach();
    }

    /// The file shown beside the active one, when the editor is split.
    pub fn split_path(&self) -> Option<&str> {
        self.split.as_deref()
    }

    /// Splits the editor, showing the next open file read-only beside the active one; splits
    /// back into one pane when it is split already.
    pub fn toggle_split(&mut self, cx: &mut Context<Self>) {
        if self.split.take().is_some() {
            self.fix_split(cx);
            cx.notify();
            return;
        }
        if self.tabs.len() < 2 {
            self.set_status("Bölmek için ikinci bir dosya açın", cx);
            return;
        }
        let next = (self.active + 1) % self.tabs.len();
        self.split = self.tabs[next].path().map(str::to_owned);
        self.fix_split(cx);
        cx.notify();
    }

    /// Trades the active file and the one beside it, so the right one can be edited.
    pub fn swap_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.split.clone() {
            self.activate_tab(&path, window, cx);
        }
    }

    /// Keeps the split valid after tabs came, went or changed place: the file beside the active
    /// one must be open and a different one, and only it is read-only.
    fn fix_split(&mut self, cx: &mut Context<Self>) {
        let active = self
            .tabs
            .get(self.active)
            .and_then(Tab::path)
            .map(str::to_owned);
        if let Some(split) = &self.split {
            if self.position_of(split).is_none() || active.as_deref() == Some(split.as_str()) {
                self.split = None;
            }
        }
        for tab in &self.tabs {
            let beside = tab.path().is_some() && tab.path() == self.split.as_deref();
            tab.state
                .update(cx, |state, cx| state.set_disabled(beside, cx));
        }
    }

    /// Opens the find box of the active file (with the replace field when `replace`).
    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        tab.state.update(cx, |state, cx| {
            state.open_search(replace, cx);
        });
        let _ = window;
    }

    /// Whether the active file's find box is open.
    pub fn find_open(&self, cx: &gpui::App) -> bool {
        self.tabs
            .get(self.active)
            .is_some_and(|tab| tab.state.read(cx).search_session().is_active())
    }

    /// Replaces every occurrence of `query` in the active file with `replacement` as one
    /// undoable edit. Returns how many were replaced. The search is plain text.
    pub fn replace_all(
        &mut self,
        query: &str,
        replacement: &str,
        case_insensitive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(tab) = self.tabs.get(self.active) else {
            return 0;
        };
        tab.state.update(cx, |state, cx| {
            state.set_search_query(query, case_insensitive, cx);
            let count = state.replace_all_search_matches(replacement, window, cx);
            state.close_search(cx);
            count
        })
    }

    /// The text of the tab for `path`, if it is open.
    pub fn text_of(&self, path: &str, cx: &gpui::App) -> Option<String> {
        let index = self.position_of(path)?;
        Some(self.tabs[index].state.read(cx).value().to_string())
    }

    /// Shows the language server's diagnostics for `path` under the text. A file that is not
    /// open is ignored.
    pub fn set_diagnostics(
        &mut self,
        path: &str,
        diagnostics: &[rwk_core::lsp::lsp_types::Diagnostic],
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        self.tabs[index].state.update(cx, |state, cx| {
            if let Some(set) = state.diagnostics_mut() {
                set.clear();
                set.extend(diagnostics.iter().cloned());
            }
            cx.notify();
        });
    }

    /// How many diagnostics the tab for `path` shows.
    pub fn diagnostic_count(&self, path: &str, cx: &gpui::App) -> usize {
        self.position_of(path)
            .and_then(|index| {
                self.tabs[index]
                    .state
                    .read(cx)
                    .diagnostics()
                    .map(|set| set.len())
            })
            .unwrap_or(0)
    }

    /// Index of the active tab (what the saved workspace stores).
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Restores a saved set of tabs: every file is read on the background executor, the ones
    /// that read cleanly become tabs in the saved order and `active` picks the active tab.
    /// Files that no longer read (gone, binary, over the size limit) are skipped silently.
    /// A restore that finishes after a project switch is dropped.
    pub fn restore_tabs(
        &mut self,
        project: Arc<ProjectRoot>,
        tabs: Vec<SavedTab>,
        active: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs: Vec<SavedTab> = tabs.into_iter().take(MAX_SAVED_TABS).collect();
        if tabs.is_empty() {
            return;
        }
        self.restore_generation += 1;
        let generation = self.restore_generation;
        cx.spawn_in(window, async move |this, cx| {
            let mut read = Vec::new();
            for tab in tabs {
                let project = project.clone();
                let path = tab.path.clone();
                let document = cx
                    .background_executor()
                    .spawn(async move { project.read_document(&path) })
                    .await;
                read.push((tab, document));
            }
            this.update_in(cx, |view, window, cx| {
                if view.restore_generation != generation {
                    return;
                }
                let mut installed: Vec<String> = Vec::new();
                for (tab, document) in read {
                    if let Ok(document) = document {
                        let path = document.path.clone();
                        view.install_document(document, Some(tab.line), window, cx);
                        installed.push(path);
                    }
                }
                if let Some(path) = installed.get(active) {
                    view.activate_tab(path, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Saves the active file through the revision-checked path.
    pub fn save(&mut self, project: Arc<ProjectRoot>, cx: &mut Context<Self>) {
        let Some(path) = self.path().map(str::to_owned) else {
            self.set_status("Açık dosya yok", cx);
            return;
        };
        self.save_path(&path, project, cx);
    }

    /// Saves one tab through the revision-checked path.
    pub fn save_path(&mut self, path: &str, project: Arc<ProjectRoot>, cx: &mut Context<Self>) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        let text = self.tabs[index].state.read(cx).value().to_string();
        let (document, ticket) = match self.tabs[index].session.begin_save(&text) {
            Ok(started) => started,
            Err(SaveBlocked::NoDocument) => {
                self.set_status("Açık dosya yok", cx);
                return;
            }
            Err(SaveBlocked::Unchanged) => {
                self.set_status("Kaydedilecek değişiklik yok", cx);
                return;
            }
            Err(SaveBlocked::Conflict) => {
                self.set_status(
                    "Dosya diskte değişti. Düzenlemeleriniz korundu; yeniden yükleyin.",
                    cx,
                );
                cx.emit(EditorEvent::Conflict);
                return;
            }
        };
        let path = path.to_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { project.save_document(document) })
                .await;
            this.update(cx, |view, cx| view.finish_save(&path, result, ticket, cx))
                .ok();
        })
        .detach();
    }

    fn finish_save(
        &mut self,
        path: &str,
        result: Result<Document, AppError>,
        ticket: SaveTicket,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.position_of(path) else {
            return;
        };
        match result {
            Ok(saved) => {
                self.tabs[index].session.save_succeeded(&saved, ticket);
                self.set_status("Kaydedildi", cx);
                cx.emit(EditorEvent::Saved(saved));
            }
            Err(error) if error.code == ErrorCode::Conflict => {
                self.tabs[index].session.save_failed(&error);
                self.set_status(
                    "Dosya diskte değişti. Düzenlemeleriniz korundu; yeniden yükleyin.",
                    cx,
                );
                cx.emit(EditorEvent::Conflict);
            }
            Err(error) => {
                self.tabs[index].session.save_failed(&error);
                self.set_status(error.message.clone(), cx);
                cx.emit(EditorEvent::SaveFailed(error));
            }
        }
    }

    /// Shows the Git diff of the active file in a read-only editor.
    pub fn show_diff(
        &mut self,
        project: Arc<ProjectRoot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.path().map(str::to_owned) else {
            self.set_status("Açık dosya yok", cx);
            return;
        };
        self.diff_generation += 1;
        let generation = self.diff_generation;
        cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { project.git_diff(&read_path) })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.diff_generation != generation {
                    return;
                }
                // The diff belongs to its file: a tab switch while the read ran must not put it
                // under another one.
                let Some(index) = view.position_of(&path) else {
                    return;
                };
                match result {
                    Ok(text) => view.install_diff(index, text, window, cx),
                    Err(error) => view.set_status(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn new_diff_state(&self, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorState> {
        cx.new(|cx| {
            EditorState::new(window, cx)
                .soft_wrap(false)
                .line_number(false)
                .folding(false)
        })
    }

    fn fill_diff_state(
        &self,
        state: &Entity<EditorState>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme = self.theme;
        state.update(cx, |state, cx| {
            state.set_editor_style(editor_style(theme));
            state.set_highlighter_factory(
                Rc::new(|_: &str| {
                    Some(Box::new(DiffHighlighter::default()) as Box<dyn InputHighlighter>)
                }),
                cx,
            );
            state.set_highlighter("diff", cx);
            state.set_value(text, window, cx);
            state.set_disabled(true, cx);
        });
    }

    fn install_diff(
        &mut self,
        index: usize,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tabs[index].diff_raw = text;
        self.render_diff(index, window, cx);
        self.tabs[index].diff_visible = true;
        cx.notify();
    }

    /// Fills the diff editors of a tab from the stored diff: one unified column, or two aligned
    /// ones.
    fn render_diff(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let left = match &self.tabs[index].diff {
            Some(diff) => diff.clone(),
            None => {
                let diff = self.new_diff_state(window, cx);
                self.tabs[index].diff = Some(diff.clone());
                diff
            }
        };
        if self.diff_split {
            let columns = side_by_side(&self.tabs[index].diff_raw);
            let right = match &self.tabs[index].diff_right {
                Some(right) => right.clone(),
                None => {
                    let right = self.new_diff_state(window, cx);
                    self.tabs[index].diff_right = Some(right.clone());
                    right
                }
            };
            self.fill_diff_state(&left, &columns.left, window, cx);
            self.fill_diff_state(&right, &columns.right, window, cx);
        } else {
            let raw = self.tabs[index].diff_raw.clone();
            self.fill_diff_state(&left, &raw, window, cx);
        }
    }

    /// Switches the diff between one unified column and two side-by-side columns.
    pub fn set_diff_split(&mut self, split: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_split = split;
        if self
            .tabs
            .get(self.active)
            .is_some_and(|tab| tab.diff.is_some())
        {
            self.render_diff(self.active, window, cx);
        }
        cx.notify();
    }

    /// True when the diff is shown side by side.
    pub fn diff_split(&self) -> bool {
        self.diff_split
    }

    /// The text of the right column while the diff is side by side.
    pub fn diff_right_text(&self, cx: &gpui::App) -> Option<String> {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.diff_right.as_ref().filter(|_| self.diff_split))
            .map(|right| right.read(cx).value().to_string())
    }

    /// Returns to the editor from the diff view.
    pub fn hide_diff(&mut self, cx: &mut Context<Self>) {
        self.diff_generation += 1;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.diff_visible = false;
        }
        cx.notify();
    }

    /// True while the diff view of the active file is shown.
    pub fn diff_visible(&self) -> bool {
        self.tabs
            .get(self.active)
            .is_some_and(|tab| tab.diff_visible)
    }

    /// The text of the diff view.
    pub fn diff_text(&self, cx: &gpui::App) -> String {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.diff.as_ref())
            .map(|diff| diff.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// True when the diff view cannot be edited.
    pub fn diff_is_read_only(&self, cx: &gpui::App) -> bool {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.diff.as_ref())
            .is_some_and(|diff| !diff.read(cx).is_editable())
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let strip = (!self.tabs.is_empty()).then(|| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_px()
                .bg(gpui::rgb(theme.bg))
                .border_b_1()
                .border_color(gpui::rgb(theme.line))
                .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                    let path = tab.path().unwrap_or_default().to_owned();
                    let title = match tab.session.is_dirty() {
                        true => format!("{path} ●"),
                        false => path.clone(),
                    };
                    let badge = self.badges.get(&path).copied();
                    let active = index == self.active;
                    let activate = path.clone();
                    let close = path.clone();
                    let menu_path = path.clone();
                    div()
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                                view.activate_tab(&menu_path, window, cx);
                                cx.emit(EditorEvent::TabContextMenu {
                                    path: menu_path.clone(),
                                    position: event.position,
                                });
                                cx.stop_propagation();
                            }),
                        )
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .pl_3()
                        .pr_2()
                        .py_1p5()
                        .max_w(gpui::px(220.0))
                        .overflow_hidden()
                        .text_xs()
                        .whitespace_nowrap()
                        .border_t_2()
                        .border_color(gpui::rgb(if active { theme.accent } else { theme.panel }))
                        .bg(gpui::rgb(if active { theme.editor } else { theme.panel }))
                        .text_color(gpui::rgb(if active { theme.text } else { theme.muted }))
                        .hover(move |style| {
                            if active {
                                style
                            } else {
                                style.bg(gpui::rgb(theme.raised))
                            }
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!("tab-{path}")))
                                .overflow_hidden()
                                .cursor_pointer()
                                .child(title)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.activate_tab(&activate, window, cx);
                                })),
                        )
                        .children(badge.map(|badge| {
                            div()
                                .text_color(gpui::rgb(crate::file_tree::view::badge_color(
                                    badge.kind, theme.dark,
                                )))
                                .child(SharedString::from(badge.letter.to_string()))
                        }))
                        .child(
                            div()
                                .id(SharedString::from(format!("tab-close-{path}")))
                                .px_1()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                                .child("×")
                                .on_click(cx.listener(move |_, _, _window, cx| {
                                    cx.emit(EditorEvent::CloseRequested(close.clone()));
                                })),
                        )
                }))
        });

        let shown = match (self.tabs.get(self.active), self.diff_visible()) {
            (Some(tab), true) => match &tab.diff {
                Some(diff) => diff,
                None => &tab.state,
            },
            (Some(tab), false) => &tab.state,
            (None, _) => {
                return div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(strip)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p_2()
                            .text_sm()
                            .text_color(gpui::rgb(theme.muted))
                            .child("Dosya açmak için soldaki ağaçtan seçin"),
                    )
                    .child(self.status_bar(None, theme, cx));
            }
        };
        let second = self.tabs.get(self.active).and_then(|tab| {
            tab.diff_right
                .as_ref()
                .filter(|_| tab.diff_visible && self.diff_split)
        });
        let title = self.path().unwrap_or("").to_owned();
        let beside = match (second, self.split.as_deref()) {
            (None, Some(path)) => self
                .position_of(path)
                .map(|index| (path.to_owned(), self.tabs[index].state.clone())),
            _ => None,
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .children(strip)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .font_family(crate::fonts::monospace_family(window))
                    .child(div().flex_1().min_w_0().child(Editor::new(shown)))
                    .children(beside.map(|(path, state)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .border_l_1()
                            .border_color(gpui::rgb(theme.line))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_0p5()
                                    .text_xs()
                                    .bg(gpui::rgb(theme.panel))
                                    .text_color(gpui::rgb(theme.muted))
                                    .child(div().flex_1().min_w_0().child(path))
                                    .child(
                                        div()
                                            .id("split-swap")
                                            .px_1()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                                            .child("↔ düzenle")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.swap_split(window, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("split-close")
                                            .px_1()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                                            .child("×")
                                            .on_click(cx.listener(|view, _, _window, cx| {
                                                view.toggle_split(cx);
                                            })),
                                    ),
                            )
                            .child(div().flex_1().min_h_0().child(Editor::new(&state)))
                    }))
                    .children(second.map(|right| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_l_1()
                            .border_color(gpui::rgb(theme.line))
                            .child(Editor::new(right))
                    })),
            )
            .child(self.status_bar(Some(title), theme, cx))
    }
}

impl EditorView {
    /// "Ln 3, Süt 14 · rust": where the cursor is and which language the file is read as.
    pub fn position_label(&self, cx: &gpui::App) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        let path = tab.path()?;
        let position = tab.state.read(cx).cursor_position();
        let language = super::highlight::language_for_path(path).unwrap_or("düz metin");
        Some(format!(
            "Sat {}, Süt {} · {language}",
            position.line.saturating_add(1),
            position.character.saturating_add(1)
        ))
    }

    fn status_bar(&self, title: Option<String>, theme: Theme, cx: &gpui::App) -> impl IntoElement {
        let title = title.unwrap_or_default();
        let marker = if self.is_dirty() { " ●" } else { "" };
        let status = self.status.clone().unwrap_or_default();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_0p5()
            .border_t_1()
            .border_color(gpui::rgb(theme.line))
            .bg(gpui::rgb(theme.panel))
            .text_xs()
            .text_color(gpui::rgb(theme.muted))
            .child(format!("{title}{marker}"))
            .child(div().flex_1().min_w_0().child(status))
            .children(
                self.position_label(cx)
                    .map(|label| div().flex_none().child(label)),
            )
    }
}
