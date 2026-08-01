use std::path::PathBuf;

use raios_core::requirements::Requirement;
use raios_runtime::discovery::{AgentInfo, SkillInfo};
use raios_runtime::filebrowser::{AgentRuleGroup, FileEntry, RecentProject};
use raios_runtime::indexer::{ProjectIndex, SearchResult};

// ─── Extension State ──────────────────────────────────────────────────────────

// ─── Extension State ──────────────────────────────────────────────────────────

/// Information metadata for an extension command definition.
#[derive(Debug, Clone)]
pub struct ExtCmdInfo {
    /// Command name string.
    pub name: String,
    /// Description of the command functionality.
    pub description: String,
}

/// Schema definition and current value for an extension configuration field.
#[derive(Debug, Clone)]
pub struct ExtConfigField {
    /// Configuration key identifier.
    pub key: String,
    /// Display label for the configuration field.
    pub label: String,
    /// Value type name (e.g., `"string"`, `"secret"`).
    pub field_type: String,
    /// Field purpose description.
    pub description: String,
    /// Current string value of the field.
    pub value: String,
    /// Whether the value should be masked in the UI.
    pub masked: bool,
}

/// Operational state of a systemd or background service managed by an extension.
#[derive(Debug, Clone)]
pub struct ExtServiceStatus {
    /// Service name identifier.
    pub name: String,
    /// `true` if service is active/running.
    pub active: bool,
}

/// Complete metadata descriptor for a discovered extension module.
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    /// Extension display name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Description of extension purpose.
    pub description: String,
    /// Root directory path of the extension.
    pub path: PathBuf,
    /// Command definitions declared by the extension.
    pub commands: Vec<ExtCmdInfo>,
    /// Configuration schema fields.
    pub config_schema: Vec<ExtConfigField>,
    /// List of managed service names.
    pub services: Vec<String>,
    /// Real-time status of managed services.
    pub service_statuses: Vec<ExtServiceStatus>,
}

/// Focused interactive sub-panel within the extension manager view.
#[derive(Debug, Default, PartialEq, Clone)]
pub enum ExtFocus {
    /// Commands list sub-panel focused.
    #[default]
    Commands,
    /// Configuration key-value sub-panel focused.
    Config,
}

/// Application state for the extension manager panel.
#[derive(Debug, Default)]
pub struct ExtState {
    /// List of discovered extensions.
    pub extensions: Vec<ExtensionInfo>,
    /// Selected index in the extension list.
    pub ext_cursor: usize,
    /// Active sub-panel focus.
    pub focus: ExtFocus,
    /// Selected index in the commands list.
    pub cmd_cursor: usize,
    /// Selected index in the configuration list.
    pub cfg_cursor: usize,
    /// Whether an input field is currently being edited.
    pub editing: bool,
    /// Active text input string.
    pub input: String,
    /// Optional status or error message.
    pub status: Option<String>,
    /// Whether extension scanning has completed.
    pub loaded: bool,
}

// ─── State ───────────────────────────────────────────────────────────────────

/// Active navigation route or major UI view state of the TUI application.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// Initializing system components.
    Booting,
    /// Running initial setup wizard.
    Setup,
    /// Main dashboard control plane.
    Dashboard,
    /// Viewing file contents.
    FileView,
    /// Editing a file inline.
    FileEdit,
    /// Viewing project detail overview.
    ProjectDetail,
    /// Viewing project health and compliance.
    HealthView,
    /// Executing search queries.
    Search,
    /// Visualizing MemPalace memory hierarchy.
    MemPalaceView,
    /// Viewing graphify architecture report.
    GraphReport,
    /// Viewing Git diff output.
    GitDiffView,
    /// Viewing help and keybinding guide.
    HelpView,
}

// ─── Background messages ──────────────────────────────────────────────────────

