use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BumpType {
    Major,
    Minor,
    Patch,
}

impl BumpType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "patch" => Some(Self::Patch),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current: String,
    pub project_type: String,
    pub version_file: String,
    pub last_tag: Option<String>,
    pub commits_since_tag: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BumpResult {
    pub ok: bool,
    pub old_version: String,
    pub new_version: String,
    pub version_file: String,
    pub changelog_entry: String,
    pub message: String,
}
