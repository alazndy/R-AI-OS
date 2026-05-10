//! Cortex — Vector Store
//!
//! Maintains an in-memory HNSW index over chunk embeddings and persists
//! chunk metadata + raw embeddings to `~/.config/raios/cortex_store.json`.
//! On load the HNSW graph is rebuilt from the stored embeddings.
//!
//! Retrieval returns the top-K most similar chunks by cosine similarity
//! (dot-product of L2-normalised vectors).

use std::path::PathBuf;
use std::collections::HashMap;
use instant_distance::{Builder, HnswMap, Search, Point};
use serde::{Deserialize, Serialize};
use super::embedder::{Embedding, EMBEDDING_DIM};

// ─── Index persistence path ───────────────────────────────────────────────────

fn store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
        .join("cortex_store.json")
}

// ─── Point wrapper ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct EmbPoint(Embedding);

impl Point for EmbPoint {
    fn distance(&self, other: &Self) -> f32 {
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        1.0 - dot.clamp(-1.0, 1.0)
    }
}

// ─── Chunk metadata ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub path: String,
    pub start_line: usize,
    pub text: String,
}

/// Result returned by `VectorEngine::query`.
#[derive(Debug, Clone)]
pub struct VectorResult {
    pub path: String,
    pub start_line: usize,
    pub text: String,
    /// Cosine similarity in [0, 1].
    pub score: f32,
}

// ─── Persisted data (what we serialize to disk) ───────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct PersistedStore {
    metas: Vec<ChunkMeta>,
    /// Raw embeddings, parallel to `metas`. Stored as Vec<Vec<f32>> for JSON.
    embeddings: Vec<Vec<f32>>,
    /// Maps file path → mtime seconds (for incremental indexing).
    indexed_files: HashMap<String, u64>,
}

// ─── VectorEngine ─────────────────────────────────────────────────────────────

pub struct VectorEngine {
    metas: Vec<ChunkMeta>,
    embeddings: Vec<Embedding>,
    indexed_files: HashMap<String, u64>,
    /// The HNSW graph — rebuilt on load and after every upsert.
    hnsw: Option<HnswMap<EmbPoint, usize>>,
    dirty: bool,
}

impl VectorEngine {
    /// Load from disk or create empty.
    pub fn load() -> Self {
        let path = store_path();
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(ps) = serde_json::from_slice::<PersistedStore>(&bytes) {
                let embeddings: Vec<Embedding> = ps
                    .embeddings
                    .iter()
                    .map(|v| vec_to_array(v))
                    .collect();

                let mut engine = Self {
                    metas: ps.metas,
                    embeddings,
                    indexed_files: ps.indexed_files,
                    hnsw: None,
                    dirty: false,
                };
                engine.rebuild_hnsw();
                return engine;
            }
        }

        Self {
            metas: Vec::new(),
            embeddings: Vec::new(),
            indexed_files: HashMap::new(),
            hnsw: None,
            dirty: false,
        }
    }

    /// Persist to disk if dirty.
    pub fn save(&mut self) {
        if !self.dirty { return; }
        let path = store_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let ps = PersistedStore {
            metas: self.metas.clone(),
            embeddings: self.embeddings.iter().map(|e| e.to_vec()).collect(),
            indexed_files: self.indexed_files.clone(),
        };
        if let Ok(json) = serde_json::to_string(&ps) {
            let _ = std::fs::write(&path, json);
            self.dirty = false;
        }
    }

    /// Returns true if the file (by mtime) is already indexed and up-to-date.
    pub fn is_indexed(&self, file_path: &str, mtime_secs: u64) -> bool {
        self.indexed_files.get(file_path).copied() == Some(mtime_secs)
    }

    /// Upsert all chunks for a file (removes old entries, appends new ones).
    pub fn upsert_file(
        &mut self,
        file_path: &str,
        mtime_secs: u64,
        pairs: Vec<(Embedding, ChunkMeta)>,
    ) {
        // Remove old entries for this file
        let mut keep = vec![true; self.metas.len()];
        for (i, meta) in self.metas.iter().enumerate() {
            if meta.path == file_path {
                keep[i] = false;
            }
        }
        let mut new_metas = Vec::new();
        let mut new_embs: Vec<Embedding> = Vec::new();
        for (i, (meta, emb)) in self.metas.drain(..).zip(self.embeddings.drain(..)).enumerate() {
            if keep[i] {
                new_metas.push(meta);
                new_embs.push(emb);
            }
        }

        // Append new chunks
        for (emb, meta) in pairs {
            new_metas.push(meta);
            new_embs.push(emb);
        }

        self.metas = new_metas;
        self.embeddings = new_embs;
        self.indexed_files.insert(file_path.to_string(), mtime_secs);
        self.dirty = true;
    }

    pub fn rebuild_hnsw(&mut self) {
        if self.embeddings.is_empty() {
            self.hnsw = None;
            return;
        }
        let points: Vec<EmbPoint> = self.embeddings.iter().map(|e| EmbPoint(*e)).collect();
        let values: Vec<usize> = (0..points.len()).collect();
        self.hnsw = Some(Builder::default().build(points, values));
    }

    /// Query for semantically similar chunks (top-K by cosine similarity).
    pub fn query(&self, query_emb: &Embedding, top_k: usize) -> Vec<VectorResult> {
        let Some(ref hnsw) = self.hnsw else { return vec![]; };

        let point = EmbPoint(*query_emb);
        let mut search = Search::default();
        let mut results = Vec::new();

        for item in hnsw.search(&point, &mut search) {
            let idx: usize = *item.value;
            if let Some(meta) = self.metas.get(idx) {
                // item.point is the neighbour; compute distance via Point trait
                let dist = point.distance(item.point);
                let score = (1.0 - dist).clamp(0.0, 1.0);
                results.push(VectorResult {
                    path: meta.path.clone(),
                    start_line: meta.start_line,
                    text: meta.text.clone(),
                    score,
                });
                if results.len() >= top_k { break; }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    pub fn chunk_count(&self) -> usize { self.metas.len() }
    pub fn file_count(&self) -> usize { self.indexed_files.len() }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn vec_to_array(v: &[f32]) -> Embedding {
    let mut arr = [0.0f32; EMBEDDING_DIM];
    let len = v.len().min(EMBEDDING_DIM);
    arr[..len].copy_from_slice(&v[..len]);
    arr
}
