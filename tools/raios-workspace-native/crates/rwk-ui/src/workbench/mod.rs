//! The workbench: the window's three panes, the terminal dock and the single place where
//! commands take effect.

pub mod host;
pub mod layout;
pub mod palette;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window,
};
use gpui_base::{h_resizable, resizable_panel, ResizableState};
use rwk_core::control::ControlPlane;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, SessionRegistry, TerminalHost, TerminalSize};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};
use rwk_core::workbench::{
    available, resolve_shortcut, Appearance, CommandId, KeyboardContext, Keystroke, SwitchDecision,
    Tone, WorkbenchState,
};

use crate::editor::view::{EditorEvent, EditorView};
use crate::file_tree::view::{FileTreeEvent, FileTreeView};
use crate::inspector::InspectorView;
use crate::terminal::registry::GateRegistry;
use crate::terminal::{Appearance as TerminalAppearance, TerminalView};

use host::SharedHost;
use layout::LayoutPreset;
use palette::{PaletteEvent, PaletteView};

/// Size a new terminal starts with; the view resizes it to its pane at the first paint.
const NEW_TERMINAL_SIZE: TerminalSize = TerminalSize {
    cols: 100,
    rows: 30,
};

/// What the workbench tells the application around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchEvent {
    /// The user changed skin or tone; the application stores it.
    AppearanceChanged(Appearance),
}

impl gpui::EventEmitter<WorkbenchEvent> for Workbench {}

/// An action that waits for the user's decision about unsaved edits or a running session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Open this file in the editor.
    OpenFile(String),
    /// Open this folder as the project (unsaved edits are being asked about).
    OpenProject(PathBuf),
    /// Open this folder as the project; running sessions will be stopped, which needs a
    /// confirmation of its own when there are no unsaved edits to ask about.
    OpenProjectStoppingSessions(PathBuf),
    /// Close the window.
    CloseWindow,
    /// Stop this terminal session.
    StopTerminal(SessionId),
}

/// The user's answer to a pending action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Save the open file, then continue with the action.
    Save,
    /// Throw the edits away and continue.
    Discard,
    /// Go ahead (stopping a session).
    Confirm,
    /// Do nothing.
    Cancel,
}

/// What a dialog shows for a pending action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    /// Title line.
    pub title: &'static str,
    /// Explanation.
    pub message: String,
    /// The buttons, in order; the first one is the default (Enter), `Cancel` is Escape.
    pub buttons: Vec<(Decision, &'static str)>,
}

impl PendingAction {
    /// The dialog this action waits behind.
    pub fn dialog(&self) -> Dialog {
        match self {
            PendingAction::StopTerminal(_) => Dialog {
                title: "Oturumu durdur",
                message: "Seçili terminal oturumu durdurulacak.".to_owned(),
                buttons: vec![(Decision::Confirm, "Durdur"), (Decision::Cancel, "Vazgeç")],
            },
            PendingAction::OpenProjectStoppingSessions(_) => Dialog {
                title: "Projeyi değiştir",
                message: "Çalışan terminal ve ajan oturumları durdurulacak.".to_owned(),
                buttons: vec![(Decision::Confirm, "Değiştir"), (Decision::Cancel, "Vazgeç")],
            },
            other => Dialog {
                title: "Kaydedilmemiş değişiklikler",
                message: match other {
                    PendingAction::OpenFile(path) => {
                        format!("Açık dosyada kaydedilmemiş değişiklikler var; {path} açılmadan önce ne yapılsın?")
                    }
                    PendingAction::OpenProject(_) => {
                        "Açık dosyada kaydedilmemiş değişiklikler var; proje değişmeden önce ne yapılsın? Çalışan terminal oturumları da durdurulacak.".to_owned()
                    }
                    _ => "Açık dosyada kaydedilmemiş değişiklikler var; pencere kapanmadan önce ne yapılsın?".to_owned(),
                },
                buttons: vec![
                    (Decision::Save, "Kaydet"),
                    (Decision::Discard, "Kaydetme"),
                    (Decision::Cancel, "Vazgeç"),
                ],
            },
        }
    }
}

