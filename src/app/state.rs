use std::path::PathBuf;

use crate::discovery::{AgentInfo, SkillInfo};
use crate::filebrowser::{AgentRuleGroup, FileEntry, RecentProject};
use crate::indexer::{ProjectIndex, SearchResult};
use crate::requirements::Requirement;

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
    Projects(Vec<crate::entities::EntityProject>),
    ProjectOpened {
        memory: Vec<String>,
        git_log: Vec<String>,
    },
    HealthReport(Vec<crate::health::ProjectHealth>),
    StateSync {
        projects: Vec<crate::entities::EntityProject>,
        health_reports: Vec<crate::health::ProjectHealth>,
        active_agents: Vec<crate::daemon::proxy::AgentProcess>,
        index_ready: bool,
        handover_count: u32,
        pending_file_changes: Vec<crate::daemon::state::FileChangeApproval>,
        sentinel_files: Vec<crate::daemon::state::SentinelFileStatus>,
    },
    SentinelUpdate {
        project: String,
        status: String,
        error_count: usize,
    },
    #[allow(dead_code)]
    ActivityUpdate(Vec<Activity>),
    #[allow(dead_code)]
    NewLog(LogEntry),
    MemPalaceBuilt(Vec<crate::mempalace::MemRoom>),
    Tasks(Vec<crate::tasks::Task>),
    VaultStatus(Vec<String>),
    ActivePorts(Vec<u16>),
    AiAuditReport(crate::system_scan::AiAuditReport),
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
        approval: crate::daemon::state::FileChangeApproval,
    },
    StatsReady(PortfolioStats),
    AgentStatusReady(AgentStatus),
    WizardActions(Vec<WizardAction>),
    WizardDone,
    GitActionDone {
        project: String,
        action: String,
        ok: bool,
        message: String,
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

pub use crate::setup_wizard::{AgentStatus, WizardAction, WizardStep};

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

// ─── Rule categories (hardcoded constitution) ────────────────────────────────

#[derive(Debug, Clone)]
pub struct RuleCategory {
    pub title: &'static str,
    pub rules: Vec<&'static str>,
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
    pub rooms: Vec<crate::mempalace::MemRoom>,
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
    pub report: Vec<crate::health::ProjectHealth>,
    pub cursor: usize,
    pub is_checking: bool,
    pub compliance: Option<crate::compliance::ComplianceReport>,
    pub is_fixing: bool,
    pub fix_status: Option<String>,
}

// ─── Task State ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TaskState {
    pub list: Vec<crate::tasks::Task>,
    pub cursor: usize,
}

// ─── System State ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SystemState {
    pub report: Option<crate::system_scan::AiAuditReport>,
    pub is_scanning: bool,
    pub boot_results: Vec<(String, bool)>,
    pub active_agents: Vec<crate::daemon::proxy::AgentProcess>,
    pub selected_agent_idx: usize,
    pub is_syncing: bool,
    pub sync_status: Option<String>,
    pub vault_projects: Vec<String>,
    pub active_ports: Vec<u16>,
    pub stats_cache: Option<PortfolioStats>,
    pub is_computing_stats: bool,
    pub handover_count: usize,
    pub bouncing_alert: bool,
    pub pending_file_changes: Vec<crate::daemon::state::FileChangeApproval>,
    pub pending_change_cursor: usize,
    pub sentinel_files: Vec<crate::daemon::state::SentinelFileStatus>,
    pub memory_watch: std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    pub memory_refresh_pending: bool,
    pub graphify_script: Option<std::path::PathBuf>,
    pub handover_modal: Option<(String, String)>,
}

// ─── Setup Wizard State ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WizardState {
    pub step: crate::setup_wizard::WizardStep,
    pub dev_ops: String,
    pub master: String,
    pub github: String,
    pub vault: String,
    pub field_cursor: usize,
    pub editing: bool,
    pub input: String,
    pub agent_status: Option<crate::setup_wizard::AgentStatus>,
    pub action_log: Vec<crate::setup_wizard::WizardAction>,
    pub skip_claude: bool,
    pub skip_gemini: bool,
    pub skip_antigravity: bool,
    pub running: bool,
}

impl Default for WizardState {
    fn default() -> Self {
        Self {
            step: crate::setup_wizard::WizardStep::default(),
            dev_ops: String::new(),
            master: String::new(),
            github: String::new(),
            vault: String::new(),
            field_cursor: 0,
            editing: false,
            input: String::new(),
            agent_status: None,
            action_log: Vec::new(),
            skip_claude: false,
            skip_gemini: false,
            skip_antigravity: false,
            running: false,
        }
    }
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
    pub system_rules: Vec<RuleCategory>,
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
    pub list: Vec<crate::entities::EntityProject>,
    pub recent: Vec<crate::filebrowser::RecentProject>,
    pub cursor: usize,
    pub sort: SortMode,
    pub panel_focus: bool,
    pub active: Option<crate::entities::EntityProject>,
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
    pub active_file: Option<crate::filebrowser::FileEntry>,
    pub lines: Vec<String>,
    pub scroll: u16,
    pub editor: crate::app::Editor,
    pub save_msg: Option<String>,
    pub watched_mtime: Option<std::time::SystemTime>,
    pub changed_externally: bool,
}

pub fn system_rules() -> Vec<RuleCategory> {
    vec![
        RuleCategory {
            title: "Core Principles",
            rules: vec![
                "İş arkadaşı tavrı, net ve direkt iletişim",
                "Kod: İngilizce, İletişim: Türkçe",
                "Güvenlik ve performans odaklı pair-programming",
            ],
        },
        RuleCategory {
            title: "Coding Standards",
            rules: vec![
                "pnpm > npm/yarn. Python: uv/pip",
                "Önce amaç ve skeleton, sonra component-by-component",
                "Fonksiyonel yazım, hata yönetimi zorunlu",
            ],
        },
        RuleCategory {
            title: "Mandatory Skills",
            rules: vec![
                "prompt-master: Her prompt öncesi zorunlu",
                "graphify: Codebase girişi ve analizde zorunlu",
                "verify-ai-os: Session başı ve tutarsızlıkta zorunlu",
            ],
        },
        RuleCategory {
            title: "Security",
            rules: vec![
                "API key asla client-side'da olmaz",
                "RLS (Row Level Security) day 0'dan zorunlu",
                "Secrets Manager kullanımı (Production)",
            ],
        },
    ]
}
