//! Cortex — Semantic Memory Engine for R-AI-OS
//!
//! Provides local, privacy-preserving vector search over the entire Dev Ops
//! workspace. All inference runs on-device via fastembed (ONNX Runtime).
//!
//! # Quick-start
//! ```ignore
//! use r_ai_os::cortex::Cortex;
//! use std::path::Path;
//!
//! let mut cortex = Cortex::init().unwrap();
//! cortex.index_workspace(Path::new("/path/to/Dev_Ops_New")).unwrap();
//! let hits = cortex.search("security vulnerability", 10).unwrap();
//! ```

pub mod chunker;
pub mod embedder;
pub mod store;

use anyhow::Result;
use std::path::Path;
use std::time::SystemTime;
use walkdir::WalkDir;

use chunker::chunk_file;
use embedder::Embedder;
use store::{ChunkMeta, VectorEngine, VectorResult};

const INDEXED_EXTS: &[&str] = &[
    "md", "rs", "ts", "tsx", "js", "jsx", "py", "toml", "json", "yaml", "yml", "go",
];

pub const MEMORY_PATTERNS: &[&str] = &["memory.md", "AGENTS.md", "MASTER.md", "CLAUDE.md"];

/// Hard cap: never index more than this many files in one call.
/// Prevents runaway indexing on giant workspaces.
const MAX_FILES_PER_INDEX: usize = 5_000;

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".pnpm",
    "target",
    ".git",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "__pycache__",
    ".turbo",
    ".cache",
    ".venv",
    "venv",
    "vendor",
    ".yarn",
    "coverage",
    ".svelte-kit",
    "out",
];

// ─── Public Cortex struct ─────────────────────────────────────────────────────

/// The semantic memory engine. Wraps the embedder and vector store.
pub struct Cortex {
    embedder: Embedder,
    engine: VectorEngine,
}

impl Cortex {
    /// Initialise the Cortex. Downloads the embedding model on first run.
    pub fn init() -> Result<Self> {
        let embedder = Embedder::init()?;
        let engine = VectorEngine::load();
        Ok(Self { embedder, engine })
    }