struct TerminalPane {
    id: SessionId,
    view: Entity<TerminalView>,
}

/// The application's main view.
pub struct Workbench {
    files: Entity<FileTreeView>,
    editor: Entity<EditorView>,
    inspector: Entity<InspectorView>,
    inspector_visible: bool,
    project: Option<Arc<ProjectRoot>>,
    host: SharedHost,
    sessions: Arc<SessionRegistry>,
    gates: Arc<GateRegistry>,
    terminals: Vec<TerminalPane>,
    selected: usize,
    dock_visible: bool,
    layout: LayoutPreset,
    sizes: Entity<ResizableState>,
    pending: Option<PendingAction>,
    palette: Option<Entity<PaletteView>>,
    palette_subscription: Option<Subscription>,
    /// An action to run once the save started by `Decision::Save` has succeeded.
    after_save: Option<PendingAction>,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    file_search_task: Option<Task<()>>,
    appearance: Appearance,
    close_confirmed: bool,
    last_command: Option<CommandId>,
    frames: u64,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workbench {
    /// A workbench with no project open. `programs` says which shell and `raios` the terminals
    /// run.
    pub fn new(
        programs: ProgramPaths,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let gates = Arc::new(GateRegistry::new());
        let wake_gates = Arc::clone(&gates);
        let pty = Arc::new(PtyTerminalHost::new(
            programs,
            Arc::new(move |id: &SessionId| wake_gates.wake(id)),
        ));
        let host = SharedHost(pty);
        let sessions = Arc::new(SessionRegistry::new(Box::new(host.clone())));
        let files = cx.new(FileTreeView::new);
        let editor = cx.new(|cx| EditorView::new(appearance.tone, window, cx));
        let inspector = cx.new(|_| InspectorView::new());
        let sizes = cx.new(|_| ResizableState::default());
        let subscriptions = vec![
            cx.subscribe_in(
                &files,
                window,
                |this, _files, event: &FileTreeEvent, window, cx| {
                    let FileTreeEvent::OpenFile(path) = event;
                    this.request_open_file(path.clone(), window, cx);
                },
            ),
            cx.subscribe_in(
                &editor,
                window,
                |this, _editor, event: &EditorEvent, window, cx| {
                    this.on_editor_event(event, window, cx);
                },
            ),
        ];
        Self {
            files,
            editor,
            inspector,
            inspector_visible: false,
            project: None,
            host,
            sessions,
            gates,
            terminals: Vec::new(),
            selected: 0,
            dock_visible: true,
            layout: LayoutPreset::Balanced,
            sizes,
            pending: None,
            palette: None,
            palette_subscription: None,
            after_save: None,
            toast: None,
            toast_task: None,
            file_search_task: None,
            appearance,
            close_confirmed: false,
            last_command: None,
            frames: 0,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    // ----- observable state ------------------------------------------------------------

    /// What `available` needs to know.
    pub fn state(&self, cx: &gpui::App) -> WorkbenchState {
        let editor = self.editor.read(cx);
        WorkbenchState {
            project_open: self.project.is_some(),
            file_open: editor.path().is_some(),
            dirty: editor.is_dirty(),
            terminals: u8::try_from(self.terminals.len()).unwrap_or(u8::MAX),
            dock_visible: self.dock_visible,
            control_plane_features: false,
            daemon_reachable: false,
        }
    }

    /// Connects the inspector (and later the guarded commands) to the daemon.
    pub fn set_control_plane(
        &mut self,
        plane: Option<Arc<dyn ControlPlane>>,
        cx: &mut Context<Self>,
    ) {
        self.inspector
            .update(cx, |inspector, cx| inspector.set_control_plane(plane, cx));
    }

    /// True while the inspector is shown above the terminals.
    pub fn inspector_visible(&self) -> bool {
        self.inspector_visible
    }

    /// The inspector panel.
    pub fn inspector(&self) -> &Entity<InspectorView> {
        &self.inspector
    }

    /// The message shown at the bottom of the window.
    pub fn toast(&self) -> Option<String> {
        self.toast.as_ref().map(ToString::to_string)
    }

    /// The action waiting for a decision.
    pub fn pending(&self) -> Option<&PendingAction> {
        self.pending.as_ref()
    }

    /// The dialog for the pending action, if one is waiting.
    pub fn dialog(&self) -> Option<Dialog> {
        self.pending.as_ref().map(PendingAction::dialog)
    }

    /// The open command palette.
    pub fn palette(&self) -> Option<&Entity<PaletteView>> {
        self.palette.as_ref()
    }

    /// The current layout preset.
    pub fn layout(&self) -> LayoutPreset {
        self.layout
    }

    /// True while the terminal dock is shown.
    pub fn dock_visible(&self) -> bool {
        self.dock_visible
    }

    /// Number of terminal panes.
    pub fn terminal_count(&self) -> usize {
        self.terminals.len()
    }

    /// Index of the selected terminal pane.
    pub fn selected_terminal(&self) -> usize {
        self.selected
    }

    /// The command that ran last (including ones refused as unavailable).
    pub fn last_command(&self) -> Option<CommandId> {
        self.last_command
    }

    /// True once the window may close.
    pub fn close_confirmed(&self) -> bool {
        self.close_confirmed
    }

    /// The handle the workbench takes keyboard focus with.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// How many times the workbench has been rendered (the cold-start gate waits for the first).
    pub fn frames_rendered(&self) -> u64 {
        self.frames
    }

    /// The current appearance.
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// The editor entity.
    pub fn editor(&self) -> &Entity<EditorView> {
        &self.editor
    }

    /// The file tree entity.
    pub fn files(&self) -> &Entity<FileTreeView> {
        &self.files
    }

    /// Sessions that are still running, according to the registry.
    pub fn running_sessions(&self) -> usize {
        self.sessions
            .list()
            .iter()
            .filter(|session| session.running)
            .count()
    }

    fn say(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_secs(5));
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |view, cx| {
                view.toast = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    // ----- commands --------------------------------------------------------------------

    /// Runs a command: the one path buttons, shortcuts and the palette all take.
    pub fn run(&mut self, id: CommandId, window: &mut Window, cx: &mut Context<Self>) {
        self.last_command = Some(id);
        self.toast = None;
        self.toast_task = None;
        let state = available(id, &self.state(cx));
        if !state.enabled {
            if let Some(reason) = state.reason {
                self.say(reason, cx);
            }
            return;
        }
        match id {
            CommandId::Project => self.pick_project(window, cx),
            CommandId::Save => self.save(cx),
            CommandId::Diff => {
                if let Some(project) = self.project.clone() {
                    self.editor
                        .update(cx, |editor, cx| editor.show_diff(project, window, cx));
                }
            }
            CommandId::NewTerminal => self.new_terminal(SessionKind::Shell, window, cx),
            CommandId::ToggleTerminal => {
                self.dock_visible = !self.dock_visible;
                cx.notify();
            }
            CommandId::Inspector => {
                self.inspector_visible = !self.inspector_visible;
                if self.inspector_visible {
                    self.dock_visible = true;
                }
                cx.notify();
            }
            CommandId::FocusEditor => self
                .editor
                .update(cx, |editor, cx| editor.focus(window, cx)),
            CommandId::FocusFiles => self.files.update(cx, |files, cx| files.focus(window, cx)),
            CommandId::NextTerminal => self.select_terminal_offset(1, window, cx),
            CommandId::PreviousTerminal => self.select_terminal_offset(-1, window, cx),
            CommandId::FocusTerminal(number) => {
                self.select_terminal(usize::from(number).saturating_sub(1), window, cx);
            }
            CommandId::ClearTerminal => self.with_selected_terminal(cx, |view, cx| {
                // Ctrl+L: the shell clears and redraws; the engine has no clear of its own.
                view.send_bytes(&[0x0c], cx);
            }),
            CommandId::CopyTerminal => {
                self.with_selected_terminal(cx, |view, cx| view.copy_selection(cx));
            }
            CommandId::PasteTerminal => {
                self.with_selected_terminal(cx, |view, cx| view.paste_from_clipboard(cx));
            }
            CommandId::StopTerminal => {
                if let Some(pane) = self.terminals.get(self.selected) {
                    self.pending = Some(PendingAction::StopTerminal(pane.id.clone()));
                    cx.notify();
                }
            }
            CommandId::Refresh => self.files.update(cx, |files, cx| files.refresh(cx)),
            CommandId::Settings => self.toggle_tone(cx),
            CommandId::Balanced => self.apply_layout(LayoutPreset::Balanced, window, cx),
            CommandId::EditorLayout => self.apply_layout(LayoutPreset::Editor, window, cx),
            CommandId::AgentsLayout => self.apply_layout(LayoutPreset::Agents, window, cx),
            CommandId::Commands | CommandId::Shortcuts => self.toggle_palette(window, cx),
            CommandId::Files => self.open_file_palette(window, cx),
            CommandId::Memory | CommandId::Agent | CommandId::Handoff => {
                self.say("Bu sürümde henüz yok", cx);
            }
        }
    }

    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        let state = self.state(cx);
        let palette = cx.new(|cx| PaletteView::new(state, window, cx));
        self.attach_palette(palette, window, cx);
    }

    fn attach_palette(
        &mut self,
        palette: Entity<PaletteView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_subscription = Some(cx.subscribe_in(
            &palette,
            window,
            |this, _palette, event: &PaletteEvent, window, cx| match event.clone() {
                PaletteEvent::Close => this.close_palette(window, cx),
                PaletteEvent::OpenFile(path) => {
                    this.close_palette(window, cx);
                    this.request_open_file(path, window, cx);
                }
                PaletteEvent::Run(id) => {
                    this.close_palette(window, cx);
                    this.run(id, window, cx);
                }
            },
        ));
        self.palette = Some(palette);
        cx.notify();
    }

    fn open_file_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .palette
            .as_ref()
            .is_some_and(|palette| palette.read(cx).is_file_picker())
        {
            self.close_palette(window, cx);
            return;
        }
        self.close_palette(window, cx);
        let Some(project) = self.project.clone() else {
            return;
        };
        let project_id = project.info().id.clone();
        let palette = cx.new(|cx| PaletteView::for_files(window, cx));
        self.attach_palette(palette.clone(), window, cx);
        self.file_search_task = Some(cx.spawn_in(window, async move |this, cx| {
            let listing = cx
                .background_executor()
                .spawn(async move {
                    rwk_core::workbench::quick_open::list_project_files(&project, 10_000)
                })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                if view.project.as_ref().map(|p| &p.info().id) != Some(&project_id)
                    || view.palette.as_ref().map(Entity::entity_id) != Some(palette.entity_id())
                {
                    return;
                }
                match listing {
                    Ok(files) => palette.update(cx, |palette, cx| palette.set_files(files, cx)),
                    Err(_) => {
                        view.close_palette(window, cx);
                        view.say("Dosyalar listelenemedi; daha dar bir proje açın", cx);
                    }
                }
            });
        }));
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_search_task = None;
        self.palette = None;
        self.palette_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn pick_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Proje klasörü seç".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |workbench, window, cx| {
                workbench.open_project(path, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self.project.clone() {
            self.editor
                .update(cx, |editor, cx| editor.save(project, cx));
        }
    }

    fn toggle_tone(&mut self, cx: &mut Context<Self>) {
        let tone = match self.appearance.tone {
            Tone::Dark => Tone::Light,
            Tone::Light => Tone::Dark,
        };
        self.appearance.tone = tone;
        cx.emit(WorkbenchEvent::AppearanceChanged(self.appearance));
        self.editor
            .update(cx, |editor, cx| editor.set_tone(tone, cx));
        let terminal = terminal_appearance(tone);
        for pane in &self.terminals {
            pane.view
                .update(cx, |view, cx| view.set_appearance(terminal, cx));
        }
        cx.notify();
    }

    fn apply_layout(&mut self, preset: LayoutPreset, window: &mut Window, cx: &mut Context<Self>) {
        self.layout = preset;
        let total = f32::from(window.viewport_size().width);
        let [files, _editor, dock] = preset.widths(total);
        self.sizes.update(cx, |sizes, cx| {
            sizes.resize_panel(0, px(files), window, cx);
            sizes.resize_panel(2, px(dock), window, cx);
        });
        cx.notify();
    }

    // ----- projects and files ----------------------------------------------------------

    /// Opens a folder as the project. Unsaved edits are asked about first.
    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.read(cx).request_leave() == SwitchDecision::NeedsDecision {
            self.pending = Some(PendingAction::OpenProject(root));
            cx.notify();
            return;
        }
        // Opening a project stops the running sessions (agents included): ask first.
        if self.running_sessions() > 0 {
            self.pending = Some(PendingAction::OpenProjectStoppingSessions(root));
            cx.notify();
            return;
        }
        self.perform(PendingAction::OpenProject(root), window, cx);
    }

