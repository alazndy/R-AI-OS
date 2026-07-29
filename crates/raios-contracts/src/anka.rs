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

/// Status report summarizing the ANKA transcript indexer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaIndexStatusDto {
    /// Current indexer state (e.g., "ready", "indexing", "uninitialized").
    pub state: String,
    /// Local file path where the ANKA cache is persisted.
    pub cache_path: String,
    /// Number of distinct transcript source files indexed.
    pub indexed_sources: usize,
    /// Total number of transcript record entries indexed.
    pub indexed_records: usize,
    /// ISO-8601 UTC timestamp when indexing was last performed.
    pub last_indexed_at: Option<String>,
}
