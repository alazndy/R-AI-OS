//! ANKA (Agent Narrative Knowledge Archive) DTO contracts.

use serde::{Deserialize, Serialize};

/// Stable transport contract for the ANKA transcript-recall search request.
///
/// ANKA results are historical evidence, not authoritative project memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaSearchRequestDto {
    /// Free-text query pattern to match against transcript records.
    pub query: String,
    /// Optional project key filter.
    pub project: Option<String>,
    /// Optional agent harness filter (e.g., "claude", "codex", "agy").
    pub harness: Option<String>,
    /// Maximum number of matching hits to return.
    pub limit: usize,
}

/// A single transcript search match hit returned by ANKA.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnkaHitDto {
    /// Unique identifier of the search hit.
    pub id: String,
    /// Agent harness that produced the transcript record.
    pub harness: String,
    /// Project identifier associated with the transcript.
    pub project: String,
    /// Session UUID of the agent run.
    pub session_id: String,
    /// ISO-8601 UTC timestamp when the event occurred.
    pub occurred_at: String,
    /// Snippet excerpt matching the search query.
    pub snippet: String,
    /// Numerical relevance score assigned to this hit.
    pub score: f64,
    /// Qualitative confidence rating of the match.
    pub confidence: String,
}

/// One harness's slice of the last refresh, as carried over the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaHarnessCoverageDto {
    /// Harness name: "claude", "codex", "opencode", "antigravity".
    pub harness: String,
    /// Source files discovered for this harness.
    pub sources: usize,
    /// Records retained for this harness after exclusions and tombstones.
    pub records: usize,
    /// ISO-8601 UTC stamp of the run that wrote this entry; empty means unknown.
    pub indexed_at: String,
    /// Whether the run that wrote this entry refreshed every harness.
    pub full_refresh: bool,
    /// Entries dropped for exceeding the per-line byte bound — its own exclusion reason.
    pub oversized: usize,
}

/// Privacy-policy state as reported alongside the status.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaPolicySummaryDto {
    /// False when `anka-policy` is absent or does not parse.
    pub initialized: bool,
    /// `keep` | `exclude`; `None` until initialized.
    pub home: Option<String>,
    /// Number of literal substring rules resolved from `anka-exclude`.
    pub exclude_rules: usize,
}

/// Status report summarizing the ANKA transcript indexer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaIndexStatusDto {
    /// Cache readiness derived from `coverage`:
    /// "ready" | "partial" | "empty" | "incompatible".
    pub state: String,
    /// Local file path where the ANKA cache is persisted.
    pub cache_path: String,
    /// Number of distinct transcript source files indexed.
    pub indexed_sources: usize,
    /// Total number of transcript record entries indexed.
    pub indexed_records: usize,
    /// ISO-8601 UTC timestamp when indexing was last performed.
    pub last_indexed_at: Option<String>,
    /// Per-harness coverage of the last refresh(es).
    pub coverage: Vec<AnkaHarnessCoverageDto>,
    /// Privacy-policy state; absent on payloads from older builds.
    #[serde(default)]
    pub policy: AnkaPolicySummaryDto,
}