/// Asynchronous message variants produced by background worker threads for the main UI loop.
pub enum BgMsg {
    /// Result of an individual boot preflight check.
    BootResult {
        /// Name of the preflight check.
        name: String,
        /// `true` if passed.
        pass: bool,
        /// `true` if all checks complete.
        done: bool,
    },
    /// Transition UI to setup wizard mode.
    TransitionToSetup,
    /// Transition UI to main dashboard mode.
    TransitionToDashboard,
    /// Recent project history loaded.
    RecentProjects(Vec<RecentProject>),
    /// Discovered agent harnesses loaded.
    Agents(Vec<AgentInfo>),
    /// Discovered skill packages loaded.
    Skills(Vec<SkillInfo>),
    /// Master constitution files loaded.
    MasterFiles(Vec<FileEntry>),
    /// Agent configuration files loaded.
    AgentFiles(Vec<FileEntry>),
    /// Security policy files loaded.
    PolicyFiles(Vec<FileEntry>),
    /// MemPalace data files loaded.
    MemPalaceFiles(Vec<FileEntry>),
    /// Synchronization operation completed successfully.
    SyncDone(String),
    /// Synchronization operation failed.
    SyncError(String),
    /// Code search index initialized and ready.
    IndexReady(ProjectIndex),
    /// Code search indexing failed.
    IndexError(String),
    /// Agent rule groups parsed and loaded.
    AgentRuleGroups(Vec<AgentRuleGroup>),
    /// Workspace project entities loaded.
    Projects(Vec<raios_core::entities::EntityProject>),
    /// Detailed project information opened.
    ProjectOpened(raios_surface_tui::app::ProjectDetailData),
    /// Complete health inspection report loaded.
    HealthReport(Vec<raios_runtime::health::ProjectHealth>),
    /// Build/test dependency health result produced.
    BuildTestDepsResult {
        /// Project index.
        idx: usize,
        /// Updated health report.
        health: Box<raios_runtime::health::ProjectHealth>,
    },
    /// Periodic state synchronization snapshot received.
    StateSync {
        /// Tracked projects list.
        projects: Vec<raios_core::entities::EntityProject>,
        /// Health reports.
        health_reports: Vec<raios_runtime::health::ProjectHealth>,
        /// Active agent processes.
        active_agents: Vec<raios_runtime::daemon::proxy::AgentProcess>,
        /// Whether search index is ready.
        index_ready: bool,
        /// Pending handover approval count.
        handover_count: u32,
        /// Pending file change approval requests.
        pending_file_changes: Vec<raios_runtime::daemon::state::FileChangeApproval>,
        /// Sentinel monitored file statuses.
        sentinel_files: Vec<raios_runtime::daemon::state::SentinelFileStatus>,
    },
    /// Sentinel file status update received.
    SentinelUpdate {
        /// Project name.
        project: String,
        /// Status string.
        status: String,
        /// Error count.
        error_count: usize,
    },
    /// Activity feed items updated.
    ActivityUpdate(Vec<Activity>),
    /// New log record entry appended.
    NewLog(LogEntry),
    /// MemPalace rooms built.
    MemPalaceBuilt(Vec<raios_core::mempalace::MemRoom>),
    /// Task list updated.
    Tasks(Vec<raios_runtime::tasks::Task>),
    /// Vault project status list loaded.
    VaultStatus(Vec<String>),
    /// Active network ports scanned.
    ActivePorts(Vec<u16>),
    /// Daemon control-plane event received.
    ControlEvent(raios_contracts::Event),
    /// AI readiness audit report generated.
    AiAuditReport(raios_runtime::system_scan::AiAuditReport),
    /// Watched file modified on disk.
    FileChanged(PathBuf),
    /// Search result hits returned.
    SearchResults(Vec<SearchResult>),
    /// Agent handover request approved.
    HandoverApproved {
        /// Target agent name.
        target: String,
        /// Instruction prompt text.
        instruction: String,
        /// Handoff counter.
        count: u32,
    },
    /// Human approval required for an agent action.
    HumanApprovalRequired {
        /// Target agent name.
        target: String,
        /// Action instruction.
        instruction: String,
        /// Reason for approval request.
        reason: String,
    },
    /// Human approval resolution result.
    HumanApprovalResult {
        /// Status string (`"approved"` or `"rejected"`).
        status: String,
    },
    /// File mutation approval requested.
    FileChangeRequested {
        /// Approval request object.
        approval: raios_runtime::daemon::state::FileChangeApproval,
    },
    /// Portfolio summary statistics computed.
    StatsReady(PortfolioStats),
    /// System agent scan status ready.
    AgentStatusReady(AgentStatus),
    /// Agent invocation session started.
    AgentStarted {
        /// Run session UUID.
        agent_id: String,
        /// Harness name.
        name: String,
        /// Path to project directory.
        project_path: String,
    },
    /// Agent invocation session stopped.
    AgentStopped {
        /// Run session UUID.
        agent_id: String,
        /// Harness name.
        name: String,
        /// Exit status string.
        final_status: String,
    },
    /// Health report delta update.
    HealthDelta(Vec<raios_runtime::health::ProjectHealth>),
    /// Wizard action logs updated.
    WizardActions(Vec<WizardAction>),
    /// Wizard execution completed.
    WizardDone,
    /// Git operation completed.
    GitActionDone {
        /// Project name.
        project: String,
        /// Action description.
        action: String,
        /// `true` if succeeded.
        ok: bool,
        /// Output message.
        message: String,
    },
    /// Remote shell command result.
    RemoteCommandResult {
        /// Command stdout/stderr text.
        output: String,
    },
    /// Workspace extension list loaded.
    ExtensionsLoaded(Vec<ExtensionInfo>),
    /// Output line received from an extension command.
    ExtCmdOutput {
        /// Extension name.
        ext: String,
        /// Command name.
        cmd: String,
        /// Text line emitted.
        line: String,
    },
}

