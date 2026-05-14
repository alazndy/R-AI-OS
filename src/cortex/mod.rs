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
