use std::path::PathBuf;

use raios_core::requirements::Requirement;
use raios_runtime::discovery::{AgentInfo, SkillInfo};
use raios_runtime::filebrowser::{AgentRuleGroup, FileEntry, RecentProject};
use raios_runtime::indexer::{ProjectIndex, SearchResult};

// ─── Extension State ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExtCmdInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ExtConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub description: String,
    pub value: String,
    pub masked: bool,
}

#[derive(Debug, Clone)]
pub struct ExtServiceStatus {
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
    pub commands: Vec<ExtCmdInfo>,
    pub config_schema: Vec<ExtConfigField>,
    pub services: Vec<String>,
    pub service_statuses: Vec<ExtServiceStatus>,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub enum ExtFocus {
    #[default]
    Commands,
    Config,
}

#[derive(Debug, Default)]
pub struct ExtState {
    pub extensions: Vec<ExtensionInfo>,
    pub ext_cursor: usize,
    pub focus: ExtFocus,
    pub cmd_cursor: usize,
    pub cfg_cursor: usize,
    pub editing: bool,
    pub input: String,
    pub status: Option<String>,
    pub loaded: bool,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Booting,
    Setup,
    Dashboard,
    FileView,
    FileEdit,
    ProjectDetail,
    HealthView,
    Search,
    MemPalaceView,
    GraphReport,
    GitDiffView,
    HelpView,
}

// ─── Background messages ──────────────────────────────────────────────────────

pub enum BgMsg {
    BootResult {
        name: String,
        pass: bool,
        done: bool,
    },
    TransitionToSetup,
    TransitionToDashboard,
    RecentProjects(Vec<RecentProject>),
    Agents(Vec<AgentInfo>),
    Skills(Vec<SkillInfo>),
    MasterFiles(Vec<FileEntry>),
    AgentFiles(Vec<FileEntry>),
    PolicyFiles(Vec<FileEntry>),
    MemPalaceFiles(Vec<FileEntry>),
    SyncDone(String),
    SyncError(String),
    IndexReady(ProjectIndex),
    IndexError(String),
    AgentRuleGroups(Vec<AgentRuleGroup>),
    Projects(Vec<raios_core::entities::EntityProject>),
    ProjectOpened(raios_surface_tui::app::ProjectDetailData),
    HealthReport(Vec<raios_runtime::health::ProjectHealth>),
    BuildTestDepsResult {
        idx: usize,
        health: Box<raios_runtime::health::ProjectHealth>,
    },
    StateSync {
        projects: Vec<raios_core::entities::EntityProject>,
        health_reports: Vec<raios_runtime::health::ProjectHealth>,
        active_agents: Vec<raios_runtime::daemon::proxy::AgentProcess>,
        index_ready: bool,
        handover_count: u32,
        pending_file_changes: Vec<raios_runtime::daemon::state::FileChangeApproval>,
        sentinel_files: Vec<raios_runtime::daemon::state::SentinelFileStatus>,
    },
    SentinelUpdate {
        project: String,
        status: String,
        error_count: usize,
    },
    ActivityUpdate(Vec<Activity>),
    NewLog(LogEntry),
    MemPalaceBuilt(Vec<raios_core::mempalace::MemRoom>),
    Tasks(Vec<raios_runtime::tasks::Task>),
    VaultStatus(Vec<String>),
    ActivePorts(Vec<u16>),
    ControlEvent(raios_contracts::Event),
    AiAuditReport(raios_runtime::system_scan::AiAuditReport),
    FileChanged(PathBuf),
    SearchResults(Vec<SearchResult>),
    HandoverApproved {
        target: String,
        instruction: String,
        count: u32,
    },
    HumanApprovalRequired {
        target: String,
        instruction: String,
        reason: String,
    },
    HumanApprovalResult {
        status: String,
    },
    FileChangeRequested {
        approval: raios_runtime::daemon::state::FileChangeApproval,
    },
    StatsReady(PortfolioStats),
    AgentStatusReady(AgentStatus),
    AgentStarted {
        agent_id: String,
        name: String,
        project_path: String,
    },
    AgentStopped {
        agent_id: String,
        name: String,
        final_status: String,
    },
    HealthDelta(Vec<raios_runtime::health::ProjectHealth>),
    WizardActions(Vec<WizardAction>),
    WizardDone,
    GitActionDone {
        project: String,
        action: String,
        ok: bool,
        message: String,
    },
    RemoteCommandResult {
        output: String,
    },
    ExtensionsLoaded(Vec<ExtensionInfo>),
    ExtCmdOutput {
        ext: String,
        cmd: String,
        line: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    pub timestamp: String,
    pub source: String, // "Git", "Agent", "System"
    pub message: String,
    pub level: &'static str, // "Info", "Warning", "Error"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub sender: String,
    pub content: String,
}

// ─── Setup field ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetupField {
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
    pub auto_detected: bool,
}

impl SetupField {
    pub fn new(label: &'static str, hint: &'static str) -> Self {
        Self {
            label,
            hint,
            value: String::new(),
            auto_detected: false,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SortMode {
    #[default]
    Name,
    Grade,
    GitDirty,
    Category,
    Status,
}

impl SortMode {
    pub fn next(&self) -> Self {
        match self {
            Self::Name => Self::Grade,
            Self::Grade => Self::GitDirty,
            Self::GitDirty => Self::Category,
            Self::Category => Self::Status,
            Self::Status => Self::Name,
        }
    }
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

#[derive(Debug, Clone, Default)]
pub struct PortfolioStats {
    pub total: usize,
    pub active: usize,
    pub archived: usize,
    pub dirty: usize,
    pub no_memory: usize,
    pub no_sigmap: usize,
    pub no_github: usize,
    pub grade_a: usize,
    pub grade_b: usize,
    pub grade_c: usize,
    pub grade_d: usize,
    pub top_dirty_category: String,
}

// ─── Constitution State ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ConstitutionTarget {
    Global {
        path: PathBuf,
    },
    ProjectFile {
        path: PathBuf,
        kind: raios_runtime::constitution::ProjectFileKind,
    },
}

impl ConstitutionTarget {
    pub fn path(&self) -> &std::path::Path {
        match self {
            ConstitutionTarget::Global { path } => path,
            ConstitutionTarget::ProjectFile { path, .. } => path,
        }
    }

    pub fn label(&self) -> String {
        match self {
            ConstitutionTarget::Global { .. } => "Global Constitution".to_string(),
            ConstitutionTarget::ProjectFile { kind, .. } => kind.filename().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineRow {
    Section {
        idx: usize,
    },
    Child {
        idx: usize,
        child_idx: usize,
    },
    Item {
        idx: usize,
        child_idx: Option<usize>,
        item_idx: usize,
    },
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreatorStep {
    #[default]
    ChooseTarget,
    Notes,
    Preview,
    /// Extra are-you-sure gate before writing to the global constitution —
    /// the single file every agent reads. Only reached when target_is_global.
    ConfirmGlobal,
}

#[derive(Debug, Default, Clone)]
pub struct CreatorState {
    pub active: bool,
    pub target_is_global: bool,
    pub step: CreatorStep,
    pub notes_input: String,
}

#[derive(Debug, Clone)]
pub struct PendingConstitutionSave {
    pub path: PathBuf,
    pub new_content: String,
    pub diff_lines: Vec<String>,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Default)]
pub struct ConstitutionState {
    pub tabs: Vec<ConstitutionTarget>,
    pub active_tab: usize,
    pub sections: Vec<raios_runtime::constitution::ConstitutionSection>,
    pub rows: Vec<OutlineRow>,
    pub outline_cursor: usize,
    pub item_editing: bool,
    pub item_input: String,
    pub pending_save: Option<PendingConstitutionSave>,
    pub creator: CreatorState,
}

// ─── Search State ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub cursor: usize,
    pub index: Option<ProjectIndex>,
    pub is_indexing: bool,
    pub status: Option<String>,
}

// ─── Mempalace State ─────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct MempalaceState {
    pub rooms: Vec<raios_core::mempalace::MemRoom>,
    pub room_cursor: usize,
    pub proj_cursor: Option<usize>,
    pub expanded: Vec<bool>,
    pub filter: String,
    pub is_building: bool,
    pub files: Vec<FileEntry>,
}

// ─── Health State ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct HealthState {
    pub report: Vec<raios_runtime::health::ProjectHealth>,
    pub cursor: usize,
    pub is_checking: bool,
    pub compliance: Option<raios_runtime::compliance::ComplianceReport>,
    pub is_fixing: bool,
    pub fix_status: Option<String>,
}

// ─── Task State ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TaskState {
    pub list: Vec<raios_runtime::tasks::Task>,
    pub cursor: usize,
}

// ─── System State ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SystemState {
    pub report: Option<raios_runtime::system_scan::AiAuditReport>,
    pub is_scanning: bool,
    pub boot_results: Vec<(String, bool)>,
    pub active_agents: Vec<raios_runtime::daemon::proxy::AgentProcess>,
    pub selected_agent_idx: usize,
    pub is_syncing: bool,
    pub sync_status: Option<String>,
    pub vault_projects: Vec<String>,
    pub active_ports: Vec<u16>,
    pub stats_cache: Option<PortfolioStats>,
    pub is_computing_stats: bool,
    pub handover_count: usize,
    pub bouncing_alert: bool,
    pub pending_file_changes: Vec<raios_runtime::daemon::state::FileChangeApproval>,
    pub pending_change_cursor: usize,
    pub sentinel_files: Vec<raios_runtime::daemon::state::SentinelFileStatus>,
    pub memory_watch: std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    pub memory_refresh_pending: bool,
    pub graphify_script: Option<std::path::PathBuf>,
    pub handover_modal: Option<(String, String)>,
}

// ─── Setup Wizard State ──────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct WizardState {
    pub step: raios_surface_tui::setup_wizard::WizardStep,
    pub dev_ops: String,
    pub master: String,
    pub github: String,
    pub vault: String,
    pub field_cursor: usize,
    pub editing: bool,
    pub input: String,
    pub agent_status: Option<raios_surface_tui::setup_wizard::AgentStatus>,
    pub action_log: Vec<raios_surface_tui::setup_wizard::WizardAction>,
    pub skip_claude: bool,
    pub skip_opencode: bool,
    pub skip_antigravity: bool,
    pub running: bool,
    /// 0 = install for all agents, 1 = skip
    pub agent_wrapper_choice: usize,
}

#[derive(Debug, Default)]
pub struct SetupState {
    pub fields: Vec<SetupField>,
    pub cursor: usize,
    pub editing: bool,
    pub input: String,
    pub status: Option<String>,
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Default)]
pub struct InventoryState {
    pub agents: Vec<AgentInfo>,
    pub skills: Vec<SkillInfo>,
    pub master_files: Vec<FileEntry>,
    pub agent_files: Vec<FileEntry>,
    pub policy_files: Vec<FileEntry>,
    pub mempalace_files: Vec<FileEntry>,
    pub agent_rule_groups: Vec<AgentRuleGroup>,
}

