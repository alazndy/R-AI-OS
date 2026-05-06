use std::path::PathBuf;

use crate::discovery::{AgentInfo, SkillInfo};
use crate::indexer::{ProjectIndex, SearchResult};
use crate::filebrowser::{AgentRuleGroup, FileEntry, RecentProject};

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
    BootResult { name: String, pass: bool, done: bool },
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
    ProjectOpened { memory: Vec<String>, git_log: Vec<String> },
    HealthReport(Vec<crate::health::ProjectHealth>),
    StateSync {
        projects: Vec<crate::entities::EntityProject>,
        health_reports: Vec<crate::health::ProjectHealth>,
        active_agents: Vec<crate::daemon::proxy::AgentProcess>,
        index_ready: bool,
        handover_count: u32,
        pending_file_changes: Vec<crate::daemon::state::FileChangeApproval>,
    },
    #[allow(dead_code)] ActivityUpdate(Vec<Activity>),
    #[allow(dead_code)] NewLog(LogEntry),
    MemPalaceBuilt(Vec<crate::mempalace::MemRoom>),
    Tasks(Vec<crate::tasks::Task>),
    VaultStatus(Vec<String>),
    ActivePorts(Vec<u16>),
    AiAuditReport(crate::system_scan::AiAuditReport),
    FileChanged(PathBuf),
    SearchResults(Vec<SearchResult>),
    HandoverApproved { target: String, instruction: String, count: u32 },
    HumanApprovalRequired { target: String, instruction: String, reason: String },
    HumanApprovalResult { status: String },
    FileChangeRequested {
        approval: crate::daemon::state::FileChangeApproval,
    },
    StatsReady(PortfolioStats),
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub timestamp: String,
    pub source: String, // "Git", "Agent", "System"
    pub message: String,
    pub level: &'static str, // "Info", "Warning", "Error"
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub sender: String,
    pub content: String,
}

// ─── Setup field ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SetupField {
    pub label: &'static str,
    pub hint:  &'static str,
    pub value: String,
    pub auto_detected: bool,
}

impl SetupField {
    pub fn new(label: &'static str, hint: &'static str) -> Self {
        Self { label, hint, value: String::new(), auto_detected: false }
    }
    pub fn with_detected(mut self, path: Option<PathBuf>) -> Self {
        if let Some(p) = path {
            self.value = p.to_string_lossy().into_owned();
            self.auto_detected = true;
        }
        self
    }
}

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
            Self::Name     => Self::Grade,
            Self::Grade    => Self::GitDirty,
            Self::GitDirty => Self::Category,
            Self::Category => Self::Status,
            Self::Status   => Self::Name,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Name     => "Name",
            Self::Grade    => "Grade",
            Self::GitDirty => "Dirty",
            Self::Category => "Category",
            Self::Status   => "Status",
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
    pub local_only: usize,
    pub grade_a: usize,
    pub grade_b: usize,
    pub grade_c: usize,
    pub grade_d: usize,
    pub top_dirty_category: String,
}

// ─── Rule categories (hardcoded constitution) ────────────────────────────────

#[derive(Clone)]
pub struct RuleCategory {
    pub title: &'static str,
    pub rules: Vec<&'static str>,
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
