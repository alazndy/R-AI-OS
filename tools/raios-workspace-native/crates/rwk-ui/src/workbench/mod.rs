//! The workbench: the window's three panes, the terminal dock and the single place where
//! commands take effect.

pub mod binding;
pub mod control;
pub mod dock;
pub mod editing;
pub mod entries;
pub mod file_replace;
pub mod footer;
pub mod git;
pub mod host;
pub mod layout;
pub mod lsp;
pub mod memory;
pub mod menus;
pub mod offers;
pub mod palette;
pub mod remote;
pub mod runner;
pub mod search;
pub mod settings;
pub mod usage;
pub mod workspace;

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
use rwk_core::sessions::{
    ResumeKey, SessionId, SessionKind, SessionLaunch, SessionName, SessionRegistry, TerminalHost,
    TerminalSize, MAX_SESSIONS,
};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};
use rwk_core::workbench::{
    available, resolve_shortcut, Appearance, CommandId, DockLayout, KeyboardContext, Keystroke,
    SavedWorkspace, SessionOffer, Skin, SwitchDecision, Theme, Tone, WorkbenchState,
    WorkspaceStateSource,
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

/// How long a workspace change waits for the user to stop fiddling before it is handed to the
/// application for storage. A project switch or a close flushes it immediately.
const WORKSPACE_SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// What the workbench tells the application around it.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkbenchEvent {
    /// The user changed skin or tone; the application stores it.
    AppearanceChanged(Appearance),
    /// The daemon confirmed this project is registered (newly or before); the application
    /// remembers it so the registration is not offered again at every start.
    ProjectRegistered(PathBuf),
    /// The open project's workspace changed (tabs, layout, sessions); the application writes
    /// it so the next start reopens the project as it was left.
    WorkspaceChanged(SavedWorkspace),
    /// An editing switch (auto-save, format on save) changed; the application stores it.
    PreferencesChanged {
        /// Save changed files when the window loses focus.
        auto_save: bool,
        /// Format with the language server before saving.
        format_on_save: bool,
        /// What the toolbar shows.
        toolbar: rwk_core::workbench::ToolbarLayout,
    },
}

impl gpui::EventEmitter<WorkbenchEvent> for Workbench {}

