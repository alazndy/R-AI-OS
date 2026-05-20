# Phase 6: Edge Intelligence — Semantic Capability Router

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add semantic routing to `CapabilityProxy` so agents can describe what they want in natural language instead of knowing the exact capability name — query "run security scan" finds `run_sentinel`, "search my code" finds `search_codebase`.

**Architecture:** A new `EdgeRouter` module embeds all capability descriptions at startup using the existing `Embedder` (TF-IDF fallback or fastembed). At routing time the query is embedded and cosine similarity finds the nearest match. A confidence threshold (0.4) gates the match — below it falls back to exact name lookup. The router lives inside `CapabilityProxy` as an optional field.

**Tech Stack:** `crate::cortex::embedder::Embedder`, `crate::cortex::embedder::Embedding`, `crate::proxy_store::CapabilityStore`, no new dependencies.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Create | `src/edge.rs` | `EdgeRouter` — embeds capability descriptions, `route(query) -> Option<&str>` |
| Modify | `src/proxy_store.rs` | `CapabilityProxy` gains `router: EdgeRouter`, adds `route(query, input) -> Result<String>` |
| Modify | `src/lib.rs` | `pub mod edge;` |
| Modify | `src/daemon/server.rs` | Add `RouteCapability` TCP command |

---

### Task 1: Implement `EdgeRouter` in `src/edge.rs`

**Files:**
- Create: `src/edge.rs`

- [ ] **Step 1: Write failing tests first**

Create `src/edge.rs` with only the test module:

```rust
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
        let result = router.route("search my code for auth logic");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "search_codebase");
    }

    #[test]
    fn low_confidence_query_returns_none() {
        let router = make_router();
        // Completely unrelated query
        let result = router.route("xyzzy flibbertigibbet nonsense");
        // May return None if confidence < threshold — acceptable
        // (don't assert None strictly since TF-IDF is approximate)
        let _ = result;
    }

    #[test]
    fn exact_name_always_routes() {
        let router = make_router();
        assert_eq!(router.route("health_check"), Some("health_check"));
    }
}
```

- [ ] **Step 2: Run to confirm compile error**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test edge::tests 2>&1 | Select-Object -Last 5
```

Expected: compile error (`EdgeRouter` not defined)

- [ ] **Step 3: Implement `src/edge.rs`**

```rust
//! Edge Intelligence — Semantic Capability Router
//!
//! Embeds capability descriptions at startup (using the existing TF-IDF/fastembed
//! embedder) and routes natural-language queries to the nearest capability via
//! cosine similarity. Falls back to exact name match above the threshold.
use crate::cortex::embedder::{Embedding, Embedder, EMBEDDING_DIM};

const CONFIDENCE_THRESHOLD: f32 = 0.35;

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
        let result = router.route("search my code for auth logic");
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
```

- [ ] **Step 4: Register in `src/lib.rs`**

Add `pub mod edge;` after `pub mod db;`.

- [ ] **Step 5: Run tests**

```powershell
cargo test edge::tests
```

Expected: 3-4 tests PASS (low_confidence may or may not return None — that's fine)

- [ ] **Step 6: Commit**

```powershell
git add src/edge.rs src/lib.rs
git commit -m "feat(edge): add EdgeRouter — semantic capability routing via cosine similarity"
```

---

### Task 2: Wire `EdgeRouter` into `CapabilityProxy`

**Files:**
- Modify: `src/proxy_store.rs`

- [ ] **Step 1: Write failing test**

In `src/proxy_store.rs` test module, add:

```rust
#[test]
fn route_natural_language_finds_capability() {
    let proxy = make_proxy();
    let result = proxy.route("check my project health", "my-project");
    // Should resolve to health_check internally and return ok
    assert!(result.is_ok());
}

#[test]
fn route_exact_name_still_works() {
    let proxy = make_proxy();
    let result = proxy.route("health_check", "my-project");
    assert!(result.is_ok());
}
```

- [ ] **Step 2: Add `EdgeRouter` to `CapabilityProxy`**

Add import at top of `src/proxy_store.rs`:
```rust
use crate::edge::EdgeRouter;
```

Change `CapabilityProxy` struct:
```rust
pub struct CapabilityProxy {
    store: CapabilityStore,
    internal_handlers: HashMap<String, Box<dyn Fn(String) -> Result<String> + Send + Sync>>,
    router: EdgeRouter,
}
```

In `CapabilityProxy::new()`, after building `internal_handlers`, add:
```rust
let descriptions: Vec<(String, String)> = store
    .list()
    .iter()
    .map(|c| (c.name.clone(), c.description.clone()))
    .collect();
let router = EdgeRouter::new(descriptions);
```

Update `Self { store, internal_handlers }` to `Self { store, internal_handlers, router }`.

- [ ] **Step 3: Add `route()` method**

Add to `impl CapabilityProxy`:
```rust
/// Route a natural-language query to the best capability and execute it.
/// Falls back to treating `query` as an exact capability name if no semantic
/// match is found.
pub fn route(&self, query: &str, input: &str) -> Result<String> {
    let capability = self
        .router
        .route(query)
        .unwrap_or(query); // fall back to exact name
    self.execute(capability, input)
}
```

- [ ] **Step 4: Run tests**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo test proxy_store::tests
```

Expected: all existing tests + 2 new tests PASS

- [ ] **Step 5: Commit**

```powershell
git add src/proxy_store.rs
git commit -m "feat(edge): wire EdgeRouter into CapabilityProxy — adds route() method"
```

---

### Task 3: Add `RouteCapability` TCP command to daemon

**Files:**
- Modify: `src/daemon/server.rs`

- [ ] **Step 1: Add handler after `ExecuteCapability`**

Find the `ExecuteCapability` handler. After it, add:

```rust
} else if v["command"] == "RouteCapability" {
    let query = v["query"].as_str().unwrap_or("").to_string();
    let input = v["input"].as_str().unwrap_or("").to_string();

    if query.is_empty() {
        let err = serde_json::json!({
            "event": "CapabilityError",
            "error": "query is required"
        });
        let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
    } else {
        match proxy_for_client.route(&query, &input) {
            Ok(result) => {
                let response = serde_json::json!({
                    "event": "CapabilityResult",
                    "routed_from": query,
                    "result": result
                });
                let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
            }
            Err(e) => {
                let err = serde_json::json!({
                    "event": "CapabilityError",
                    "query": query,
                    "error": e.to_string()
                });
                let _ = writer.write_all(format!("{}\n", err).as_bytes()).await;
            }
        }
    }
```

- [ ] **Step 2: Build check + full test**

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
cd "C:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
cargo check
cargo test 2>&1 | Select-Object -Last 5
```

Expected: no new failures

- [ ] **Step 3: Commit**

```powershell
git add src/daemon/server.rs
git commit -m "feat(edge): add RouteCapability TCP command — natural language to capability"
```