/// Timestamped activity record entry displayed in the system activity timeline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    /// ISO-8601 UTC timestamp string.
    pub timestamp: String,
    /// Category source of the activity (`"Git"`, `"Agent"`, `"System"`).
    pub source: String,
    /// Message description text.
    pub message: String,
    /// Log severity level string (`"Info"`, `"Warning"`, `"Error"`).
    pub level: &'static str,
}

/// Log output record emitted by agent harnesses or background services.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    /// ISO-8601 UTC timestamp string.
    pub timestamp: String,
    /// Sender tag or service name.
    pub sender: String,
    /// Log message content text.
    pub content: String,
}

// ─── Setup field ──────────────────────────────────────────────────────────────

/// Field input state used in configuration setup screens.
#[derive(Debug, Clone)]
pub struct SetupField {
    /// Field display label.
    pub label: &'static str,
    /// Helpful explanation text.
    pub hint: &'static str,
    /// Current input string value.
    pub value: String,
    /// Whether the value was populated by auto-detection.
    pub auto_detected: bool,
}

impl SetupField {
    /// Constructs a new `SetupField` with the given label and hint text.
    pub fn new(label: &'static str, hint: &'static str) -> Self {
        Self {
            label,
            hint,
            value: String::new(),
            auto_detected: false,
        }
    }
    /// Updates `SetupField` with an auto-detected file system path string.
    pub fn with_detected(mut self, path: Option<PathBuf>) -> Self {
        if let Some(p) = path {
            self.value = p.to_string_lossy().into_owned();
            self.auto_detected = true;
        }
        self
    }
}

// ─── Wizard ──────────────────────────────────────────────────────────────────

pub use raios_surface_tui::setup_wizard::{AgentStatus, WizardAction, WizardStep};

// ─── Project Sort Mode ───────────────────────────────────────────────────────

/// Ordering criteria for sorting projects in the project list view.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SortMode {
    /// Sort alphabetically by project name.
    #[default]
    Name,
    /// Sort by policy compliance letter grade.
    Grade,
    /// Sort by uncommitted Git file changes.
    GitDirty,
    /// Sort by project workspace category.
    Category,
    /// Sort by project status string.
    Status,
}

impl SortMode {
    /// Advances to the next `SortMode` variant in cyclic order.
    pub fn next(&self) -> Self {
        match self {
            Self::Name => Self::Grade,
            Self::Grade => Self::GitDirty,
            Self::GitDirty => Self::Category,
            Self::Category => Self::Status,
            Self::Status => Self::Name,
        }
    }
    /// Returns a short human-readable string label for this sort mode.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Grade => "Grade",
            Self::GitDirty => "Dirty",
            Self::Category => "Category",
            Self::Status => "Status",
        }
    }
}

// ─── Portfolio stats cache ───────────────────────────────────────────────────

