//! Hybrid Search — Reciprocal Rank Fusion (RRF)
//!
//! Merges BM25 keyword results (`indexer::SearchResult`) with vector
//! semantic results (`cortex::store::VectorResult`) into a single ranked list.
//!
//! RRF formula:  score(d) = Σ  1 / (k + rank_i(d))
//!                           i
//! where k = 60 (standard constant), rank_i is 1-indexed position in list i.
//!
//! BM25 results are retrieved from the in-memory `ProjectIndex`.
//! Vector results are retrieved from the `Cortex` engine (blocking call).

use crate::cortex::store::VectorResult;
use crate::indexer::SearchResult as BM25Result;
use std::collections::HashMap;

/// Combined search result after RRF fusion.
#[derive(Debug, Clone)]
pub struct HybridResult {
    pub path: std::path::PathBuf,
    pub project: String,
    pub snippet: String,
    pub start_line: usize,
    /// RRF fusion score (higher = more relevant).
    pub rrf_score: f64,
    /// Original BM25 score if available.
    pub bm25_score: Option<f32>,
    /// Cosine similarity from vector search if available.
    pub vector_score: Option<f32>,
    /// How this result was found.
    pub source: ResultSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResultSource {
    BM25Only,
    VectorOnly,
    Hybrid,
}

impl ResultSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BM25Only => "keyword",
            Self::VectorOnly => "🧠 semantic",
            Self::Hybrid => "🧠 hybrid",
        }
    }
}

/// RRF constant (60 is the standard; higher → more rank-tolerant).
const K: f64 = 60.0;

/// Fuse BM25 and vector results using Reciprocal Rank Fusion.
///
/// - `bm25_results`: already-ranked BM25 results (index 0 = best).
/// - `vector_results`: already-ranked vector results (index 0 = best).
/// - `top_n`: how many results to return.
pub fn fuse(
    bm25_results: Vec<BM25Result>,
    vector_results: Vec<VectorResult>,
    top_n: usize,
) -> Vec<HybridResult> {
    // key: canonical path string
    let mut scores: HashMap<String, (f64, Option<f32>, Option<f32>, usize, String, String)> =
        HashMap::new();
    //                              rrf   bm25       vec      line   snippet  project

    // Incorporate BM25 rankings
    for (rank, r) in bm25_results.iter().enumerate() {
        let key = r.path.to_string_lossy().into_owned();
        let rrf = 1.0 / (K + (rank + 1) as f64);
        let entry = scores.entry(key).or_insert((
            0.0,
            None,
            None,
            r.line,
            r.snippet.clone(),
            r.project.clone(),
        ));
        entry.0 += rrf;
        entry.1 = Some(r.score);
    }

    // Incorporate vector rankings
    for (rank, r) in vector_results.iter().enumerate() {
        let key = r.path.clone();
        let rrf = 1.0 / (K + (rank + 1) as f64);
        // Derive project name from path (second-to-last component)
        let project = std::path::Path::new(&r.path)
            .components()
            .rev()
            .nth(1)
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("?")
            .to_string();
        let entry = scores.entry(key.clone()).or_insert((
            0.0,
            None,
            None,
            r.start_line,
            r.text.clone(),
            project.clone(),
        ));
        entry.0 += rrf;
        entry.2 = Some(r.score);
        // Prefer the vector snippet (often richer context)
        if entry.4.len() < r.text.len() {
            entry.4 = r.text.clone();
        }
        if entry.5.is_empty() {
            entry.5 = project;
        }
    }

    // Build sorted result list
    let mut results: Vec<HybridResult> = scores
        .into_iter()
        .map(
            |(path_str, (rrf, bm25, vec_score, line, snippet, project))| {
                let source = match (bm25.is_some(), vec_score.is_some()) {
                    (true, true) => ResultSource::Hybrid,
                    (true, false) => ResultSource::BM25Only,
                    (false, true) => ResultSource::VectorOnly,
                    (false, false) => ResultSource::BM25Only,
                };
                HybridResult {
                    path: std::path::PathBuf::from(&path_str),
                    project,
                    snippet: snippet.chars().take(200).collect(),
                    start_line: line,
                    rrf_score: rrf,
                    bm25_score: bm25,
                    vector_score: vec_score,
                    source,
                }
            },
        )
        .collect();

    results.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(top_n);
    results
}

/// Convert a `HybridResult` back to the `SearchResult` type used by the TUI,
/// so existing UI code works without modification.
impl From<HybridResult> for crate::indexer::SearchResult {
    fn from(h: HybridResult) -> Self {
        crate::indexer::SearchResult {
            path: h.path,
            project: h.project,
            snippet: h.snippet,
            score: h.rrf_score as f32,
            line: h.start_line,
        }
    }
}