/// An action that waits for the user's decision about unsaved edits or a running session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Open this file in the editor; `line` (1-based) is where the cursor should land when a
    /// search hit asked for it.
    OpenFile {
        /// Project-relative path.
        path: String,
        /// 1-based line to put the cursor on.
        line: Option<usize>,
    },
    /// Close this editor tab (unsaved edits are being asked about).
    CloseTab(String),
    /// Open this folder as the project (unsaved edits are being asked about).
    OpenProject(PathBuf),
    /// Open this folder as the project; running sessions will be stopped, which needs a
    /// confirmation of its own when there are no unsaved edits to ask about.
    OpenProjectStoppingSessions(PathBuf),
    /// Close the window.
    CloseWindow,
    /// Stop this terminal session.
    StopTerminal(SessionId),
    /// End this terminal session and remove its pane; `force` skips an agent's grace period.
    CloseTerminal {
        /// The terminal.
        id: SessionId,
        /// End it at once.
        force: bool,
    },
    /// Delete this file, or this folder when it is empty (project-relative path).
    DeleteEntry {
        /// Project-relative path.
        path: String,
        /// The entry is a folder.
        folder: bool,
    },
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
            PendingAction::CloseTerminal { force: false, .. } => Dialog {
                title: "Terminali kapat",
                message: "Terminalde çalışan işlem sonlandırılacak ve panel kaldırılacak. Bir ajan oturumuna kapanması için kısa bir süre tanınır.".to_owned(),
                buttons: vec![(Decision::Confirm, "Kapat"), (Decision::Cancel, "Vazgeç")],
            },
            PendingAction::CloseTerminal { force: true, .. } => Dialog {
                title: "Terminali zorla öldür",
                message: "Terminaldeki tüm işlemler hemen SIGKILL ile öldürülecek. Bir ajan oturumu temiz kapanamaz ve RAIOS'ta açık bir çalıştırma kaydı kalabilir.".to_owned(),
                buttons: vec![(Decision::Confirm, "Öldür"), (Decision::Cancel, "Vazgeç")],
            },
            PendingAction::DeleteEntry { path, folder } => Dialog {
                title: if *folder { "Klasörü sil" } else { "Dosyayı sil" },
                message: format!(
                    "{path} kalıcı olarak silinecek; geri alınamaz.{}",
                    if *folder {
                        " Yalnızca boş klasörler silinir."
                    } else {
                        ""
                    }
                ),
                buttons: vec![(Decision::Confirm, "Sil"), (Decision::Cancel, "Vazgeç")],
            },
            PendingAction::OpenProjectStoppingSessions(_) => Dialog {
                title: "Projeyi değiştir",
                message: "Çalışan terminal ve ajan oturumları durdurulacak.".to_owned(),
                buttons: vec![(Decision::Confirm, "Değiştir"), (Decision::Cancel, "Vazgeç")],
            },
            other => Dialog {
                title: "Kaydedilmemiş değişiklikler",
                message: match other {
                    PendingAction::OpenFile { path, .. } => {
                        format!("Açık dosyada kaydedilmemiş değişiklikler var; {path} açılmadan önce ne yapılsın?")
                    }
                    PendingAction::CloseTab(path) => {
                        format!("{path} dosyasında kaydedilmemiş değişiklikler var; sekme kapanmadan önce ne yapılsın?")
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
    /// The name the user gave the terminal, when there is one.
    name: Option<SessionName>,
    /// The saved conversation this pane continues, when it was started from an offer.
    resume: Option<ResumeKey>,
    /// Where a remote shell (SSH) leads. Such a pane is not part of the saved workspace:
    /// reopening the project never connects anywhere by itself.
    ssh: Option<rwk_core::ssh::SshTarget>,
    /// The task given to this terminal's agent from the task list.
    task: Option<rwk_core::handoff_flow::TaskLink>,
    /// A command from the user's task file runs here; the pane is not saved with the workspace.
    run: bool,
    view: Entity<TerminalView>,
    /// Opens a `path:line` that was Ctrl-clicked in the output.
    _open_location: Subscription,
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
    search_source: Option<Arc<dyn rwk_core::search::SearchSource>>,
    search: Option<Entity<crate::search::SearchView>>,
    search_subscription: Option<Subscription>,
    /// Bumped on every search and on every close; an answer for an older search is dropped.
    search_generation: u64,
    /// How language servers are started; `None` turns them off.
    /// Save changed files when the window loses focus.
    auto_save: bool,
    /// Format with the language server before saving.
    format_on_save: bool,
    /// The checked-out git branch of the project.
    branch: Option<String>,
    /// Bumped on every status read for the tree marks; an older answer is dropped.
    badge_generation: u64,
    /// The state of each language server that was started.
    server_status: Vec<(String, rwk_core::lsp::ServerStatus)>,
    /// Footer items the application added.
    extra_footer: Vec<rwk_core::workbench::FooterItem>,
    task_source: Option<Arc<dyn rwk_core::runner::TaskSource>>,
    /// The open palette lists tasks, not files.
    palette_picks_tasks: bool,
    git_view: Option<Entity<crate::git::GitView>>,
    git_subscription: Option<Subscription>,
    /// Bumped by every git action and on close; an answer for an older one is dropped.
    git_generation: u64,
    lsp_launcher: Option<rwk_core::lsp::Launcher>,
    servers: Option<rwk_core::lsp::LanguageServers>,
    problems: rwk_core::lsp::Problems,
    problems_view: Option<Entity<crate::problems::ProblemsView>>,
    problems_subscription: Option<Subscription>,
    /// A list of places (usages, a file's symbols), shown like the Problems view.
    locations_view: Option<Entity<crate::problems::ProblemsView>>,
    /// The toolbar category whose buttons show.
    toolbar_tab: rwk_core::workbench::ToolbarGroup,
    /// What the toolbar shows and how.
    toolbar_layout: rwk_core::workbench::ToolbarLayout,
    settings_view: Option<Entity<crate::settings::SettingsView>>,
    file_replace: Option<Entity<crate::file_replace::FileReplaceView>>,
    file_replace_subscription: Option<Subscription>,
    settings_subscription: Option<Subscription>,
    locations_subscription: Option<Subscription>,
    lsp_task: Option<Task<()>>,
    lsp_sync_task: Option<Task<()>>,
    /// Where the quota signal is read from (the application sets it).
    usage_source: Option<Arc<dyn rwk_core::usage::UsageSource>>,
    /// The last quota reading, shown at the right of the status bar.
    quota: Option<String>,
    /// The background task that reads the quota signal and refreshes it on a timer.
    usage_task: Option<Task<()>>,
    consent: Arc<ConsentRegistry>,
    consent_wait: Option<std::sync::mpsc::Receiver<rwk_core::consent::ConsentDecision>>,
    composer: Option<Entity<crate::handoff::HandoffComposer>>,
    composer_subscription: Option<Subscription>,
    reject_prompt: Option<Entity<crate::handoff::RejectHandoffPrompt>>,
    reject_prompt_subscription: Option<Subscription>,
    /// How the dock arranges the terminal panes.
    dock: DockLayout<SessionId>,
    /// Where the terminal being dragged would land, while a drag is over a pane.
    drop_hint: Option<(SessionId, rwk_core::workbench::DropZone)>,
    rename_prompt: Option<Entity<crate::rename::RenameTerminalPrompt>>,
    /// The open context menu.
    menu: Option<menus::OpenMenu>,
    /// The open SSH picker.
    ssh_picker: Option<Entity<crate::ssh_picker::SshPicker>>,
    ssh_subscription: Option<Subscription>,
    /// Where `~/.ssh` is read from; `None` means `$HOME`.
    ssh_home: Option<PathBuf>,
    rename_subscription: Option<Subscription>,
    handoffs: Vec<TrackedHandoff>,
    /// The task the open composer was started for.
    composer_task: Option<rwk_core::handoff_flow::TaskLink>,
    /// The task of a handoff that is waiting for policy and consent.
    queued_task: Option<rwk_core::handoff_flow::TaskLink>,
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
    /// An action to run once the saves started by `Decision::Save` have all succeeded.
    after_save: Option<PendingAction>,
    /// The tabs still to save, in strip order (a save answer may cover several dirty files).
    save_queue: Vec<String>,
    /// Where the saved workspace of each project is read from (the application sets it).
    workspace_source: Option<Arc<dyn WorkspaceStateSource>>,
    /// The workspace state waiting for the write delay to pass.
    pending_workspace: Option<SavedWorkspace>,
    /// The debounce task that hands `pending_workspace` to the application.
    workspace_save_task: Option<Task<()>>,
    /// Previous agent sessions offered for continuation, in saved order.
    offers: Vec<SessionOffer>,
    /// The task and terminal of an assignment that was sent and not yet answered.
    pending_bind: Option<(String, SessionId)>,
    /// Tasks whose terminal was closed; the task list says so.
    closed_tasks: std::collections::HashSet<String>,
    /// Bumped by every offer plan; a plan that finishes after a project switch is dropped.
    offers_generation: u64,
    /// Offers whose continuation is being validated or started right now.
    resuming: std::collections::HashSet<ResumeKey>,
    /// The programs terminals (and the session catalog) run with.
    programs: ProgramPaths,
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
            programs.clone(),
            Arc::new(move |id: &SessionId| wake_gates.wake(id)),
        ));
        let host = SharedHost(pty);
        let sessions = Arc::new(SessionRegistry::new(Box::new(host.clone())));
        let theme = Theme::of(appearance);
        if theme.lcars {
            crate::fonts::ensure_display_font(cx);
        }
        let files = cx.new(FileTreeView::new);
        files.update(cx, |files, cx| files.set_theme(theme, cx));
        let editor = cx.new(|cx| EditorView::themed(theme, window, cx));
        let inspector = cx.new(|_| InspectorView::new());
        inspector.update(cx, |inspector, cx| inspector.set_theme(theme, cx));
        let sizes = cx.new(|_| ResizableState::default());
        let inspector_events = cx.subscribe_in(
            &inspector,
            window,
            |this, _inspector, event: &crate::inspector::InspectorEvent, window, cx| match event {
                crate::inspector::InspectorEvent::StartReceiver(id) => {
                    this.start_receiver(id.clone(), window, cx);
                }
                crate::inspector::InspectorEvent::ApprovalMenu {
                    approval_id,
                    revision,
                    decidable,
                    position,
                } => {
                    this.open_approval_menu(
                        approval_id.clone(),
                        *revision,
                        *decidable,
                        *position,
                        window,
                        cx,
                    );
                }
                crate::inspector::InspectorEvent::HandoffMenu {
                    id,
                    can_start,
                    position,
                } => {
                    this.open_handoff_menu(id.clone(), *can_start, *position, window, cx);
                }
                crate::inspector::InspectorEvent::ApproveHandoff {
                    approval_id,
                    revision,
                } => {
                    this.approve_handoff(approval_id.clone(), *revision, window, cx);
                }
                crate::inspector::InspectorEvent::RejectHandoff {
                    approval_id,
                    revision,
                } => {
                    this.request_reject_handoff(approval_id.clone(), *revision, window, cx);
                }
                crate::inspector::InspectorEvent::SetTaskStatus { task_id, status } => {
                    this.set_task_status(task_id.clone(), *status, window, cx);
                }
                crate::inspector::InspectorEvent::AssignToSelected {
                    task_id,
                    updated_at,
                } => {
                    let selected = this.selected;
                    this.assign_task_to_terminal(
                        task_id.clone(),
                        updated_at.clone(),
                        selected,
                        window,
                        cx,
                    );
                }
                crate::inspector::InspectorEvent::TaskMenu {
                    task_id,
                    title,
                    status,
                    assigned,
                    updated_at,
                    position,
                } => {
                    let info = menus::TaskMenuInfo {
                        task_id: task_id.clone(),
                        title: title.clone(),
                        status: status.clone(),
                        assigned: *assigned,
                        updated_at: updated_at.clone(),
                    };
                    this.open_task_menu(info, *position, window, cx);
                }
                crate::inspector::InspectorEvent::HandoffTask { task_id, title } => {
                    let link = rwk_core::handoff_flow::TaskLink {
                        task_id: task_id.clone(),
                        title: title.clone(),
                    };
                    this.open_composer(Some(link), window, cx);
                }
                crate::inspector::InspectorEvent::ClearAssignee {
                    task_id,
                    updated_at,
                } => {
                    this.assign_task(task_id.clone(), updated_at.clone(), None, window, cx);
                }
            },
        );
        let subscriptions = vec![
            inspector_events,
            cx.subscribe_in(
                &files,
                window,
                |this, _files, event: &FileTreeEvent, window, cx| match event {
                    FileTreeEvent::OpenFile(path) => {
                        this.request_open_file(path.clone(), window, cx);
                    }
                    FileTreeEvent::ContextMenu {
                        path,
                        folder,
                        position,
                    } => this.open_file_menu(path.clone(), *folder, *position, window, cx),
                },
            ),
            cx.subscribe_in(
                &editor,
                window,
                |this, _editor, event: &EditorEvent, window, cx| {
                    this.on_editor_event(event, window, cx);
                },
            ),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.auto_save_now(cx);
                } else {
                    this.refresh_branch(cx);
                    this.refresh_git_badges(cx);
                }
            }),
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
            search_source: None,
            search: None,
            search_subscription: None,
            search_generation: 0,
            auto_save: false,
            format_on_save: false,
            branch: None,
            badge_generation: 0,
            server_status: Vec::new(),
            extra_footer: Vec::new(),
            task_source: None,
            palette_picks_tasks: false,
            git_view: None,
            git_subscription: None,
            git_generation: 0,
            lsp_launcher: None,
            servers: None,
            problems: rwk_core::lsp::Problems::default(),
            problems_view: None,
            problems_subscription: None,
            toolbar_tab: rwk_core::workbench::ToolbarGroup::Files,
            toolbar_layout: rwk_core::workbench::ToolbarLayout::default(),
            settings_view: None,
            file_replace: None,
            file_replace_subscription: None,
            settings_subscription: None,
            locations_view: None,
            locations_subscription: None,
            lsp_task: None,
            lsp_sync_task: None,
            usage_source: None,
            quota: None,
            usage_task: None,
            consent: Arc::new(ConsentRegistry::new()),
            consent_wait: None,
            composer: None,
            composer_subscription: None,
            reject_prompt: None,
            reject_prompt_subscription: None,
            dock: DockLayout::new(),
            drop_hint: None,
            rename_prompt: None,
            menu: None,
            ssh_picker: None,
            ssh_subscription: None,
            ssh_home: None,
            rename_subscription: None,
            handoffs: Vec::new(),
            composer_task: None,
            queued_task: None,
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
            save_queue: Vec::new(),
            workspace_source: None,
            pending_workspace: None,
            workspace_save_task: None,
            offers: Vec::new(),
            pending_bind: None,
            closed_tasks: std::collections::HashSet::new(),
            offers_generation: 0,
            resuming: std::collections::HashSet::new(),
            programs,
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
            reconnectable: self
                .terminals
                .get(self.selected)
                .is_some_and(|pane| pane.ssh.is_some() && !self.session_running(&pane.id)),
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

    /// Previous agent sessions waiting in the dock, in saved order.
    pub fn offers(&self) -> &[SessionOffer] {
        &self.offers
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

    /// Resizes requested of the PTY host so far (a layout that makes terminals resize over
    /// and over shows here). Read by the benchmark.
    #[doc(hidden)]
    pub fn pty_resize_count(&self) -> u64 {
        self.host.0.resize_count()
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
            CommandId::Save => self.save(window, cx),
            CommandId::SplitEditor => self.editor.update(cx, |editor, cx| editor.toggle_split(cx)),
            CommandId::FindInFile => self.find(false, window, cx),
            CommandId::ReplaceInFile => self.find(true, window, cx),
            CommandId::ReplaceRegexInFile => self.open_file_replace(window, cx),
            CommandId::ToggleAutoSave => self.toggle_auto_save(cx),
            CommandId::ToggleFormatOnSave => self.toggle_format_on_save(cx),
            CommandId::CloseTab => {
                if let Some(path) = self.editor.read(cx).path().map(str::to_owned) {
                    self.request_close_tab(path, window, cx);
                }
            }
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
            CommandId::ToolbarTab(number) => {
                if let Some(group) = rwk_core::workbench::ToolbarGroup::ALL
                    .get(usize::from(number).saturating_sub(1))
                {
                    self.toolbar_tab = *group;
                    cx.notify();
                }
            }
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
            CommandId::CloseTerminal => self.request_close_terminal(false, window, cx),
            CommandId::KillTerminal => self.request_close_terminal(true, window, cx),
            CommandId::Ssh => self.open_ssh_picker(window, cx),
            CommandId::Reconnect => self.reconnect_ssh(window, cx),
            CommandId::RenameTerminal => self.open_rename_prompt(self.selected, window, cx),
            CommandId::NewTask => self.open_new_task_prompt(window, cx),
            CommandId::MoveTerminalPrevious
            | CommandId::MoveTerminalNext
            | CommandId::SplitTerminalRight
            | CommandId::SplitTerminalDown
            | CommandId::JoinTerminalPrevious
            | CommandId::UngroupTerminal
            | CommandId::GrowTerminal
            | CommandId::ShrinkTerminal => self.arrange(id, window, cx),
            CommandId::Refresh => self.files.update(cx, |files, cx| files.refresh(cx)),
            CommandId::Settings => self.open_settings(window, cx),
            CommandId::Skin => self.toggle_skin(cx),
            CommandId::GrowDock => self.nudge_dock(DOCK_STEP_PX, window, cx),
            CommandId::ShrinkDock => self.nudge_dock(-DOCK_STEP_PX, window, cx),
            CommandId::Balanced => self.apply_layout(LayoutPreset::Balanced, window, cx),
            CommandId::EditorLayout => self.apply_layout(LayoutPreset::Editor, window, cx),
            CommandId::AgentsLayout => self.apply_layout(LayoutPreset::Agents, window, cx),
            CommandId::Commands | CommandId::Shortcuts => self.toggle_palette(window, cx),
            CommandId::Files => self.open_file_palette(window, cx),
            CommandId::Search => self.open_search(window, cx),
            CommandId::Agent => self.open_agent_chooser(cx),
            CommandId::Handoff => self.open_composer(None, window, cx),
            CommandId::RegisterProject => self.register_project(window, cx),
            CommandId::Memory => self.open_memory(window, cx),
            CommandId::Problems => self.open_problems(window, cx),
            CommandId::SourceControl => self.open_git(window, cx),
            CommandId::RunTask => self.open_task_palette(window, cx),
            CommandId::OpenOutputLocation => self.open_output_location(window, cx),
            CommandId::RenameSymbol => self.open_symbol_prompt(window, cx),
            CommandId::FindReferences => self.find_references(window, cx),
            CommandId::Outline => self.show_outline(window, cx),
            CommandId::RestartServers => self.restart_servers(cx),
            CommandId::Format => self
                .editor
                .update(cx, |editor, cx| editor.format(window, cx)),
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
                    let tasks = std::mem::take(&mut this.palette_picks_tasks);
                    this.close_palette(window, cx);
                    if tasks {
                        this.run_named_task(&path, window, cx);
                    } else {
                        this.request_open_file(path, window, cx);
                    }
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

    /// Whether a panel (palette, picker, prompt or view) floats over the workbench.
    pub fn overlay_open(&self) -> bool {
        self.palette.is_some()
            || self.ssh_picker.is_some()
            || self.composer.is_some()
            || self.reject_prompt.is_some()
            || self.rename_prompt.is_some()
            || self.memory.is_some()
            || self.search.is_some()
            || self.problems_view.is_some()
            || self.locations_view.is_some()
            || self.settings_view.is_some()
            || self.file_replace.is_some()
            || self.git_view.is_some()
    }

    /// Closes the panel on top (the one opened last).
    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_replace.is_some() {
            self.close_file_replace(window, cx);
        } else if self.settings_view.is_some() {
            self.close_settings(window, cx);
        } else if self.git_view.is_some() {
            self.close_git(window, cx);
        } else if self.locations_view.is_some() {
            self.close_locations(window, cx);
        } else if self.problems_view.is_some() {
            self.close_problems(window, cx);
        } else if self.search.is_some() {
            self.close_search(window, cx);
        } else if self.memory.is_some() {
            self.close_memory(window, cx);
        } else if self.rename_prompt.is_some() {
            self.close_rename_prompt(window, cx);
        } else if self.reject_prompt.is_some() {
            self.close_reject_prompt(window, cx);
        } else if self.composer.is_some() {
            self.close_composer(window, cx);
        } else if self.ssh_picker.is_some() {
            self.close_ssh_picker(window, cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        }
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_search_task = None;
        self.palette_picks_tasks = false;
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

    fn toggle_tone(&mut self, cx: &mut Context<Self>) {
        self.appearance.tone = match self.appearance.tone {
            Tone::Dark => Tone::Light,
            Tone::Light => Tone::Dark,
        };
        self.apply_appearance(cx);
    }

    pub(super) fn set_skin(&mut self, skin: Skin, cx: &mut Context<Self>) {
        if self.appearance.skin != skin {
            self.appearance.skin = skin;
            self.apply_appearance(cx);
        }
    }

    pub(super) fn set_tone(&mut self, tone: Tone, cx: &mut Context<Self>) {
        if self.appearance.tone != tone {
            self.appearance.tone = tone;
            self.apply_appearance(cx);
        }
    }

    fn toggle_skin(&mut self, cx: &mut Context<Self>) {
        self.appearance.skin = match self.appearance.skin {
            Skin::Standard => Skin::Lcars,
            Skin::Lcars => Skin::Aperture,
            Skin::Aperture => Skin::Standard,
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
        if theme.lcars {
            crate::fonts::ensure_display_font(cx);
        }
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
        if let Some(prompt) = &self.reject_prompt {
            prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        }
        if let Some(prompt) = &self.rename_prompt {
            prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        }
        if let Some(memory) = &self.memory {
            memory.update(cx, |memory, cx| memory.set_theme(theme, cx));
        }
        if let Some(search) = &self.search {
            search.update(cx, |search, cx| search.set_theme(theme, cx));
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
        self.note_workspace_change(cx);
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

    /// Opens a file of the project. A file that is already open is brought forward instead of
    /// being read again; nothing the user typed can be lost by opening a new tab.
    pub fn request_open_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.request_open_file_at(path, None, window, cx);
    }

    /// Opens a file of the project with the cursor on `line` (1-based), as a search hit asks.
    /// A clean file that is already open is reloaded to move the cursor; a dirty one is only
    /// brought forward, never reloaded.
    pub fn request_open_file_at(
        &mut self,
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project.is_none() {
            return;
        }
        let (already_open, clean) = {
            let editor = self.editor.read(cx);
            (
                editor.contains(&path),
                !editor.dirty_paths().contains(&path),
            )
        };
        if already_open {
            if clean && line.is_some() {
                // A hit with a line: read the file again and put the cursor there.
                self.perform(PendingAction::OpenFile { path, line }, window, cx);
            } else {
                // Already open: bring its tab forward without touching the buffer.
                self.editor
                    .update(cx, |editor, cx| editor.activate_tab(&path, window, cx));
            }
            return;
        }
        self.perform(PendingAction::OpenFile { path, line }, window, cx);
    }

    /// Closes an editor tab; a tab with unsaved edits is asked about first.
    pub fn request_close_tab(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self.editor.read(cx).dirty_paths().contains(&path);
        if dirty {
            self.pending = Some(PendingAction::CloseTab(path));
            cx.notify();
            return;
        }
        self.perform(PendingAction::CloseTab(path), window, cx);
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
                | PendingAction::CloseTerminal { .. }
                | PendingAction::DeleteEntry { .. }
                | PendingAction::OpenProjectStoppingSessions(_)
                | PendingAction::ChooseAgent
                | PendingAction::Consent(..),
            ) => {}
            (Decision::Save, PendingAction::CloseTab(path)) => {
                self.save_queue = vec![path.clone()];
                self.after_save = Some(action);
                self.save_next(window, cx);
            }
            (Decision::Save, _) => {
                self.save_queue = self.editor.read(cx).dirty_paths();
                self.after_save = Some(action);
                self.save_next(window, cx);
            }
            (Decision::Discard, PendingAction::CloseTab(path)) => {
                self.editor
                    .update(cx, |editor, cx| editor.close_tab(path, window, cx));
            }
            (Decision::Discard, _) => {
                self.perform(action, window, cx);
            }
            (
                Decision::Confirm,
                PendingAction::StopTerminal(_)
                | PendingAction::CloseTerminal { .. }
                | PendingAction::DeleteEntry { .. }
                | PendingAction::OpenProjectStoppingSessions(_),
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
            EditorEvent::Saved(saved) => {
                self.save_queue.retain(|path| path != &saved.path);
                self.refresh_git_badges(cx);
                self.save_next(window, cx);
            }
            EditorEvent::Conflict | EditorEvent::SaveFailed(_) => {
                // The edits are still there: the pending action must not proceed.
                if !self.save_queue.is_empty() || self.after_save.is_some() {
                    self.save_queue.clear();
                    self.after_save = None;
                    self.say("Kaydedilemedi; işlem iptal edildi", cx);
                }
            }
            EditorEvent::CloseRequested(path) => {
                self.request_close_tab(path.clone(), window, cx);
            }
            EditorEvent::TabContextMenu { position, .. } => {
                self.open_context_menu(
                    rwk_core::workbench::MenuTarget::EditorTab,
                    *position,
                    window,
                    cx,
                );
            }
            EditorEvent::TextContextMenu {
                position,
                selection,
                editable,
            } => self.open_text_menu(*selection, *editable, *position, window, cx),
            EditorEvent::Opened(path) => {
                self.lsp_opened(path, cx);
                self.note_workspace_change(cx);
            }
            EditorEvent::Closed(path) => {
                self.lsp_closed(path, cx);
                self.note_workspace_change(cx);
            }
            EditorEvent::Edited => {
                self.lsp_edited(cx);
                self.note_workspace_change(cx);
            }
            EditorEvent::Activated(_) => self.note_workspace_change(cx),
            EditorEvent::OpenLocation { path, line } => {
                self.request_open_file_at(path.clone(), Some(*line), window, cx);
            }
        }
    }

    /// Saves the next queued tab; when none is left the action that waited for the saves runs.
    /// Tabs that were closed or are no longer dirty are skipped, so the queue always drains.
    fn save_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.clone() else {
            self.save_queue.clear();
            self.after_save = None;
            return;
        };
        loop {
            let Some(path) = self.save_queue.first().cloned() else {
                if let Some(action) = self.after_save.take() {
                    self.perform(action, window, cx);
                }
                return;
            };
            let saveable = {
                let editor = self.editor.read(cx);
                editor.contains(&path) && editor.dirty_paths().contains(&path)
            };
            if saveable {
                self.editor
                    .update(cx, |editor, cx| editor.save_path(&path, project, cx));
                return;
            }
            self.save_queue.remove(0);
        }
    }

    fn perform(&mut self, action: PendingAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            PendingAction::OpenFile { path, line } => {
                if let Some(project) = self.project.clone() {
                    self.editor.update(cx, |editor, cx| {
                        editor.open_path_at(project, path, line, window, cx)
                    });
                }
            }
            PendingAction::CloseTab(path) => {
                self.editor
                    .update(cx, |editor, cx| editor.close_tab(&path, window, cx));
            }
            PendingAction::OpenProjectStoppingSessions(root) => {
                self.perform(PendingAction::OpenProject(root), window, cx);
            }
            PendingAction::OpenProject(root) => match ProjectRoot::open(root) {
                Ok(project) => {
                    // The state of the project being left is written before anything is torn down.
                    self.flush_workspace(cx);
                    self.close_palette(window, cx);
                    self.close_search(window, cx);
                    self.close_problems(window, cx);
                    self.close_git(window, cx);
                    self.stop_terminals(false);
                    self.editor
                        .update(cx, |editor, cx| editor.clear(window, cx));
                    let project = Arc::new(project);
                    self.files
                        .update(cx, |files, cx| files.set_project(Arc::clone(&project), cx));
                    let root = project.info().root.clone();
                    self.project = Some(project);
                    self.server_status.clear();
                    self.refresh_branch(cx);
                    self.refresh_git_badges(cx);
                    self.restart_language_servers(cx);
                    self.inspector
                        .update(cx, |inspector, cx| inspector.set_project(Some(root), cx));
                    self.register_if_unknown(window, cx);
                    self.restore_workspace(window, cx);
                    if let Some(command) = self.after_pick.take() {
                        self.run(command, window, cx);
                    }
                    cx.notify();
                }
                Err(error) => self.say(error.message, cx),
            },
            PendingAction::CloseWindow => {
                // Next time the project opens with what was open now, sessions included.
                self.flush_workspace(cx);
                self.stop_terminals(true);
                self.close_confirmed = true;
            }
            PendingAction::CloseTerminal { id, force } => {
                self.remove_terminal(&id, force, window, cx);
            }
            PendingAction::DeleteEntry { path, .. } => self.delete_entry(&path, cx),
            PendingAction::StopTerminal(id) => {
                if let Err(error) = self.sessions.stop(&id) {
                    self.say(error.message, cx);
                }
                self.note_workspace_change(cx);
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
        self.dock = DockLayout::new();
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
        let mut gone = Vec::new();
        self.terminals.retain(|pane| {
            let alive = running.contains(&pane.id);
            if !alive {
                gates.detach(&pane.id);
                host.remove(&pane.id);
                gone.push(pane.id.clone());
            }
            alive
        });
        for id in gone {
            self.dock.remove(&id);
        }
        self.selected = self.selected.min(self.terminals.len().saturating_sub(1));
    }

    /// Starts one terminal pane for the open project. `false` (with a message) when it could not
    /// start. Silent about layout and the saved workspace: a restore seeds panes without
    /// nudging the layout or noting a change it is itself restoring.
    fn spawn_terminal(
        &mut self,
        kind: SessionKind,
        launch: SessionLaunch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(project) = self.project.clone() else {
            return false;
        };
        self.prune_dead_panes();
        if self.terminals.len() >= MAX_SESSIONS {
            return false;
        }
        let info = match self
            .sessions
            .start(&project, kind, &launch, NEW_TERMINAL_SIZE)
        {
            Ok(info) => info,
            Err(error) => {
                self.say(error.message, cx);
                return false;
            }
        };
        let Some(engine) = self.host.0.engine(&info.id) else {
            self.say("Terminal oturumu başlatılamadı", cx);
            return false;
        };
        let (gate, receiver) = self.gates.attach(&info.id);
        let host: Arc<dyn TerminalHost> = Arc::new(self.host.clone());
        let appearance = terminal_appearance(self.theme());
        let id = info.id.clone();
        let view = cx.new(|cx| TerminalView::new(engine, host, id, appearance, gate, receiver, cx));
        let (resume, name) = match &launch {
            SessionLaunch::Resume(key) => (Some(key.clone()), None),
            SessionLaunch::Fresh { name, .. } => (None, name.clone()),
            // A remote shell is named after where it leads.
            SessionLaunch::Ssh(target) => (None, SessionName::parse(&target.display()).ok()),
            SessionLaunch::Task(task) => (None, SessionName::parse(&task.name).ok()),
        };
        let ssh = match &launch {
            SessionLaunch::Ssh(target) => Some(target.clone()),
            _ => None,
        };
        let _ = self.dock.push_default(info.id.clone());
        let open_location = cx.subscribe_in(
            &view,
            window,
            |this, _view, event: &crate::terminal::TerminalEvent, window, cx| {
                let crate::terminal::TerminalEvent::Open { line, column } = event;
                this.open_clicked_location(line, *column, window, cx);
            },
        );
        self.terminals.push(TerminalPane {
            kind,
            id: info.id,
            name,
            resume,
            ssh,
            task: None,
            run: matches!(launch, SessionLaunch::Task(_)),
            view,
            _open_location: open_location,
        });
        self.dock_visible = true;
        let last = self.terminals.len() - 1;
        self.select_terminal(last, window, cx);
        true
    }

    /// The view of terminal `index`.
    pub fn terminal_view(&self, index: usize) -> Option<&Entity<TerminalView>> {
        self.terminals.get(index).map(|pane| &pane.view)
    }

    /// The name of terminal `index`, when the user gave it one.
    pub fn terminal_name(&self, index: usize) -> Option<&SessionName> {
        self.terminals.get(index)?.name.as_ref()
    }

    /// What the title strip of terminal `index` shows: its name, or the kind with an ordinal
    /// among the terminals of the same kind ("claude 2"), so unnamed panes stay tellable apart.
    pub fn terminal_title(&self, index: usize) -> String {
        let Some(pane) = self.terminals.get(index) else {
            return String::new();
        };
        if let Some(name) = &pane.name {
            return name.to_string();
        }
        let label = pane.kind.agent().unwrap_or("shell");
        let ordinal = self.terminals[..=index]
            .iter()
            .filter(|other| other.kind == pane.kind)
            .count();
        format!("{label} {ordinal}")
    }

    /// Sets or clears the name of terminal `index` and remembers it in the saved workspace. The
    /// daemon keeps the name a run started with; this is the local label.
    pub fn rename_terminal(
        &mut self,
        index: usize,
        name: Option<SessionName>,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.terminals.get_mut(index) else {
            return;
        };
        pane.name = name;
        self.note_workspace_change(cx);
        cx.notify();
    }

    /// The open rename prompt.
    pub fn rename_prompt(&self) -> Option<&Entity<crate::rename::RenameTerminalPrompt>> {
        self.rename_prompt.as_ref()
    }

    /// Asks for a new name for terminal `index`.
    pub fn open_rename_prompt(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::rename::{RenamePromptEvent, RenameTerminalPrompt};

        if self.rename_prompt.is_some() {
            return;
        }
        let Some(pane) = self.terminals.get(index) else {
            return;
        };
        let current = pane.name.clone();
        let running_agent = pane.kind != SessionKind::Shell;
        let theme = self.theme();
        let prompt =
            cx.new(|cx| RenameTerminalPrompt::new(current.as_ref(), running_agent, window, cx));
        prompt.update(cx, |prompt, cx| prompt.set_theme(theme, cx));
        // The pane is found again by id: terminals may have come and gone while the prompt was open.
        let id = pane.id.clone();
        self.rename_subscription = Some(cx.subscribe_in(
            &prompt,
            window,
            move |this, _prompt, event: &RenamePromptEvent, window, cx| match event {
                RenamePromptEvent::Rename(name) => {
                    if let Some(index) = this.terminals.iter().position(|pane| pane.id == id) {
                        this.rename_terminal(index, name.clone(), cx);
                    }
                    this.close_rename_prompt(window, cx);
                }
                RenamePromptEvent::Close => this.close_rename_prompt(window, cx),
                RenamePromptEvent::NewTask(_) | RenamePromptEvent::EntryName(_) => {}
            },
        ));
        self.rename_prompt = Some(prompt);
        cx.notify();
    }

    /// Closes the rename prompt and returns focus to the workbench.
    pub fn close_rename_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rename_prompt = None;
        self.rename_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn new_terminal(&mut self, kind: SessionKind, window: &mut Window, cx: &mut Context<Self>) {
        if !self.spawn_terminal(kind, SessionLaunch::default(), window, cx) {
            return;
        }
        self.after_started_terminal(window, cx);
    }

    /// What every user-started terminal does after its pane exists: nudge the layout once the
    /// dock is crowded, and remember the new state. A restore calls neither.
    fn after_started_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminals.len() >= 3 && self.layout != LayoutPreset::Agents {
            self.apply_layout(LayoutPreset::Agents, window, cx);
        }
        self.note_workspace_change(cx);
    }

    fn select_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.terminals.get(index) else {
            return;
        };
        self.selected = index;
        let id = pane.id.clone();
        let handle = pane.view.read(cx).focus_handle().clone();
        // A tab that was hidden is painted for the first time on the next frame; focus can only
        // land on it then.
        let was_shown = self.dock.is_shown(&id);
        self.dock.activate(&id);
        if was_shown {
            window.focus(&handle, cx);
        } else {
            window.on_next_frame(move |window, cx| window.focus(&handle, cx));
        }
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
            input: self.palette.is_some() || self.ssh_picker.is_some(),
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
        // An open menu owns the keyboard like a dialog does.
        if self.menu_key(event.keystroke.key.as_str(), cx) {
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
        aperture: theme.skin == Skin::Aperture,
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        // A process that ended on its own is noticed here; the task list only repaints when the
        // set of marked tasks changed.
        self.sync_task_marks(cx);
        let theme = self.theme();
        let dock = div()
            .id("dock-panel")
            .size_full()
            .flex()
            .flex_col()
            // Terminals, titles, the inspector and the offers stop their own right clicks; what
            // reaches the panel itself is the empty part of the dock.
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    this.open_context_menu(
                        rwk_core::workbench::MenuTarget::DockBackground,
                        event.position,
                        window,
                        cx,
                    );
                }),
            )
            .children(self.inspector_visible.then(|| {
                div()
                    .h(px(INSPECTOR_HEIGHT))
                    .flex_none()
                    .border_b_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(self.inspector.clone())
            }))
            .children(self.offer_rows(cx))
            .children(self.dock_tree(theme, cx));
        let main = h_resizable("workbench")
            .with_state(&self.sizes)
            .child(
                resizable_panel()
                    .size(px(260.0))
                    .size_range(px(150.0)..px(640.0))
                    .child(self.files.clone()),
            )
            .child(resizable_panel().child(self.editor.clone()))
            .child(
                resizable_panel()
                    .size(px(380.0))
                    .visible(self.dock_visible)
                    .child(dock),
            );
        let dialog = self.dialog().map(|dialog| {
            let buttons = dialog.buttons.iter().map(|(decision, label)| {
                let decision = *decision;
                div()
                    .id(SharedString::from(format!("dialog-{label}")))
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::rgb(theme.accent_ink))
                    .hover(move |style| style.bg(gpui::rgb(theme.raised)))
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
                .p_4()
                .rounded_md()
                .shadow_lg()
                .bg(gpui::rgb(theme.panel))
                .border_1()
                .border_color(gpui::rgb(if theme.lcars {
                    theme.accent
                } else {
                    theme.line
                }))
                .text_color(gpui::rgb(theme.text))
                .child(div().child(dialog.title))
                .child(div().py_3().child(dialog.message))
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
            .child(self.toolbar_row(theme, cx))
            .child(div().flex_1().min_h_0().child(main))
            .child(
                div()
                    .px_3()
                    .py_0p5()
                    .min_h(px(22.0))
                    .border_t_1()
                    .border_color(gpui::rgb(theme.line))
                    .bg(gpui::rgb(theme.panel))
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
                    .child(self.footer_bar(cx)),
            )
            .children(self.overlay_open().then(|| {
                // A click beside an overlay closes it, and never also presses what is below.
                div()
                    .id("overlay-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .bg(gpui::rgba(0x00000055))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_overlay(window, cx)),
                    )
            }))
            .children(self.palette.clone())
            .children(self.ssh_picker.clone())
            .children(self.composer.clone())
            .children(self.reject_prompt.clone())
            .children(self.rename_prompt.clone())
            .children(self.memory.clone())
            .children(self.search.clone())
            .children(self.problems_view.clone())
            .children(self.locations_view.clone())
            .children(self.git_view.clone())
            .children(self.settings_view.clone())
            .children(self.file_replace.clone())
            .children(self.menu.as_ref().map(|open| open.view.clone()))
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