/// Aggregated summary statistics across all workspace projects.
#[derive(Debug, Clone, Default)]
pub struct PortfolioStats {
    /// Total count of tracked projects.
    pub total: usize,
    /// Number of active projects.
    pub active: usize,
    /// Number of archived projects.
    pub archived: usize,
    /// Projects with uncommitted Git modifications.
    pub dirty: usize,
    /// Projects missing a `memory.md` file.
    pub no_memory: usize,
    /// Projects missing a `SIGMAP.md` file.
    pub no_sigmap: usize,
    /// Projects missing a remote GitHub origin.
    pub no_github: usize,
    /// Projects with compliance Grade A.
    pub grade_a: usize,
    /// Projects with compliance Grade B.
    pub grade_b: usize,
    /// Projects with compliance Grade C.
    pub grade_c: usize,
    /// Projects with compliance Grade D.
    pub grade_d: usize,
    /// Category name containing the highest number of modified repositories.
    pub top_dirty_category: String,
}

// ─── Constitution State ────────────────────────────────────────────────────────

/// Target file location for constitution viewing and editing.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstitutionTarget {
    /// The global unified constitution file (`AGENT_CONSTITUTION.md`).
    Global {
        /// Absolute path to the global constitution file.
        path: PathBuf,
    },
    /// A project-specific constitution or agent configuration file.
    ProjectFile {
        /// Absolute path to the project file.
        path: PathBuf,
        /// Kind classifier of the project file.
        kind: raios_runtime::constitution::ProjectFileKind,
    },
}

impl ConstitutionTarget {
    /// Returns a reference to the file system path of this constitution target.
    pub fn path(&self) -> &std::path::Path {
        match self {
            ConstitutionTarget::Global { path } => path,
            ConstitutionTarget::ProjectFile { path, .. } => path,
        }
    }

    /// Returns a human-readable display title for this constitution target.
    pub fn label(&self) -> String {
        match self {
            ConstitutionTarget::Global { .. } => "Global Constitution".to_string(),
            ConstitutionTarget::ProjectFile { kind, .. } => kind.filename().to_string(),
        }
    }
}

/// Flattened outline tree node type for constitution structure navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineRow {
    /// Top-level section header node.
    Section {
        /// Section index in the document.
        idx: usize,
    },
    /// Sub-section child header node.
    Child {
        /// Parent section index.
        idx: usize,
        /// Child subsection index.
        child_idx: usize,
    },
    /// Individual requirement or policy item node.
    Item {
        /// Section index.
        idx: usize,
        /// Optional child subsection index.
        child_idx: Option<usize>,
        /// Item index within section/child.
        item_idx: usize,
    },
}

/// Flattens nested constitution sections into a sequential vector of `OutlineRow` tree nodes.
pub fn flatten_sections(
    sections: &[raios_runtime::constitution::ConstitutionSection],
) -> Vec<OutlineRow> {
    let mut rows = Vec::new();
    for (idx, sec) in sections.iter().enumerate() {
        rows.push(OutlineRow::Section { idx });
        for item_idx in 0..sec.items.len() {
            rows.push(OutlineRow::Item {
                idx,
                child_idx: None,
                item_idx,
            });
        }
        for (child_idx, child) in sec.children.iter().enumerate() {
            rows.push(OutlineRow::Child { idx, child_idx });
            for item_idx in 0..child.items.len() {
                rows.push(OutlineRow::Item {
                    idx,
                    child_idx: Some(child_idx),
                    item_idx,
                });
            }
        }
    }
    rows
}

/// Multi-step navigation wizard phase for creating new constitution sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreatorStep {
    /// Select target document for section creation.
    #[default]
    ChooseTarget,
    /// Enter notes and section content.
    Notes,
    /// Preview generated section.
    Preview,
    /// Confirmation dialog before updating the global constitution.
    ConfirmGlobal,
}

/// In-progress state for the constitution section creation wizard.
#[derive(Debug, Default, Clone)]
pub struct CreatorState {
    /// Whether section creator wizard is active.
    pub active: bool,
    /// Whether target is global constitution.
    pub target_is_global: bool,
    /// Current wizard step phase.
    pub step: CreatorStep,
    /// Input buffer for notes and content.
    pub notes_input: String,
}

/// Staged diff details awaiting user confirmation before persisting a constitution edit.
#[derive(Debug, Clone)]
pub struct PendingConstitutionSave {
    /// File path to save.
    pub path: PathBuf,
    /// New content string.
    pub new_content: String,
    /// Diff lines showing additions and deletions.
    pub diff_lines: Vec<String>,
    /// Number of lines added.
    pub added: usize,
    /// Number of lines removed.
    pub removed: usize,
}

