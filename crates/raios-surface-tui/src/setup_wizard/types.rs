use std::process::Command;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum WizardStep {
    #[default]
    Welcome,
    Workspace,
    Constitution,
    Claude,
    Codex,
    OpenCode,
    Skills,
    AgentWrapper,
    Initialize,
    Done,
}

impl WizardStep {
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

    pub fn total() -> usize {
        9
    }

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

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub claude_installed: bool,
    pub claude_version: String,
    pub codex_installed: bool,
    pub codex_version: String,
    pub opencode_installed: bool,
    pub opencode_version: String,
    pub agy_installed: bool,
    pub agy_version: String,
    pub git_installed: bool,
    pub git_version: String,
    pub gh_installed: bool,
    pub gh_version: String,
}

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

#[derive(Debug, Clone)]
pub struct WizardAction {
    pub desc: String,
    pub ok: bool,
    pub skipped: bool,
}

impl WizardAction {
    pub(super) fn ok(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: true, skipped: false }
    }
    pub(super) fn fail(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: false, skipped: false }
    }
    pub(super) fn skip(desc: impl Into<String>) -> Self {
        Self { desc: desc.into(), ok: true, skipped: true }
    }
}