    /// Opens a file of the project. Unsaved edits in the open file are asked about first.
    pub fn request_open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let editor = self.editor.read(cx);
        let decision = editor.request_open(&project.info().id, &path);
        // The session treats reopening the open file as free, but doing it would reload the
        // file from disk: harmless when clean (nothing to do), a loss when dirty (ask).
        let same_file = editor.path() == Some(path.as_str());
        if same_file && decision == SwitchDecision::Allowed {
            if editor.is_dirty() {
                self.pending = Some(PendingAction::OpenFile(path));
                cx.notify();
            }
            return;
        }
        if decision == SwitchDecision::NeedsDecision {
            self.pending = Some(PendingAction::OpenFile(path));
            cx.notify();
            return;
        }
        self.perform(PendingAction::OpenFile(path), window, cx);
    }

    /// Asks whether the window may close. Returns true when it may (sessions are stopped);
    /// false while a decision about unsaved edits is pending.
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.editor.read(cx).request_leave() == SwitchDecision::NeedsDecision {
            self.pending = Some(PendingAction::CloseWindow);
            cx.notify();
            return false;
        }
        self.perform(PendingAction::CloseWindow, window, cx);
        true
    }

    /// Applies the user's answer to the pending action.
    pub fn resolve(&mut self, decision: Decision, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.pending.take() else {
            return;
        };
        match (decision, &action) {
            (Decision::Cancel, _) => {}
            (
                Decision::Save | Decision::Discard,
                PendingAction::StopTerminal(_) | PendingAction::OpenProjectStoppingSessions(_),
            ) => {}
            (Decision::Save, _) => {
                self.after_save = Some(action);
                self.save(cx);
            }
            (Decision::Discard, _) => {
                self.editor
                    .update(cx, |editor, cx| editor.discard(window, cx));
                self.perform(action, window, cx);
            }
            (
                Decision::Confirm,
                PendingAction::StopTerminal(_) | PendingAction::OpenProjectStoppingSessions(_),
            ) => self.perform(action, window, cx),
            (Decision::Confirm, _) => {}
        }
        if self.close_confirmed {
            // A close that waited for a decision is finished here; a close the window itself
            // asked for returns `true` from `request_close` instead.
            window.remove_window();
        }
        cx.notify();
    }

    fn on_editor_event(
        &mut self,
        event: &EditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Saved(_) => {
                if let Some(action) = self.after_save.take() {
                    self.perform(action, window, cx);
                }
            }
            EditorEvent::Conflict | EditorEvent::SaveFailed(_) => {
                // The edits are still there: the pending action must not proceed.
                if self.after_save.take().is_some() {
                    self.say("Kaydedilemedi; işlem iptal edildi", cx);
                }
            }
            EditorEvent::Edited | EditorEvent::Opened(_) => {}
        }
    }

    fn perform(&mut self, action: PendingAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            PendingAction::OpenFile(path) => {
                if let Some(project) = self.project.clone() {
                    self.editor
                        .update(cx, |editor, cx| editor.open_path(project, path, window, cx));
                }
            }
            PendingAction::OpenProjectStoppingSessions(root) => {
                self.perform(PendingAction::OpenProject(root), window, cx);
            }
            PendingAction::OpenProject(root) => match ProjectRoot::open(root) {
                Ok(project) => {
                    self.close_palette(window, cx);
                    self.stop_terminals(false);
                    self.editor
                        .update(cx, |editor, cx| editor.discard(window, cx));
                    let project = Arc::new(project);
                    self.files
                        .update(cx, |files, cx| files.set_project(Arc::clone(&project), cx));
                    let root = project.info().root.clone();
                    self.project = Some(project);
                    self.inspector
                        .update(cx, |inspector, cx| inspector.set_project(Some(root), cx));
                    cx.notify();
                }
                Err(error) => self.say(error.message, cx),
            },
            PendingAction::CloseWindow => {
                self.stop_terminals(true);
                self.close_confirmed = true;
            }
            PendingAction::StopTerminal(id) => {
                if let Err(error) = self.sessions.stop(&id) {
                    self.say(error.message, cx);
                }
            }
        }
    }

    // ----- terminals -------------------------------------------------------------------

    /// Stops every owned session and drops the panes. `closing` ends the workspace for good:
    /// the registry then refuses new sessions. A project switch stops the sessions one by one
    /// instead, so the next project can start its own.
    fn stop_terminals(&mut self, closing: bool) {
        if closing {
            self.sessions.stop_all();
        } else {
            for session in self.sessions.list() {
                let _ = self.sessions.stop(&session.id);
            }
        }
        for pane in self.terminals.drain(..) {
            self.gates.detach(&pane.id);
            self.host.0.remove(&pane.id);
        }
        self.selected = 0;
    }

    /// Drops panes whose session has ended, to make room for a new one.
    fn prune_dead_panes(&mut self) {
        let running: Vec<SessionId> = self
            .sessions
            .list()
            .into_iter()
            .filter(|session| session.running)
            .map(|session| session.id)
            .collect();
        let gates = Arc::clone(&self.gates);
        let host = self.host.0.clone();
        self.terminals.retain(|pane| {
            let alive = running.contains(&pane.id);
            if !alive {
                gates.detach(&pane.id);
                host.remove(&pane.id);
            }
            alive
        });
        self.selected = self.selected.min(self.terminals.len().saturating_sub(1));
    }

    fn new_terminal(&mut self, kind: SessionKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        self.prune_dead_panes();
        let info = match self.sessions.start(&project, kind, None, NEW_TERMINAL_SIZE) {
            Ok(info) => info,
            Err(error) => {
                self.say(error.message, cx);
                return;
            }
        };
        let Some(engine) = self.host.0.engine(&info.id) else {
            self.say("Terminal oturumu başlatılamadı", cx);
            return;
        };
        let (gate, receiver) = self.gates.attach(&info.id);
        let host: Arc<dyn TerminalHost> = Arc::new(self.host.clone());
        let appearance = terminal_appearance(self.appearance.tone);
        let id = info.id.clone();
        let view = cx.new(|cx| TerminalView::new(engine, host, id, appearance, gate, receiver, cx));
        self.terminals.push(TerminalPane { id: info.id, view });
        self.dock_visible = true;
        let last = self.terminals.len() - 1;
        self.select_terminal(last, window, cx);
    }

    fn select_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.terminals.get(index) else {
            return;
        };
        self.selected = index;
        let handle = pane.view.read(cx).focus_handle().clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    fn select_terminal_offset(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.terminals.len() as isize;
        if count == 0 {
            return;
        }
        let next = (self.selected as isize + delta).rem_euclid(count) as usize;
        self.select_terminal(next, window, cx);
    }

    fn with_selected_terminal(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut TerminalView, &mut Context<TerminalView>),
    ) {
        if let Some(pane) = self.terminals.get(self.selected) {
            pane.view.update(cx, action);
        }
    }

    // ----- keyboard --------------------------------------------------------------------

    fn keyboard_context(&self, window: &Window, cx: &gpui::App) -> KeyboardContext {
        KeyboardContext {
            terminal: self.terminals.iter().any(|pane| {
                pane.view
                    .read(cx)
                    .focus_handle()
                    .contains_focused(window, cx)
            }),
            dialog: self.pending.is_some(),
            input: self.palette.is_some(),
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.dialog() {
            // A dialog owns the keyboard: Enter takes the default button, Escape cancels.
            let decision = match event.keystroke.key.as_str() {
                "enter" => dialog.buttons.first().map(|(decision, _)| *decision),
                "escape" => Some(Decision::Cancel),
                _ => None,
            };
            if let Some(decision) = decision {
                self.resolve(decision, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let stroke = Keystroke {
            key: event.keystroke.key.clone(),
            ctrl: modifiers.control,
            platform: modifiers.platform,
            alt: modifiers.alt,
            shift: modifiers.shift,
            composing: false,
            repeat: event.is_held,
            alt_graph: false,
        };
        let context = self.keyboard_context(window, cx);
        if let Some(command) = resolve_shortcut(&stroke, context, false) {
            self.run(command, window, cx);
            cx.stop_propagation();
        }
    }
}

fn terminal_appearance(tone: Tone) -> TerminalAppearance {
    match tone {
        Tone::Dark => TerminalAppearance::Dark,
        Tone::Light => TerminalAppearance::Light,
    }
}

impl Render for Workbench {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let dock = div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.inspector_visible.then(|| {
                div()
                    .h(px(INSPECTOR_HEIGHT))
                    .flex_none()
                    .border_b_1()
                    .border_color(gpui::rgb(0x333333))
                    .child(self.inspector.clone())
            }))
            .children(self.terminals.iter().enumerate().map(|(index, pane)| {
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .when_selected(index == self.selected)
                    .child(pane.view.clone())
            }));
        let main = h_resizable("workbench")
            .with_state(&self.sizes)
            .child(resizable_panel().size(px(260.0)).child(self.files.clone()))
            .child(resizable_panel().child(self.editor.clone()))
            .child(
                resizable_panel()
                    .size(px(380.0))
                    .visible(self.dock_visible)
                    .child(dock),
            );
        let toast = self.toast.clone().unwrap_or_default();
        let dialog = self.dialog().map(|dialog| {
            let buttons = dialog.buttons.iter().map(|(decision, label)| {
                let decision = *decision;
                div()
                    .id(SharedString::from(format!("dialog-{label}")))
                    .px_3()
                    .py_1()
                    .border_1()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.resolve(decision, window, cx);
                    }))
                    .child(*label)
            });
            div()
                .absolute()
                .top_16()
                .left_0()
                .right_0()
                .mx_auto()
                .w(px(520.0))
                .p_3()
                .bg(gpui::rgb(0x252526))
                .text_color(gpui::rgb(0xe5e5e5))
                .child(div().child(dialog.title))
                .child(div().py_2().child(dialog.message))
                .child(div().flex().flex_row().gap_2().children(buttons))
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            // GPUI's default text colour is black on a black window: every pane that does not
            // paint its own colours (file tree, toast line) would be invisible.
            .bg(surface_background(self.appearance.tone))
            .text_color(surface_foreground(self.appearance.tone))
            // Capture phase: a shortcut is resolved before the focused terminal sees the key.
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(div().flex_1().min_h_0().child(main))
            .child(div().px_2().child(toast))
            .children(self.palette.clone())
            .children(dialog)
    }
}

/// Height of the inspector above the terminals.
const INSPECTOR_HEIGHT: f32 = 260.0;

/// Window background of a tone, the same surface the editor and terminal paint on.
fn surface_background(tone: Tone) -> gpui::Rgba {
    gpui::rgb(match tone {
        Tone::Dark => 0x1e1e1e,
        Tone::Light => 0xffffff,
    })
}

/// Default text colour of a tone.
fn surface_foreground(tone: Tone) -> gpui::Rgba {
    gpui::rgb(match tone {
        Tone::Dark => 0xd4d4d4,
        Tone::Light => 0x1f2328,
    })
}

/// Styling helper kept next to its only user.
trait SelectedPane: Styled + Sized {
    fn when_selected(self, selected: bool) -> Self {
        if selected {
            self.border_color(gpui::rgb(0x3794ff))
        } else {
            self.border_color(gpui::rgb(0x333333))
        }
    }
}

impl SelectedPane for gpui::Div {}