/// Application state for the constitution viewer, editor, and outline navigation panel.
#[derive(Debug, Default)]
pub struct ConstitutionState {
    /// Open constitution targets tabs.
    pub tabs: Vec<ConstitutionTarget>,
    /// Selected tab index.
    pub active_tab: usize,
    /// Parsed constitution section structures.
    pub sections: Vec<raios_runtime::constitution::ConstitutionSection>,
    /// Flattened outline rows.
    pub rows: Vec<OutlineRow>,
    /// Selected cursor line in outline navigation.
    pub outline_cursor: usize,
    /// Whether item editing is active.
    pub item_editing: bool,
    /// Text input for item editing.
    pub item_input: String,
    /// Staged pending save state, if any.
    pub pending_save: Option<PendingConstitutionSave>,
    /// Section creation wizard state.
    pub creator: CreatorState,
}

/// Application state for the full-text and trigram code search view.
#[derive(Debug, Default)]
pub struct SearchState {
    /// Active search query string.
    pub query: String,
    /// Returned search match results.
    pub results: Vec<SearchResult>,
    /// Selected result cursor index.
    pub cursor: usize,
    /// Loaded project index data.
    pub index: Option<ProjectIndex>,
    /// Whether index build is in progress.
    pub is_indexing: bool,
    /// Status message string.
    pub status: Option<String>,
}

/// Application state for the MemPalace memory hierarchy visualization.
#[derive(Debug, Default)]
pub struct MempalaceState {
    /// Parsed memory room structures.
    pub rooms: Vec<raios_core::mempalace::MemRoom>,
    /// Selected room cursor index.
    pub room_cursor: usize,
    /// Selected project cursor index within room.
    pub proj_cursor: Option<usize>,
    /// Expansion state flags for rooms.
    pub expanded: Vec<bool>,
    /// Category filter text string.
    pub filter: String,
    /// Whether MemPalace build is running.
    pub is_building: bool,
    /// File entries in memory palace.
    pub files: Vec<FileEntry>,
}

/// Application state for project compliance and health inspection reports.
#[derive(Debug, Default)]
pub struct HealthState {
    /// Inspection report items for workspace projects.
    pub report: Vec<raios_runtime::health::ProjectHealth>,
    /// Selected report cursor index.
    pub cursor: usize,
    /// Whether health inspection scan is running.
    pub is_checking: bool,
    /// Detailed compliance scanner report.
    pub compliance: Option<raios_runtime::compliance::ComplianceReport>,
    /// Whether auto-fix process is running.
    pub is_fixing: bool,
    /// Auto-fix status message.
    pub fix_status: Option<String>,
}

/// Application state for the control-plane task management view.
#[derive(Debug, Default)]
pub struct TaskState {
    /// Loaded tasks list.
    pub list: Vec<raios_runtime::tasks::Task>,
    /// Selected task cursor index.
    pub cursor: usize,
}

/// System scan, daemon proxy, and active process state.
#[derive(Debug, Default)]
pub struct SystemState {
    /// AI audit inspection report.
    pub report: Option<raios_runtime::system_scan::AiAuditReport>,
    /// Whether system scan is in progress.
    pub is_scanning: bool,
    /// Boot preflight check results.
    pub boot_results: Vec<(String, bool)>,
    /// Active agent process instances.
    pub active_agents: Vec<raios_runtime::daemon::proxy::AgentProcess>,
    /// Selected active agent index.
    pub selected_agent_idx: usize,
    /// Whether synchronization operation is running.
    pub is_syncing: bool,
    /// Sync status message.
    pub sync_status: Option<String>,
    /// List of project names present in Obsidian Vault.
    pub vault_projects: Vec<String>,
    /// Active network ports.
    pub active_ports: Vec<u16>,
    /// Cached portfolio statistics.
    pub stats_cache: Option<PortfolioStats>,
    /// Whether portfolio statistics computation is running.
    pub is_computing_stats: bool,
    /// Pending handover approval count.
    pub handover_count: usize,
    /// Whether system alert animation is active.
    pub bouncing_alert: bool,
    /// Pending file mutation approval requests.
    pub pending_file_changes: Vec<raios_runtime::daemon::state::FileChangeApproval>,
    /// Cursor index in pending file changes list.
    pub pending_change_cursor: usize,
    /// Sentinel monitored file statuses.
    pub sentinel_files: Vec<raios_runtime::daemon::state::SentinelFileStatus>,
    /// Watched memory file mtimes for change detection.
    pub memory_watch: std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    /// Whether memory refresh is requested.
    pub memory_refresh_pending: bool,
    /// Path to `graphify.py` script.
    pub graphify_script: Option<std::path::PathBuf>,
    /// Modal dialog data for handover approval.
    pub handover_modal: Option<(String, String)>,
}

