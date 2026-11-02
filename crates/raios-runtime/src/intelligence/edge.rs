//! Edge Intelligence — Semantic Capability Router
//!
//! Embeds capability descriptions at startup (using the existing TF-IDF/fastembed
//! embedder) and routes natural-language queries to the nearest capability via
//! cosine similarity. Falls back to exact name match above the threshold.
use crate::cortex::embedder::{Embedder, Embedding};

const CONFIDENCE_THRESHOLD: f32 = 0.25;

pub struct EdgeRouter {
    /// Every capability name, for the exact-match step. Kept separately from
    /// `entries` so exact matching still works when no embedder is available.
    names: Vec<String>,
    /// (capability_name, description_embedding) pairs
    entries: Vec<(String, Embedding)>,
    /// `None` when the embedder could not be initialised (e.g. the model is
    /// not cached and the machine is offline). Routing then degrades to
    /// exact-name matching instead of aborting the process.
    embedder: Option<Embedder>,
}

impl EdgeRouter {
    /// Build the router from (name, description) pairs.
    /// Embedder init may download a model on first use with the `cortex` feature;
    /// if it fails, semantic routing is disabled and only exact names match.
    pub fn new(descriptions: Vec<(String, String)>) -> Self {
        let embedder = match Embedder::init() {
            Ok(embedder) => Some(embedder),
            Err(e) => {
                eprintln!("[edge] semantic routing disabled, exact names only: {e}");
                None
            }
        };
        Self::with_embedder(descriptions, embedder)
    }

    fn with_embedder(descriptions: Vec<(String, String)>, embedder: Option<Embedder>) -> Self {
        let names = descriptions.iter().map(|(name, _)| name.clone()).collect();
        let entries = match &embedder {
            Some(embedder) => descriptions
                .into_iter()
                .filter_map(|(name, desc)| {
                    let emb = embedder.embed_one(&desc).ok()?;
                    Some((name, emb))
                })
                .collect(),
            None => Vec::new(),
        };

        Self {
            names,
            embedder,
            entries,
        }
    }

    /// Find the best matching capability name for `query`.
    /// Returns `None` if no entry exceeds the confidence threshold.
    pub fn route(&self, query: &str) -> Option<&str> {
        // 1. Exact name match (always wins)
        if let Some(name) = self.names.iter().find(|n| n.as_str() == query) {
            return Some(name.as_str());
        }

        // 2. Semantic match (unavailable without an embedder)
        let query_emb = self.embedder.as_ref()?.embed_one(query).ok()?;
        let (best_name, best_score) = self
            .entries
            .iter()
            .map(|(name, emb)| {
                let score = cosine_similarity(&query_emb, emb);
                (name.as_str(), score)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;

        if best_score >= CONFIDENCE_THRESHOLD {
            Some(best_name)
        } else {
            None
        }
    }
}

fn cosine_similarity(a: &Embedding, b: &Embedding) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x * y)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_embedder_only_exact_names_route() {
        let router = EdgeRouter::with_embedder(
            vec![(
                "health_check".to_string(),
                "Scan and report project health".to_string(),
            )],
            None,
        );
        assert_eq!(router.route("health_check"), Some("health_check"));
        assert_eq!(router.route("how healthy is my project"), None);
    }

    fn make_router() -> EdgeRouter {
        let descriptions = vec![
            (
                "health_check".to_string(),
                "Scan and report project health status, missing docs, dirty files".to_string(),
            ),
            (
                "search_codebase".to_string(),
                "Semantic search and find code snippets, files, and symbols across the workspace or codebase".to_string(),
            ),
            (
                "run_sentinel".to_string(),
                "Compile, build, lint, and run static analysis checks for code quality errors".to_string(),
            ),
            (
                "list_projects".to_string(),
                "List and enumerate all known projects in the workspace portfolio".to_string(),
            ),
        ];
        EdgeRouter::new(descriptions)
    }

    #[test]
    fn routes_health_query_to_health_check() {
        let router = make_router();
        let result = router.route("check project health status");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "health_check");
    }

    #[test]
    fn routes_search_query_to_search_codebase() {
        let router = make_router();
        let result = router.route("search codebase semantically for auth");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "search_codebase");
    }

    #[test]
    fn low_confidence_query_returns_none() {
        let router = make_router();
        let _ = router.route("xyzzy flibbertigibbet nonsense");
    }

    #[test]
    fn exact_name_always_routes() {
        let router = make_router();
        assert_eq!(router.route("health_check"), Some("health_check"));
    }
}
