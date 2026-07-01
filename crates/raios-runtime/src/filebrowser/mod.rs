mod agents;
mod files;
mod git;

pub use agents::{discover_all_agent_rules, AgentRuleGroup};
pub use files::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, get_policy_files, load_file_content, save_file_content,
};
pub use git::{
    get_git_log, git_commit, git_get_remote_url, git_is_dirty, git_push, load_recent_projects,
    GitCommitResult, RecentProject,
};

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub read_only: bool,
    pub exists: bool,
}

impl FileEntry {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let exists = path.exists();
        Self {
            name: name.into(),
            path,
            read_only: false,
            exists,
        }
    }

    pub fn readonly(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn exists(&self) -> bool {
        self.exists
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}