/// State tracking setup wizard execution, inputs, and step progression.
#[derive(Debug)]
pub struct WizardState {
    /// Active wizard step.
    pub step: raios_surface_tui::setup_wizard::WizardStep,
    /// Dev_Ops directory input.
    pub dev_ops: String,
    /// Master constitution path input.
    pub master: String,
    /// GitHub username input.
    pub github: String,
    /// Vault path input.
    pub vault: String,
    /// Custom system name for constitution generation.
    pub system_name: String,
    /// Claude agent identity name for constitution generation.
    pub claude_name: String,
    /// Codex agent identity name for constitution generation.
    pub codex_name: String,
    /// OpenCode agent identity name for constitution generation.
    pub opencode_name: String,
    /// Antigravity agent identity name for constitution generation.
    pub antigravity_name: String,
    /// Preferred communication language & style.
    pub communication_lang: String,
    /// Selected input field cursor index.
    pub field_cursor: usize,
    /// Whether field text input is active.
    pub editing: bool,
    /// Text input buffer.
    pub input: String,
    /// Discovered agent status report.
    pub agent_status: Option<raios_surface_tui::setup_wizard::AgentStatus>,
    /// Executed setup action logs.
    pub action_log: Vec<raios_surface_tui::setup_wizard::WizardAction>,
    /// Skip Claude Code configuration.
    pub skip_claude: bool,
    /// Skip OpenCode configuration.
    pub skip_opencode: bool,
    /// Skip Antigravity configuration.
    pub skip_antigravity: bool,
    /// Whether setup installation is running.
    pub running: bool,
    /// Agent wrapper configuration choice index.
    pub agent_wrapper_choice: usize,
}

impl Default for WizardState {
    fn default() -> Self {
        let defaults = raios_surface_tui::setup_wizard::ConstitutionParams::default();
        Self {
            step: raios_surface_tui::setup_wizard::WizardStep::default(),
            dev_ops: String::new(),
            master: String::new(),
            github: String::new(),
            vault: String::new(),
            system_name: defaults.system_name,
            claude_name: defaults.claude_name,
            codex_name: defaults.codex_name,
            opencode_name: defaults.opencode_name,
            antigravity_name: defaults.antigravity_name,
            communication_lang: defaults.communication_lang,
            field_cursor: 0,
            editing: false,
            input: String::new(),
            agent_status: None,
            action_log: Vec::new(),
            skip_claude: false,
            skip_opencode: false,
            skip_antigravity: false,
            running: false,
            agent_wrapper_choice: 0,
        }
    }
}

impl WizardState {
    /// Builds `ConstitutionParams` from current wizard input state.
    pub fn to_constitution_params(&self) -> raios_surface_tui::setup_wizard::ConstitutionParams {
        raios_surface_tui::setup_wizard::ConstitutionParams {
            github_user: self.github.clone(),
            dev_ops_path: self.dev_ops.clone(),
            system_name: self.system_name.clone(),
            claude_name: self.claude_name.clone(),
            codex_name: self.codex_name.clone(),
            opencode_name: self.opencode_name.clone(),
            antigravity_name: self.antigravity_name.clone(),
            communication_lang: self.communication_lang.clone(),
        }
    }
}

/// Application state for project requirements and initial configuration.
#[derive(Debug, Default)]
pub struct SetupState {
    /// Setup input fields.
    pub fields: Vec<SetupField>,
    /// Field cursor index.
    pub cursor: usize,
    /// Editing flag.
    pub editing: bool,
    /// Input buffer.
    pub input: String,
    /// Status message string.
    pub status: Option<String>,
    /// Project requirements list.
    pub requirements: Vec<Requirement>,
}

