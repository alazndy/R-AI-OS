//! The editor view: one open file, its edit/save state and the read-only diff view.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window,
};
use gpui_base::input::{Editor, EditorState, InputEvent, InputHighlighter};
use rwk_core::documents::Document;
use rwk_core::error::{AppError, ErrorCode};
use rwk_core::project::{ProjectId, ProjectRoot};
use rwk_core::workbench::{DocumentSession, SaveBlocked, SaveTicket, SwitchDecision, Tone};

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
}

/// Builds an editor state with the settings every editor in the workspace uses: soft wrap off
/// (spec section 2: it costs about 0.26 ms per line), line numbers and folding on.
fn new_editor_state(tone: Tone, window: &mut Window, cx: &mut Context<EditorState>) -> EditorState {
    let mut state = EditorState::new(window, cx)
        .soft_wrap(false)
        .line_number(true)
        .folding(true);
    state.set_editor_style(editor_style(tone));
    state
}

/// One file open for editing, plus the optional Git diff of it.
pub struct EditorView {
    tone: Tone,
    state: Entity<EditorState>,
    diff: Option<Entity<EditorState>>,
    diff_visible: bool,
    session: DocumentSession,
    status: Option<SharedString>,
    highlighting: bool,
    /// Bumped by every open request; a read that finishes for an older request is dropped.
    open_generation: u64,
    diff_generation: u64,
    _subscription: Subscription,
}

impl EventEmitter<EditorEvent> for EditorView {}

