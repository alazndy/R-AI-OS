//! The workbench: the window's three panes, the terminal dock and the single place where
//! commands take effect.

pub mod control;
pub mod host;
pub mod layout;
pub mod memory;
pub mod palette;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window,
};
use gpui_base::{h_resizable, resizable_panel, ResizableState};
use rwk_core::consent::{ConsentRegistry, ConsentRequest};
use rwk_core::control::ControlPlane;
use rwk_core::guarded::Guarded;
use rwk_core::handoff_flow::TrackedHandoff;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, SessionRegistry, TerminalHost, TerminalSize};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};
use rwk_core::workbench::{
    available, resolve_shortcut, Appearance, CommandId, KeyboardContext, Keystroke, Skin,
    SwitchDecision, Theme, Tone, WorkbenchState,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbenchEvent {
    /// The user changed skin or tone; the application stores it.
    AppearanceChanged(Appearance),
    /// The daemon confirmed this project is registered (newly or before); the application
    /// remembers it so the registration is not offered again at every start.
    ProjectRegistered(PathBuf),
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
    /// Choose which agent to start.
    ChooseAgent,
    /// A command whose policy is `confirm` waits for an explicit yes; the text is derived from
    /// the payload that will be sent.
    Consent(ConsentRequest, Box<Guarded>),
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
    /// Start this agent.
    Agent(SessionKind),
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
            PendingAction::ChooseAgent => Dialog {
                title: "Ajan başlat",
                message: "Hangi ajan bu projede başlatılsın? Daemon politikası ve denetim kaydı uygulanır.".to_owned(),
                buttons: vec![
                    (Decision::Agent(SessionKind::Claude), "Claude"),
                    (Decision::Agent(SessionKind::Codex), "Codex"),
                    (Decision::Agent(SessionKind::Opencode), "OpenCode"),
                    (Decision::Agent(SessionKind::Agy), "Antigravity"),
                    (Decision::Cancel, "Vazgeç"),
                ],
            },
            PendingAction::Consent(request, _) => Dialog {
                title: "Onay gerekiyor",
                message: request.summary.clone(),
                buttons: vec![(Decision::Confirm, "Onayla"), (Decision::Cancel, "Vazgeç")],
            },
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
    kind: SessionKind,
    id: SessionId,
    view: Entity<TerminalView>,
}

/// The application's main view.
pub struct Workbench {
    files: Entity<FileTreeView>,
    editor: Entity<EditorView>,
    inspector: Entity<InspectorView>,
    inspector_visible: bool,
    plane: Option<Arc<dyn ControlPlane>>,
    memory_source: Option<Arc<dyn rwk_core::memory::MemorySource>>,
    memory: Option<Entity<crate::memory::MemoryView>>,
    memory_subscription: Option<Subscription>,
    consent: Arc<ConsentRegistry>,
    consent_wait: Option<std::sync::mpsc::Receiver<rwk_core::consent::ConsentDecision>>,
    composer: Option<Entity<crate::handoff::HandoffComposer>>,
    composer_subscription: Option<Subscription>,
    handoffs: Vec<TrackedHandoff>,
    receipt_task: Option<Task<()>>,
    receiver_for: Option<String>,
    /// Projects already known to be registered with the daemon.
    known_projects: std::collections::HashSet<PathBuf>,
    /// A command that asked for a project first and runs once one is open.
    after_pick: Option<CommandId>,
    receiver_session: Option<(String, SessionId)>,
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
        let theme = Theme::of(appearance);
        let files = cx.new(FileTreeView::new);
        files.update(cx, |files, cx| files.set_theme(theme, cx));
        let editor = cx.new(|cx| EditorView::themed(theme, window, cx));
        let inspector = cx.new(|_| InspectorView::new());
        inspector.update(cx, |inspector, cx| inspector.set_theme(theme, cx));
        let sizes = cx.new(|_| ResizableState::default());
        let inspector_events = cx.subscribe_in(
            &inspector,
            window,
            |this, _inspector, event: &crate::inspector::InspectorEvent, window, cx| {
                let crate::inspector::InspectorEvent::StartReceiver(id) = event;
                this.start_receiver(id.clone(), window, cx);
            },
        );
        let subscriptions = vec![
            inspector_events,
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
            plane: None,
            memory_source: None,
            memory: None,
            memory_subscription: None,
            consent: Arc::new(ConsentRegistry::new()),
            consent_wait: None,
            composer: None,
            composer_subscription: None,
            handoffs: Vec::new(),
            receipt_task: None,
            receiver_for: None,
            known_projects: std::collections::HashSet::new(),
            after_pick: None,
            receiver_session: None,
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
            control_plane_features: self.plane.is_some(),
            daemon_reachable: self.plane.is_some()
                && !matches!(
                    self.inspector.read(cx).model().banner,
                    rwk_core::inspector::Banner::Unreachable(_)
                ),
        }
    }

    /// True while the inspector is shown above the terminals.
    pub fn inspector_visible(&self) -> bool {
        self.inspector_visible
    }

    /// The inspector panel.
    pub fn inspector(&self) -> &Entity<InspectorView> {
        &self.inspector
    }

    /// The name of the open project, or an empty string.
    pub fn project_name(&self) -> String {
        self.project
            .as_ref()
            .map(|project| project.info().name.clone())
            .unwrap_or_default()
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

    /// Width of the dock in pixels, as the divider currently has it.
    pub fn dock_width(&self, cx: &gpui::App) -> f32 {
        f32::from(
            self.sizes
                .read(cx)
                .sizes()
                .get(2)
                .copied()
                .unwrap_or(px(0.0)),
        )
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
        // Starting a session without a project offers the folder picker first; cancelling it
        // starts nothing.
        if self.project.is_none() && matches!(id, CommandId::NewTerminal | CommandId::Agent) {
            self.after_pick = Some(id);
            self.pick_project(window, cx);
            return;
        }
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
            CommandId::DiffSplit => {
                let split = !self.editor.read(cx).diff_split();
                self.editor
                    .update(cx, |editor, cx| editor.set_diff_split(split, window, cx));
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
            CommandId::Skin => self.toggle_skin(cx),
            CommandId::GrowDock => self.nudge_dock(DOCK_STEP_PX, window, cx),
            CommandId::ShrinkDock => self.nudge_dock(-DOCK_STEP_PX, window, cx),
            CommandId::Balanced => self.apply_layout(LayoutPreset::Balanced, window, cx),
            CommandId::EditorLayout => self.apply_layout(LayoutPreset::Editor, window, cx),
            CommandId::AgentsLayout => self.apply_layout(LayoutPreset::Agents, window, cx),
            CommandId::Commands | CommandId::Shortcuts => self.toggle_palette(window, cx),
            CommandId::Files => self.open_file_palette(window, cx),
            CommandId::Agent => self.open_agent_chooser(cx),
            CommandId::Handoff => self.open_composer(window, cx),
            CommandId::RegisterProject => self.register_project(window, cx),
            CommandId::Memory => self.open_memory(window, cx),
        }
    }

    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        let state = self.state(cx);
        let theme = self.theme();
        let palette = cx.new(|cx| PaletteView::new(state, window, cx));
        palette.update(cx, |palette, cx| palette.set_theme(theme, cx));
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
        let theme = self.theme();
        let palette = cx.new(|cx| PaletteView::for_files(window, cx));
        palette.update(cx, |palette, cx| palette.set_theme(theme, cx));
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
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = picked else {
                this.update(cx, |workbench, _| workbench.after_pick = None)
                    .ok();
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
        self.appearance.tone = match self.appearance.tone {
            Tone::Dark => Tone::Light,
            Tone::Light => Tone::Dark,
        };
        self.apply_appearance(cx);
    }

    fn toggle_skin(&mut self, cx: &mut Context<Self>) {
        self.appearance.skin = match self.appearance.skin {
            Skin::Standard => Skin::Lcars,
            Skin::Lcars => Skin::Standard,
        };
        self.apply_appearance(cx);
    }

    /// The colour tokens of the current appearance.
    pub fn theme(&self) -> Theme {
        Theme::of(self.appearance)
    }

    /// Tells every pane about a changed skin or tone, without recreating editors or sessions.
    fn apply_appearance(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme();
        cx.emit(WorkbenchEvent::AppearanceChanged(self.appearance));
        self.files
            .update(cx, |files, cx| files.set_theme(theme, cx));
        self.inspector
            .update(cx, |inspector, cx| inspector.set_theme(theme, cx));
        self.editor
            .update(cx, |editor, cx| editor.set_theme(theme, cx));
        let terminal = terminal_appearance(theme);
        for pane in &self.terminals {
            pane.view
                .update(cx, |view, cx| view.set_appearance(terminal, cx));
        }
        if let Some(palette) = &self.palette {
            palette.update(cx, |palette, cx| palette.set_theme(theme, cx));
        }
        if let Some(composer) = &self.composer {
            composer.update(cx, |composer, cx| composer.set_theme(theme, cx));
        }
        if let Some(memory) = &self.memory {
            memory.update(cx, |memory, cx| memory.set_theme(theme, cx));
        }
        cx.notify();
    }

    /// Moves the dock divider by `delta` pixels (positive widens the dock). The same minimums as
    /// the layout presets apply.
    fn nudge_dock(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let total = f32::from(window.viewport_size().width);
        let current = f32::from(
            self.sizes
                .read(cx)
                .sizes()
                .get(2)
                .copied()
                .unwrap_or(px(0.0)),
        );
        let files = f32::from(
            self.sizes
                .read(cx)
                .sizes()
                .first()
                .copied()
                .unwrap_or(px(0.0)),
        );
        let wanted = (current + delta).clamp(
            layout::MIN_SIDE_PANE_PX,
            (total - files - layout::MIN_EDITOR_PX).max(layout::MIN_SIDE_PANE_PX),
        );
        self.sizes.update(cx, |sizes, cx| {
            sizes.resize_panel(2, px(wanted), window, cx)
        });
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
            (Decision::Cancel, PendingAction::Consent(request, op)) => {
                self.answer_consent(request, (**op).clone(), false, window, cx);
            }
            (Decision::Cancel, _) => {}
            (Decision::Agent(kind), PendingAction::ChooseAgent) => {
                self.launch_agent(kind, window, cx);
            }
            (Decision::Agent(_), _) => {}
            (Decision::Confirm, PendingAction::Consent(request, op)) => {
                self.answer_consent(request, (**op).clone(), true, window, cx);
            }
            (
                Decision::Save | Decision::Discard,
                PendingAction::StopTerminal(_)
                | PendingAction::OpenProjectStoppingSessions(_)
                | PendingAction::ChooseAgent
                | PendingAction::Consent(..),
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
                    self.register_if_unknown(window, cx);
                    if let Some(command) = self.after_pick.take() {
                        self.run(command, window, cx);
                    }
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
            // These two are answered through `resolve`, never performed directly.
            PendingAction::ChooseAgent | PendingAction::Consent(..) => {}
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
        let appearance = terminal_appearance(self.theme());
        let id = info.id.clone();
        let view = cx.new(|cx| TerminalView::new(engine, host, id, appearance, gate, receiver, cx));
        self.terminals.push(TerminalPane {
            kind,
            id: info.id,
            view,
        });
        self.dock_visible = true;
        if self.terminals.len() >= 3 && self.layout != LayoutPreset::Agents {
            self.apply_layout(LayoutPreset::Agents, window, cx);
        }
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
                // A consent dialog is never answered by a stray Enter: approval is a click.
                "enter" if matches!(self.pending, Some(PendingAction::Consent(..))) => None,
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

fn terminal_appearance(theme: Theme) -> TerminalAppearance {
    TerminalAppearance::Themed {
        dark: theme.dark,
        foreground: theme.text,
        background: theme.editor,
        selection: theme.selected,
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        let theme = self.theme();
        let dock = div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.inspector_visible.then(|| {
                div()
                    .h(px(INSPECTOR_HEIGHT))
                    .flex_none()
                    .border_b_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(self.inspector.clone())
            }))
            .children(self.terminals.iter().enumerate().map(|(index, pane)| {
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .when_selected(theme, index == self.selected)
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
                    .border_color(gpui::rgb(theme.accent_ink))
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
                .bg(gpui::rgb(theme.panel))
                .border_1()
                .border_color(gpui::rgb(if theme.lcars {
                    theme.accent
                } else {
                    theme.line
                }))
                .text_color(gpui::rgb(theme.text))
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
            .bg(gpui::rgb(theme.bg))
            .text_color(gpui::rgb(theme.text))
            // Capture phase: a shortcut is resolved before the focused terminal sees the key.
            .capture_key_down(cx.listener(Self::on_key_down))
            .children(
                theme
                    .lcars
                    .then(|| lcars_header(theme, self.project_name(), window)),
            )
            .child(div().flex_1().min_h_0().child(main))
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(gpui::rgb(if theme.lcars {
                        theme.accent_ink
                    } else {
                        theme.muted
                    }))
                    .children(
                        theme
                            .lcars
                            .then(|| div().h(px(3.0)).mb_1().bg(gpui::rgb(theme.blue))),
                    )
                    .child(toast),
            )
            .children(self.palette.clone())
            .children(self.composer.clone())
            .children(self.memory.clone())
            .children(dialog)
    }
}

/// How far one dock-resize command moves the divider.
const DOCK_STEP_PX: f32 = 40.0;

/// Height of the inspector above the terminals.
const INSPECTOR_HEIGHT: f32 = 260.0;

/// The GT-LCARS header: the brand in display lettering and the command rail, three bars in
/// blue, blue and orange as in the KDE theme.
fn lcars_header(theme: Theme, project: String, window: &Window) -> impl IntoElement {
    let display = crate::fonts::display_family(window);
    div()
        .flex_none()
        .px_4()
        .pt_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_3()
                .font_family(display)
                .child(
                    div()
                        .text_size(px(30.0))
                        .text_color(gpui::rgb(theme.blue))
                        .child("RAIOS"),
                )
                .child(
                    div()
                        .text_size(px(16.0))
                        .text_color(gpui::rgb(theme.ice))
                        .child(SharedString::from(project.to_uppercase())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1p5()
                .h(px(9.0))
                .mt_1()
                .child(
                    div()
                        .w(px(198.0))
                        .bg(gpui::rgb(theme.blue))
                        .rounded_tl(px(12.0)),
                )
                .child(div().flex_1().bg(gpui::rgb(theme.blue)))
                .child(
                    div()
                        .w(px(278.0))
                        .bg(gpui::rgb(theme.accent))
                        .rounded_tr(px(12.0)),
                ),
        )
}

/// Styling helper kept next to its only user.
trait SelectedPane: Styled + Sized {
    fn when_selected(self, theme: Theme, selected: bool) -> Self {
        if selected {
            self.border_color(gpui::rgb(theme.accent_ink))
        } else {
            self.border_color(gpui::rgb(theme.line))
        }
    }
}

impl SelectedPane for gpui::Div {}