/// Application state tracking active agents, skills, and policy rules.
#[derive(Debug, Default)]
pub struct InventoryState {
    /// Discovered agent harnesses.
    pub agents: Vec<AgentInfo>,
    /// Discovered skills.
    pub skills: Vec<SkillInfo>,
    /// Master constitution files.
    pub master_files: Vec<FileEntry>,
    /// Agent config files.
    pub agent_files: Vec<FileEntry>,
    /// Policy configuration files.
    pub policy_files: Vec<FileEntry>,
    /// MemPalace data files.
    pub mempalace_files: Vec<FileEntry>,
    /// Agent rule groups.
    pub agent_rule_groups: Vec<AgentRuleGroup>,
}

/// General UI layout, sub-panel focus, command palette, and modal dialog state.
#[derive(Debug, Default)]
pub struct UIState {
    /// Selected index in left-hand route navigation tab bar.
    pub menu_cursor: usize,
    /// Whether right-side detail panel has keyboard focus.
    pub right_panel_focus: bool,
    /// Selected index in right panel file list.
    pub right_file_cursor: usize,
    /// Scroll position of right detail panel.
    pub right_panel_scroll: usize,
    /// Whether command palette input mode is active.
    pub command_mode: bool,
    /// Command palette input text buffer.
    pub command_buf: String,
    /// Command palette item selection index.
    pub palette_cursor: usize,
    /// Whether agent launcher modal dialog is shown.
    pub show_launcher: bool,
    /// Agent selection index in launcher modal.
    pub launcher_cursor: usize,
    /// Initial prompt input in launcher modal.
    pub launcher_input: String,
}

/// Application state holding event timeline activities and log entries.
#[derive(Debug, Default)]
pub struct TimelineState {
    /// Activity feed items.
    pub activities: Vec<Activity>,
    /// Log output records.
    pub logs: Vec<LogEntry>,
}

// ─── Project State ────────────────────────────────────────────────────────────

/// Application state for project list management, detail views, and Git integration.
#[derive(Debug, Default)]
pub struct ProjectState {
    /// Workspace entity projects.
    pub list: Vec<raios_core::entities::EntityProject>,
    /// Recent project history.
    pub recent: Vec<raios_runtime::filebrowser::RecentProject>,
    /// Selected project list cursor index.
    pub cursor: usize,
    /// Active project sorting mode.
    pub sort: SortMode,
    /// Whether project list panel has focus.
    pub panel_focus: bool,
    /// Currently opened active project entity.
    pub active: Option<raios_core::entities::EntityProject>,
    /// Loaded lines from project's `memory.md`.
    pub memory_lines: Vec<String>,
    /// Scroll position for memory view.
    pub memory_scroll: u16,
    /// Git commit log output lines.
    pub git_log: Vec<String>,
    /// Graphify architecture report lines.
    pub graph_report_lines: Vec<String>,
    /// Scroll position for graph report view.
    pub graph_report_scroll: u16,
    /// Git diff output lines.
    pub git_diff_lines: Vec<String>,
    /// Scroll position for git diff view.
    pub git_diff_scroll: u16,
}

// ─── Editor State ─────────────────────────────────────────────────────────────

/// Application state for in-memory file viewing and editing within the TUI.
#[derive(Debug, Default)]
pub struct EditorState {
    /// Metadata of the file currently opened in editor.
    pub active_file: Option<raios_runtime::filebrowser::FileEntry>,
    /// Document line content buffer.
    pub lines: Vec<String>,
    /// Viewport scroll position.
    pub scroll: u16,
    /// Embedded text editor instance.
    pub editor: raios_surface_tui::app::Editor,
    /// Status message after file save operation.
    pub save_msg: Option<String>,
    /// File mtime when opened for external change tracking.
    pub watched_mtime: Option<std::time::SystemTime>,
    /// Whether file was modified on disk outside the editor.
    pub changed_externally: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use raios_runtime::constitution::{ConstitutionSection, ProjectFileKind};
    use std::path::Path;

    fn section(
        title: &str,
        items: &[&str],
        children: Vec<ConstitutionSection>,
    ) -> ConstitutionSection {
        ConstitutionSection {
            level: 1,
            title: title.into(),
            line_start: 0,
            line_end: 0,
            items: items.iter().map(|s| s.to_string()).collect(),
            children,
        }
    }

