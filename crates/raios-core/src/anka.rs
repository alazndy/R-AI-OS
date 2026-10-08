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

/// What the cache's own coverage says about its readiness — computed from
/// [`AnkaIndexStatus::coverage`], never guessed from `last_indexed_at`, so a
/// harness-specific refresh cannot masquerade as a full one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnkaCacheState {
    /// Every harness has a coverage entry: the cache speaks for all of them.
    Ready,
    /// Some harness has no coverage entry (a harness-scoped refresh, or a legacy
    /// cache that predates coverage) — the cache speaks for a subset.
    Partial,
    /// Never indexed: no coverage and no records.
    Empty,
    /// The cache file exists but this build cannot read it (corrupt bytes or an
    /// unknown schema version). Recall fails safe against it instead of guessing.
    Incompatible,
}

impl AnkaCacheState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnkaCacheState::Ready => "ready",
            AnkaCacheState::Partial => "partial",
            AnkaCacheState::Empty => "empty",
            AnkaCacheState::Incompatible => "incompatible",
        }
    }
}

/// One harness's slice of the last refresh, persisted in the cache envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaHarnessCoverage {
    pub harness: AnkaHarness,
    /// Source files discovered for this harness.
    pub sources: usize,
    /// Records that survived exclusions and tombstones for this harness.
    pub records: usize,
    /// Refresh stamp of the run that wrote this entry; empty means unknown
    /// (reconstructed from a legacy cache, which carries no per-harness timing).
    pub indexed_at: String,
    /// True when the run that wrote this entry refreshed every harness. A
    /// harness-scoped run writes `false` for its own slice and preserves the
    /// other harnesses' entries verbatim, so a partial refresh is visible as one.
    pub full_refresh: bool,
    /// Entries dropped because a single history line exceeded the byte bound —
    /// surfaced as its own exclusion reason, distinct from malformed/empty.
    pub oversized: usize,
}

/// Whether the privacy policy is initialized — carried on every status so a
/// fail-closed recall surface is explainable without a second call. Never
/// fails: `initialized: false` is a state `status` reports, while the
/// diagnostic details belong to `policy-show`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaPolicySummary {
    /// False when `anka-policy` is absent or does not parse.
    pub initialized: bool,
    /// `keep` | `exclude`; `None` until initialized.
    pub home: Option<String>,
    /// Number of literal substring rules resolved from `anka-exclude`.
    pub exclude_rules: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaIndexStatus {
    pub state: AnkaCacheState,
    pub coverage: Vec<AnkaHarnessCoverage>,
    pub cache_path: PathBuf,
    pub indexed_sources: usize,
    pub indexed_records: usize,
    pub last_indexed_at: Option<String>,
    /// Privacy-policy state as seen by the same reader that saw this status.
    pub policy: AnkaPolicySummary,
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