impl EditorView {
    /// An empty editor.
    pub fn new(tone: Tone, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| new_editor_state(tone, window, cx));
        let subscription = cx.subscribe(&state, |this, _state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.session.edited();
                cx.emit(EditorEvent::Edited);
                cx.notify();
            }
        });
        Self {
            tone,
            state,
            diff: None,
            diff_visible: false,
            session: DocumentSession::new(),
            status: None,
            highlighting: true,
            open_generation: 0,
            diff_generation: 0,
            _subscription: subscription,
        }
    }

    /// The text in the editor.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.state.read(cx).value().to_string()
    }

    /// Path of the open file, relative to its project.
    pub fn path(&self) -> Option<&str> {
        self.session.path()
    }

    /// True when the open file has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// True when the last save found the file changed on disk.
    pub fn has_conflict(&self) -> bool {
        self.session.has_conflict()
    }

    /// The message shown under the editor.
    pub fn status(&self) -> Option<String> {
        self.status.as_ref().map(ToString::to_string)
    }

    /// False when the open file is above the highlight limit.
    pub fn highlighting_enabled(&self) -> bool {
        self.highlighting
    }

    /// Whether the user may open `path` of `project` now.
    pub fn request_open(&self, project: &ProjectId, path: &str) -> SwitchDecision {
        self.session.request_open(project, path)
    }

    /// Whether the window or project may close now.
    pub fn request_leave(&self) -> SwitchDecision {
        self.session.request_leave()
    }

    fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(message.into());
        cx.notify();
    }

    /// Inserts text at the cursor, as typing does.
    pub fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_owned();
        self.state
            .update(cx, |state, cx| state.insert(text.as_str(), window, cx));
    }

    /// Throws the open file's text away and closes it (the user chose to discard).
    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.close();
        self.state
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.status = None;
        self.diff_visible = false;
        cx.notify();
    }

    /// Gives the editor keyboard focus.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    /// Switches the tone: editor colours and syntax colours, without recreating the editors.
    pub fn set_tone(&mut self, tone: Tone, cx: &mut Context<Self>) {
        self.tone = tone;
        let style = editor_style(tone);
        self.state
            .update(cx, |state, _| state.set_editor_style(style.clone()));
        if let Some(diff) = &self.diff {
            diff.update(cx, |state, _| state.set_editor_style(style));
        }
        cx.notify();
    }

    /// Loads a document into the editor. Callers ask [`Self::request_open`] first.
    pub fn open(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let enabled = highlight_enabled(document.content.len());
        let language = language_for_path(&document.path);
        let content = document.content.clone();
        self.state.update(cx, |state, cx| {
            state.set_highlighter_factory(highlighter_factory(), cx);
            // An unknown language name has no grammar, so the factory answers `None`.
            let name = match (enabled, language) {
                (true, Some(language)) => language,
                _ => "plain",
            };
            state.set_highlighter(name, cx);
            state.set_value(content.as_str(), window, cx);
        });
        self.highlighting = enabled;
        self.status = (!enabled).then(|| {
            SharedString::from(format!(
                "Büyük dosya ({} KiB üstü): renklendirme kapalı",
                HIGHLIGHT_LIMIT_BYTES / 1024
            ))
        });
        self.session.opened(&document);
        self.diff_visible = false;
        cx.emit(EditorEvent::Opened(document.path));
        cx.notify();
        // The editor lays a new text out during paint; without input nothing else would ask for
        // the frame that shows it.
        window.refresh();
        // The next keystroke belongs to the file that was just opened.
        self.focus(window, cx);
    }

    /// Reads `path` on the background executor and opens it. A file that cannot be edited
    /// (binary, not UTF-8, over 2 MiB) leaves the open file untouched and shows a message.
    pub fn open_path(
        &mut self,
        project: Arc<ProjectRoot>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Fail-safe: callers are expected to have asked the user, but unsaved edits are never
        // replaced without a decision, whoever calls and whichever file is meant (reloading the
        // open file would overwrite them as well). Discarding first makes the file clean.
        if self.session.is_dirty() {
            self.set_status(
                "Kaydedilmemiş değişiklikler var; önce kaydedin veya vazgeçin",
                cx,
            );
            return;
        }
        self.open_generation += 1;
        let generation = self.open_generation;
        cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { project.read_document(&read_path) })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.open_generation != generation {
                    return;
                }
                match result {
                    Ok(document) => view.open(document, window, cx),
                    Err(error) => view.set_status(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Saves the open file through the revision-checked path.
    pub fn save(&mut self, project: Arc<ProjectRoot>, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let (document, ticket) = match self.session.begin_save(&text) {
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
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { project.save_document(document) })
                .await;
            this.update(cx, |view, cx| view.finish_save(result, ticket, cx))
                .ok();
        })
        .detach();
    }

    fn finish_save(
        &mut self,
        result: Result<Document, AppError>,
        ticket: SaveTicket,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(saved) => {
                self.session.save_succeeded(&saved, ticket);
                self.set_status("Kaydedildi", cx);
                cx.emit(EditorEvent::Saved(saved));
            }
            Err(error) if error.code == ErrorCode::Conflict => {
                self.session.save_failed(&error);
                self.set_status(
                    "Dosya diskte değişti. Düzenlemeleriniz korundu; yeniden yükleyin.",
                    cx,
                );
                cx.emit(EditorEvent::Conflict);
            }
            Err(error) => {
                self.session.save_failed(&error);
                self.set_status(error.message.clone(), cx);
                cx.emit(EditorEvent::SaveFailed(error));
            }
        }
    }

    /// Shows the Git diff of the open file in a read-only editor.
    pub fn show_diff(
        &mut self,
        project: Arc<ProjectRoot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.session.path().map(str::to_owned) else {
            self.set_status("Açık dosya yok", cx);
            return;
        };
        self.diff_generation += 1;
        let generation = self.diff_generation;
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { project.git_diff(&path) })
                .await;
            this.update_in(cx, |view, window, cx| {
                if view.diff_generation != generation {
                    return;
                }
                match result {
                    Ok(text) => view.install_diff(text, window, cx),
                    Err(error) => view.set_status(error.message, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn install_diff(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let tone = self.tone;
        let diff = match &self.diff {
            Some(diff) => diff.clone(),
            None => {
                let diff = cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(false)
                        .line_number(false)
                        .folding(false)
                });
                self.diff = Some(diff.clone());
                diff
            }
        };
        diff.update(cx, |state, cx| {
            state.set_editor_style(editor_style(tone));
            state.set_highlighter_factory(
                Rc::new(|_: &str| {
                    Some(Box::new(DiffHighlighter::default()) as Box<dyn InputHighlighter>)
                }),
                cx,
            );
            state.set_highlighter("diff", cx);
            state.set_value(text.as_str(), window, cx);
            state.set_disabled(true, cx);
        });
        self.diff_visible = true;
        cx.notify();
    }

    /// Returns to the editor from the diff view.
    pub fn hide_diff(&mut self, cx: &mut Context<Self>) {
        self.diff_generation += 1;
        self.diff_visible = false;
        cx.notify();
    }

    /// True while the diff view is shown.
    pub fn diff_visible(&self) -> bool {
        self.diff_visible
    }

    /// The text of the diff view.
    pub fn diff_text(&self, cx: &gpui::App) -> String {
        self.diff
            .as_ref()
            .map(|diff| diff.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// True when the diff view cannot be edited.
    pub fn diff_is_read_only(&self, cx: &gpui::App) -> bool {
        self.diff
            .as_ref()
            .is_some_and(|diff| !diff.read(cx).is_editable())
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let shown = match (&self.diff, self.diff_visible) {
            (Some(diff), true) => diff,
            _ => &self.state,
        };
        let title = self.session.path().unwrap_or("").to_owned();
        let marker = if self.session.is_dirty() { " ●" } else { "" };
        let status = self.status.clone().unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .font_family(crate::fonts::monospace_family(window))
                    .child(Editor::new(shown)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .px_2()
                    .child(format!("{title}{marker}"))
                    .child(status),
            )
    }
}