    /// Index (or re-index changed files in) a workspace directory.
    /// Skips files that haven't changed since last indexing.
    /// Hard-capped at MAX_FILES_PER_INDEX to avoid runaway indexing.
    pub fn index_workspace(&mut self, root: &Path) -> Result<usize> {
        let mut indexed = 0usize;
        let mut seen = 0usize;

        // Fetch registered projects to avoid blindly scanning the giant root directory
        let projects = crate::entities::load_entities(root);

        for proj in projects {
            if !proj.local_path.exists() {
                continue;
            }

            let walker = WalkDir::new(&proj.local_path)
                .max_depth(6) // deep enough for a single project
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    let n = e.file_name().to_string_lossy();
                    !SKIP_DIRS.contains(&n.as_ref())
                        && !e.path().components().any(|c| c.as_os_str() == ".pnpm")
                })
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file());

            for entry in walker {
                if seen >= MAX_FILES_PER_INDEX {
                    break; // safety cap hit across all projects
                }
                seen += 1;
                if self.index_file(entry.path()).unwrap_or(false) {
                    indexed += 1;
                }
            }

            if seen >= MAX_FILES_PER_INDEX {
                break;
            }
        }

        if indexed > 0 {
            self.engine.rebuild_hnsw();
            self.engine.save();
        }
        Ok(indexed)
    }

    /// Index a single project directory with deeper depth (max 6).
    /// Use this when the user has selected a specific project in the TUI.
    pub fn index_project(&mut self, project_path: &Path) -> Result<usize> {
        let mut indexed = 0usize;

        let walker = WalkDir::new(project_path)
            .max_depth(6)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&n.as_ref())
                    && !e.path().components().any(|c| c.as_os_str() == ".pnpm")
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file());

        for entry in walker {
            if self.index_file(entry.path()).unwrap_or(false) {
                indexed += 1;
            }
        }

        if indexed > 0 {
            self.engine.rebuild_hnsw();
            self.engine.save();
        }
        Ok(indexed)
    }

    /// Rebuilds the search index and saves to disk.
    pub fn rebuild_index(&mut self) {
        self.engine.rebuild_hnsw();
        self.engine.save();
    }

    /// Index a single file. Returns true if it was actually indexed (or re-indexed).
    pub fn index_file(&mut self, path: &Path) -> Result<bool> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !INDEXED_EXTS.contains(&ext) {
            return Ok(false);
        }

        let mtime = file_mtime(path);
        let path_str = path.to_string_lossy().into_owned();

        if self.engine.is_indexed(&path_str, mtime) {
            return Ok(false); // unchanged — skip
        }

        let content = std::fs::read_to_string(path)?;
        let chunks = chunk_file(path, &content);
        if chunks.is_empty() {
            return Ok(false);
        }

        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let embeddings = self.embedder.embed_batch(texts)?;

        let pairs: Vec<_> = embeddings
            .into_iter()
            .zip(chunks)
            .map(|(emb, chunk)| {
                let meta = ChunkMeta {
                    path: chunk.path,
                    start_line: chunk.start_line,
                    text: chunk.text,
                };
                (emb, meta)
            })
            .collect();

        self.engine.upsert_file(&path_str, mtime, pairs);
        Ok(true)
    }

    /// Semantic search: embed the query and retrieve top-K similar chunks.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<VectorResult>> {
        let emb = self.embedder.embed_one(query)?;
        Ok(self.engine.query(&emb, top_k))
    }

    /// Index only memory/agents/master/CLAUDE files across the workspace.
    /// Called automatically when the cortex store is empty and --query is used.
    pub fn index_memory_files(&mut self, root: &Path) -> Result<usize> {
        let mut indexed = 0usize;

        let walker = WalkDir::new(root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&n.as_ref())
                    && !e.path().components().any(|c| c.as_os_str() == ".pnpm")
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file());

        for entry in walker {
            let name = entry.file_name().to_string_lossy();
            if MEMORY_PATTERNS.iter().any(|p| name == *p) {
                if self.index_file(entry.path()).unwrap_or(false) {
                    indexed += 1;
                }
            }
        }

        if indexed > 0 {
            self.engine.rebuild_hnsw();
            self.engine.save();
        }
        Ok(indexed)
    }

    /// Semantic search filtered to files whose names match any of `filename_patterns`.
    /// Pulls top_k * 10 candidates, filters by filename suffix, returns best top_k sorted by score.
    pub fn search_with_filter(
        &self,
        query: &str,
        top_k: usize,
        filename_patterns: &[&str],
    ) -> Result<Vec<VectorResult>> {
        let emb = self.embedder.embed_one(query)?;
        let candidates = self.engine.query(&emb, top_k * 10);
        let mut filtered: Vec<VectorResult> = candidates
            .into_iter()
            .filter(|r| filename_patterns.iter().any(|pat| r.path.ends_with(pat)))
            .take(top_k)
            .collect();
        filtered.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(filtered)
    }

    /// Number of chunks currently in the index.
    pub fn chunk_count(&self) -> usize {
        self.engine.chunk_count()
    }

    /// Number of files currently indexed.
    pub fn file_count(&self) -> usize {
        self.engine.file_count()
    }
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn file_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .and_then(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::cortex::store::VectorResult;

    fn make_result(path: &str, score: f32) -> VectorResult {
        VectorResult { path: path.to_string(), start_line: 1, text: "x".into(), score }
    }

    fn filter_by_patterns(results: Vec<VectorResult>, patterns: &[&str]) -> Vec<VectorResult> {
        results
            .into_iter()
            .filter(|r| patterns.iter().any(|p| r.path.ends_with(p)))
            .collect()
    }

    #[test]
    fn filter_keeps_only_matching_files() {
        let results = vec![
            make_result("/proj/memory.md", 0.9),
            make_result("/proj/main.rs", 0.8),
            make_result("/proj/CLAUDE.md", 0.7),
            make_result("/proj/README.md", 0.6),
        ];
        let filtered = filter_by_patterns(results, &["memory.md", "CLAUDE.md"]);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|r| {
            r.path.ends_with("memory.md") || r.path.ends_with("CLAUDE.md")
        }));
    }

    #[test]
    fn filter_respects_top_k_limit() {
        let results = (0..20)
            .map(|i| make_result(&format!("/proj/{}/memory.md", i), 0.9 - i as f32 * 0.01))
            .collect::<Vec<_>>();
        let filtered: Vec<VectorResult> = filter_by_patterns(results, &["memory.md"])
            .into_iter()
            .take(5)
            .collect();
        assert_eq!(filtered.len(), 5);
    }

    #[test]
    fn filter_returns_empty_when_no_match() {
        let results = vec![make_result("/proj/main.rs", 0.9)];
        let filtered = filter_by_patterns(results, &["memory.md"]);
        assert!(filtered.is_empty());
    }
}
