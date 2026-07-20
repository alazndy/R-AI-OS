//! ANKA — Agent Narrative Knowledge Archive.
//!
//! This module defines the boundary for a rebuildable, read-only transcript
//! recall cache. It deliberately does not persist into `workspace.db`: curated
//! memory and control-plane state remain the authoritative R-AI-OS stores.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ANKA_CACHE_DIRECTORY: &str = "anka";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnkaHarness {
    Claude,
    Codex,
    Opencode,
    Antigravity,
}

impl AnkaHarness {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Opencode, Self::Antigravity];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaSourceRef {
    pub harness: AnkaHarness,
    pub project: String,
    pub session_id: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnkaConfidence {
    Exact,
    Lexical,
    Semantic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnkaHit {
    pub id: String,
    pub source: AnkaSourceRef,
    pub snippet: String,
    pub score: f64,
    pub confidence: AnkaConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaSearchQuery {
    pub text: String,
    pub project: Option<String>,
    pub harness: Option<AnkaHarness>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaIndexStatus {
    pub cache_path: PathBuf,
    pub indexed_sources: usize,
    pub indexed_records: usize,
    pub last_indexed_at: Option<String>,
}

pub trait AnkaRecallStore {
    fn status(&self) -> Result<AnkaIndexStatus>;
    fn search(&self, query: &AnkaSearchQuery) -> Result<Vec<AnkaHit>>;
    fn blame(&self, path: &str, limit: usize) -> Result<Vec<AnkaHit>>;
}

pub trait AnkaImporter {
    fn index(&self, harnesses: &[AnkaHarness]) -> Result<AnkaIndexStatus>;
}

pub fn default_cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("raios")
        .join(ANKA_CACHE_DIRECTORY)
}
