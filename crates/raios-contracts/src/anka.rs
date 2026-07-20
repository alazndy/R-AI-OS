use serde::{Deserialize, Serialize};

/// Stable transport contract for the ANKA transcript-recall surface.
///
/// ANKA results are historical evidence, not authoritative project memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaSearchRequestDto {
    pub query: String,
    pub project: Option<String>,
    pub harness: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnkaHitDto {
    pub id: String,
    pub harness: String,
    pub project: String,
    pub session_id: String,
    pub occurred_at: String,
    pub snippet: String,
    pub score: f64,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnkaIndexStatusDto {
    pub state: String,
    pub cache_path: String,
    pub indexed_sources: usize,
    pub indexed_records: usize,
    pub last_indexed_at: Option<String>,
}