    // ─── SortMode ────────────────────────────────────────────────────────────

    #[test]
    fn sort_mode_next_cycles_through_every_variant_back_to_name() {
        let mut mode = SortMode::Name;
        let mut seen = vec![mode.clone()];
        for _ in 0..4 {
            mode = mode.next();
            seen.push(mode.clone());
        }
        assert_eq!(
            seen,
            vec![
                SortMode::Name,
                SortMode::Grade,
                SortMode::GitDirty,
                SortMode::Category,
                SortMode::Status,
            ]
        );
        assert_eq!(mode.next(), SortMode::Name); // cycle closes
    }

    #[test]
    fn sort_mode_label_is_non_empty_for_every_variant() {
        for mode in [
            SortMode::Name,
            SortMode::Grade,
            SortMode::GitDirty,
            SortMode::Category,
            SortMode::Status,
        ] {
            assert!(!mode.label().is_empty());
        }
    }

    // ─── ConstitutionTarget ──────────────────────────────────────────────────

    #[test]
    fn constitution_target_global_reports_its_path_and_label() {
        let target = ConstitutionTarget::Global {
            path: PathBuf::from("/etc/AGENT_CONSTITUTION.md"),
        };
        assert_eq!(target.path(), Path::new("/etc/AGENT_CONSTITUTION.md"));
        assert_eq!(target.label(), "Global Constitution");
    }

    #[test]
    fn constitution_target_project_file_reports_its_path_and_kind_filename() {
        let target = ConstitutionTarget::ProjectFile {
            path: PathBuf::from("/proj/CLAUDE.md"),
            kind: ProjectFileKind::ClaudeMd,
        };
        assert_eq!(target.path(), Path::new("/proj/CLAUDE.md"));
        assert_eq!(
            target.label(),
            target.path().file_name().unwrap().to_str().unwrap()
        );
    }

    // ─── flatten_sections ────────────────────────────────────────────────────

    #[test]
    fn flatten_sections_of_empty_input_is_empty() {
        assert!(flatten_sections(&[]).is_empty());
    }

    #[test]
    fn flatten_sections_orders_section_then_its_items_then_children_and_their_items() {
        let sections = vec![section(
            "Top",
            &["item-a", "item-b"],
            vec![section("Child", &["child-item"], vec![])],
        )];

        let rows = flatten_sections(&sections);

        assert_eq!(
            rows,
            vec![
                OutlineRow::Section { idx: 0 },
                OutlineRow::Item {
                    idx: 0,
                    child_idx: None,
                    item_idx: 0
                },
                OutlineRow::Item {
                    idx: 0,
                    child_idx: None,
                    item_idx: 1
                },
                OutlineRow::Child {
                    idx: 0,
                    child_idx: 0
                },
                OutlineRow::Item {
                    idx: 0,
                    child_idx: Some(0),
                    item_idx: 0
                },
            ]
        );
    }

    #[test]
    fn flatten_sections_handles_multiple_top_level_sections() {
        let sections = vec![section("A", &[], vec![]), section("B", &["x"], vec![])];
        let rows = flatten_sections(&sections);
        assert_eq!(
            rows,
            vec![
                OutlineRow::Section { idx: 0 },
                OutlineRow::Section { idx: 1 },
                OutlineRow::Item {
                    idx: 1,
                    child_idx: None,
                    item_idx: 0
                },
            ]
        );
    }

    // ─── SetupField ──────────────────────────────────────────────────────────

    #[test]
    fn setup_field_new_starts_undetected_and_empty() {
        let field = SetupField::new("Dev Ops", "hint");
        assert_eq!(field.label, "Dev Ops");
        assert_eq!(field.value, "");
        assert!(!field.auto_detected);
    }

    #[test]
    fn setup_field_with_detected_some_marks_auto_detected() {
        let field =
            SetupField::new("Dev Ops", "hint").with_detected(Some(PathBuf::from("/home/user/dev")));
        assert_eq!(field.value, "/home/user/dev");
        assert!(field.auto_detected);
    }

    #[test]
    fn setup_field_with_detected_none_leaves_it_untouched() {
        let field = SetupField::new("Dev Ops", "hint").with_detected(None);
        assert_eq!(field.value, "");
        assert!(!field.auto_detected);
    }
}
