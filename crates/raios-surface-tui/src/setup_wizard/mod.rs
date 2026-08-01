//! Setup wizard for initial workspace configuration and agent environment detection.

mod exec;
mod templates;
/// Data types and environment detector for the setup wizard.
pub mod types;

pub use exec::{
    exec_agent_wrapper, exec_claude, exec_codex, exec_initialize, exec_master, exec_opencode,
    exec_skills, exec_workspace,
};
pub use templates::master_template;
pub use types::{detect_agents, AgentStatus, ConstitutionParams, WizardAction, WizardStep};
