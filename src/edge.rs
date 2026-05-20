//! Edge Intelligence — Semantic Capability Router
//!
//! Embeds capability descriptions at startup (using the existing TF-IDF/fastembed
//! embedder) and routes natural-language queries to the nearest capability via
//! cosine similarity. Falls back to exact name match above the threshold.
use crate::cortex::embedder::{Embedding, Embedder};

const CONFIDENCE_THRESHOLD: f32 = 0.25;

pub struct EdgeRouter {
    /// (capability_name, description_embedding) pairs
    entries: Vec<(String, Embedding)>,
    embedder: Embedder,
}

impl EdgeRouter {
    /// Build the router from (name, description) pairs.
    /// Embedder init may download a model on first use with the `cortex` feature.
    pub fn new(descriptions: Vec<(String, String)>) -> Self {
        let embedder = Embedder::init().unwrap_or_else(|_| {
            // Fallback embedder always succeeds
            Embedder::init().expect("even fallback embedder failed")
        });

        let entries = descriptions
            .into_iter()
            .filter_map(|(name, desc)| {
                let emb = embedder.embed_one(&desc).ok()?;
                Some((name, emb))
            })
            .collect();

        Self { embedder, entries }
    }

    /// Find the best matching capability name for `query`.
    /// Returns `None` if no entry exceeds the confidence threshold.
    pub fn route(&self, query: &str) -> Option<&str> {
        // 1. Exact name match (always wins)
        if let Some((name, _)) = self.entries.iter().find(|(n, _)| n == query) {
            return Some(name.as_str());
        }

        // 2. Semantic match
        let query_emb = self.embedder.embed_one(query).ok()?;
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
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>().clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> EdgeRouter {
        let descriptions = vec![
            ("health_check".to_string(), "Run health scan on a project".to_string()),
            ("search_codebase".to_string(), "Search across all indexed files semantically".to_string()),
            ("run_sentinel".to_string(), "Run compile and lint checks on project".to_string()),
            ("list_projects".to_string(), "List all known projects in workspace".to_string()),
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
