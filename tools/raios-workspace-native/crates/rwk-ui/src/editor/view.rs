//! The editor view: one open file, its edit/save state and the read-only diff view.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window,
};
use gpui_base::input::{Editor, EditorState, InputEvent, InputHighlighter, Position};
use rwk_core::documents::Document;
use rwk_core::error::{AppError, ErrorCode};
use rwk_core::project::{ProjectId, ProjectRoot};
use rwk_core::workbench::{
    side_by_side, Appearance, DocumentSession, SaveBlocked, SaveTicket, Skin, SwitchDecision,
    Theme, Tone,
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

/// One file open for editing, plus the optional Git diff of it.
pub struct EditorView {
    theme: Theme,
    state: Entity<EditorState>,
    diff: Option<Entity<EditorState>>,
    /// The right column while the diff is shown side by side (`diff` is then the left one).
    diff_right: Option<Entity<EditorState>>,
    diff_split: bool,
    diff_raw: String,
    diff_visible: bool,
    session: DocumentSession,
    status: Option<SharedString>,
    highlighting: bool,
    /// Bumped by every open request; a read that finishes for an older request is dropped.
    open_generation: u64,
    /// The 1-based line the next successful open puts the cursor on (a search hit's line).
    pending_line: Option<usize>,
    diff_generation: u64,
    _subscription: Subscription,
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
    pub fn themed(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| new_editor_state(theme, window, cx));
        let subscription = cx.subscribe(&state, |this, _state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.session.edited();
                cx.emit(EditorEvent::Edited);
                cx.notify();
            }
        });
        Self {
            theme,
            state,
            diff: None,
            diff_right: None,
            diff_split: false,
            diff_raw: String::new(),
            diff_visible: false,
            session: DocumentSession::new(),
            status: None,
            highlighting: true,
            open_generation: 0,
            pending_line: None,
            diff_generation: 0,
            _subscription: subscription,
        }
    }

    /// The text in the editor.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.state.read(cx).value().to_string()
    }

    /// The 1-based line of the cursor, when a document is open.
    pub fn cursor_line(&self, cx: &gpui::App) -> Option<usize> {
        self.path()?;
        let position = self.state.read(cx).cursor_position();
        Some(usize::try_from(position.line).unwrap_or(usize::MAX) + 1)
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
        self.state
            .update(cx, |state, _| state.set_editor_style(style.clone()));
        if let Some(right) = &self.diff_right {
            right.update(cx, |state, _| state.set_editor_style(style.clone()));
        }
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
        let pending_line = self.pending_line.take();
        self.state.update(cx, |state, cx| {
            state.set_highlighter_factory(highlighter_factory(), cx);
            // An unknown language name has no grammar, so the factory answers `None`.
            let name = match (enabled, language) {
                (true, Some(language)) => language,
                _ => "plain",
            };
            state.set_highlighter(name, cx);
            state.set_value(content.as_str(), window, cx);
            if let Some(line) = pending_line {
                // A search hit's line is 1-based; an editor position is 0-based.
                let line = u32::try_from(line.saturating_sub(1)).unwrap_or(u32::MAX);
                state.set_cursor_position(Position::new(line, 0), window, cx);
            }
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
        self.pending_line = line;
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
                    Err(error) => {
                        view.pending_line = None;
                        view.set_status(error.message, cx);
                    }
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

    fn install_diff(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_raw = text;
        self.render_diff(window, cx);
        self.diff_visible = true;
        cx.notify();
    }

    /// Fills the diff editors from the stored diff: one unified column, or two aligned ones.
    fn render_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let left = match &self.diff {
            Some(diff) => diff.clone(),
            None => {
                let diff = self.new_diff_state(window, cx);
                self.diff = Some(diff.clone());
                diff
            }
        };
        if self.diff_split {
            let columns = side_by_side(&self.diff_raw);
            let right = match &self.diff_right {
                Some(right) => right.clone(),
                None => {
                    let right = self.new_diff_state(window, cx);
                    self.diff_right = Some(right.clone());
                    right
                }
            };
            self.fill_diff_state(&left, &columns.left, window, cx);
            self.fill_diff_state(&right, &columns.right, window, cx);
        } else {
            let raw = self.diff_raw.clone();
            self.fill_diff_state(&left, &raw, window, cx);
        }
    }

    /// Switches the diff between one unified column and two side-by-side columns.
    pub fn set_diff_split(&mut self, split: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_split = split;
        if self.diff.is_some() {
            self.render_diff(window, cx);
        }
        cx.notify();
    }

    /// True when the diff is shown side by side.
    pub fn diff_split(&self) -> bool {
        self.diff_split
    }

    /// The text of the right column while the diff is side by side.
    pub fn diff_right_text(&self, cx: &gpui::App) -> Option<String> {
        self.diff_right
            .as_ref()
            .filter(|_| self.diff_split)
            .map(|right| right.read(cx).value().to_string())
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
        let second = match (&self.diff_right, self.diff_visible && self.diff_split) {
            (Some(right), true) => Some(right),
            _ => None,
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
                    .flex()
                    .flex_row()
                    .font_family(crate::fonts::monospace_family(window))
                    .child(div().flex_1().min_w_0().child(Editor::new(shown)))
                    .children(second.map(|right| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_l_1()
                            .border_color(gpui::rgb(self.theme.line))
                            .child(Editor::new(right))
                    })),
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