#[derive(Debug, Default)]
pub struct UIState {
    pub menu_cursor: usize,
    pub right_panel_focus: bool,
    pub right_file_cursor: usize,
    pub right_panel_scroll: usize,
    pub command_mode: bool,
    pub command_buf: String,
    pub palette_cursor: usize,
    pub show_launcher: bool,
    pub launcher_cursor: usize,
    pub launcher_input: String,
}

#[derive(Debug, Default)]
pub struct TimelineState {
    pub activities: Vec<Activity>,
    pub logs: Vec<LogEntry>,
}

// ─── Project State ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ProjectState {
    pub list: Vec<raios_core::entities::EntityProject>,
    pub recent: Vec<raios_runtime::filebrowser::RecentProject>,
    pub cursor: usize,
    pub sort: SortMode,
    pub panel_focus: bool,
    pub active: Option<raios_core::entities::EntityProject>,
    pub memory_lines: Vec<String>,
    pub memory_scroll: u16,
    pub git_log: Vec<String>,
    pub graph_report_lines: Vec<String>,
    pub graph_report_scroll: u16,
    pub git_diff_lines: Vec<String>,
    pub git_diff_scroll: u16,
}

// ─── Editor State ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct EditorState {
    pub active_file: Option<raios_runtime::filebrowser::FileEntry>,
    pub lines: Vec<String>,
    pub scroll: u16,
    pub editor: raios_surface_tui::app::Editor,
    pub save_msg: Option<String>,
    pub watched_mtime: Option<std::time::SystemTime>,
    pub changed_externally: bool,
}
