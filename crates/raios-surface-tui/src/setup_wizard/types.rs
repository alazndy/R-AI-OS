//! Data types and environment detector for the setup wizard.

use std::process::Command;

/// Parameters for generating an interactive AGENT_CONSTITUTION.md file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstitutionParams {
    /// GitHub username of the workspace owner.
    pub github_user: String,
    /// Dev_Ops directory path.
    pub dev_ops_path: String,
    /// Overall system name (e.g., "raios").
    pub system_name: String,
    /// Claude agent identity name (e.g., "Claude").
    pub claude_name: String,
    /// Codex agent identity name (e.g., "Codex").
    pub codex_name: String,
    /// OpenCode agent identity name (e.g., "OpenCode").
    pub opencode_name: String,
    /// Antigravity agent identity name (e.g., "Antigravity").
    pub antigravity_name: String,
    /// Primary communication language & chat style preference.
    pub communication_lang: String,
}

/// Neutral, non-personal defaults — every field here is what a stranger who
/// never touches a field gets, so none of them may be a specific person's
/// persona/nickname or a non-English-default language choice. Plain agent
/// product names and a generic system name, not "<Agent> Kaira"/"k-ai-ra".
impl Default for ConstitutionParams {
    fn default() -> Self {
        Self {
            github_user: String::new(),
            dev_ops_path: String::from("~/dev"),
            system_name: String::from("raios"),
            claude_name: String::from("Claude"),
            codex_name: String::from("Codex"),
            opencode_name: String::from("OpenCode"),
            antigravity_name: String::from("Antigravity"),
            communication_lang: String::from("English in chat and code."),
        }
    }
}

/// Steps in the initial setup wizard sequence.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum WizardStep {
    /// Welcome screen introducing the setup process.
    #[default]
    Welcome,
    /// Workspace root directory selection step.
    Workspace,
    /// Constitution and guidelines setup step.
    Constitution,
    /// Claude Code agent harness setup step.
    Claude,
    /// Codex agent harness setup step.
    Codex,
    /// OpenCode agent harness setup step.
    OpenCode,
    /// Skills and hooks installation step.
    Skills,
    /// Agent wrapper script installation step.
    AgentWrapper,
    /// Database and service initialization step.
    Initialize,
    /// Completion summary screen.
    Done,
}

impl WizardStep {
    /// Advances to the next step in the setup sequence.
    pub fn next(&self) -> Self {
        match self {
            Self::Welcome => Self::Workspace,
            Self::Workspace => Self::Constitution,
            Self::Constitution => Self::Claude,
            Self::Claude => Self::Codex,
            Self::Codex => Self::OpenCode,
            Self::OpenCode => Self::Skills,
            Self::Skills => Self::AgentWrapper,
            Self::AgentWrapper => Self::Initialize,
            Self::Initialize => Self::Done,
            Self::Done => Self::Done,
        }
    }

    /// Returns the 0-based index of the step.
    pub fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Workspace => 1,
            Self::Constitution => 2,
            Self::Claude => 3,
            Self::Codex => 4,
            Self::OpenCode => 5,
            Self::Skills => 6,
            Self::AgentWrapper => 7,
            Self::Initialize => 8,
            Self::Done => 9,
        }
    }

    /// Returns the total count of active configuration steps.
    pub fn total() -> usize {
        9
    }

    /// Returns the header title for the step.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Welcome => "WELCOME TO K-AI-RA",
            Self::Workspace => "WORKSPACE",
            Self::Constitution => "AGENT_CONSTITUTION.md",
            Self::Claude => "CLAUDE KAIRA",
            Self::Codex => "CODEX KAIRA",
            Self::OpenCode => "OPENCODE",
            Self::Skills => "SKILLS & HOOKS",
            Self::AgentWrapper => "AGENT WRAPPER",
            Self::Initialize => "INITIALIZE",
            Self::Done => "DONE",
        }
    }
}

/// System environment detection results for agent harnesses and tools.
#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    /// Whether `claude` is detected on system PATH.
    pub claude_installed: bool,
    /// Output version string for `claude`.
    pub claude_version: String,
    /// Whether `codex` is detected on system PATH.
    pub codex_installed: bool,
    /// Output version string for `codex`.
    pub codex_version: String,
    /// Whether `opencode` is detected on system PATH.
    pub opencode_installed: bool,
    /// Output version string for `opencode`.
    pub opencode_version: String,
    /// Whether `agy` binary is detected on system PATH.
    pub agy_installed: bool,
    /// Output version string for `agy`.
    pub agy_version: String,
    /// Whether `git` is detected on system PATH.
    pub git_installed: bool,
    /// Output version string for `git`.
    pub git_version: String,
    /// Whether `gh` GitHub CLI is detected on system PATH.
    pub gh_installed: bool,
    /// Output version string for `gh`.
    pub gh_version: String,
}

/// Detects installed agent harnesses and tools on the local system.
pub fn detect_agents() -> AgentStatus {
    let mut s = AgentStatus::default();

    if let Some((ok, v)) = run_version(&["claude", "--version"]) {
        s.claude_installed = ok;
        s.claude_version = v;
    }
    if let Some((ok, v)) = run_version(&["codex", "--version"]) {
        s.codex_installed = ok;
        s.codex_version = v;
    }
    if let Some((ok, v)) = run_version(&["opencode", "--version"]) {
        s.opencode_installed = ok;
        s.opencode_version = v;
    }
    s.agy_installed = raios_core::core::process::resolve_command_path("agy").is_some();
    if s.agy_installed {
        s.agy_version = "installed".to_string();
    }
    if let Some((ok, v)) = run_version(&["git", "--version"]) {
        s.git_installed = ok;
        s.git_version = v;
    }
    if let Some((ok, v)) = run_version(&["gh", "--version"]) {
        s.gh_installed = ok;
        s.gh_version = v.lines().next().unwrap_or("").to_string();
    }
    s
}

fn run_version(args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new(args[0]).args(&args[1..]).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let v = if v.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else {
        v
    };
    Some((
        out.status.success(),
        v.lines().next().unwrap_or("").to_string(),
    ))
}

/// Action execution result for a setup wizard step.
#[derive(Debug, Clone)]
pub struct WizardAction {
    /// Description of the action performed.
    pub desc: String,
    /// `true` if the action succeeded or was skipped cleanly.
    pub ok: bool,
    /// `true` if the action was skipped because pre-conditions were already met.
    pub skipped: bool,
}

impl WizardAction {
    pub(super) fn ok(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: true,
            skipped: false,
        }
    }
    pub(super) fn fail(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: false,
            skipped: false,
        }
    }
    pub(super) fn skip(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            ok: true,
            skipped: true,
        }
    }
}
