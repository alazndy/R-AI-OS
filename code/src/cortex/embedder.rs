//! Cortex — Embedding Engine
//!
//! Two modes:
//!   - `cortex` feature ON  → real local embeddings via fastembed (all-MiniLM-L6-v2)
//!   - `cortex` feature OFF → lightweight TF-IDF bag-of-words embedding (fallback)
//!
//! The fallback produces lower-quality but functional vectors that allow the
//! vector store and RRF fusion to work without any native model dependencies.
//! Switch to real embeddings by compiling with `--features cortex`.

use anyhow::Result;

/// Dimensionality common to both modes.
/// all-MiniLM-L6-v2 uses 384; our TF-IDF fallback also maps to 384 buckets.
pub const EMBEDDING_DIM: usize = 384;

pub type Embedding = [f32; EMBEDDING_DIM];

// ─── Real embedder (fastembed) ────────────────────────────────────────────────

#[cfg(feature = "cortex")]
mod real {
    use super::{Embedding, EMBEDDING_DIM};
    use anyhow::{Context, Result};
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

    pub struct Embedder {
        model: TextEmbedding,
    }

    impl Embedder {
        pub fn init() -> Result<Self> {
            let opts = InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true);
            let model = TextEmbedding::try_new(opts)
                .context("fastembed init failed")?;
            Ok(Self { model })
        }

        pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
            let raw: Vec<Vec<f32>> = self.model.embed(texts, None)
                .context("embedding inference failed")?;
            Ok(raw.into_iter().map(|v| norm(v)).collect())
        }

        pub fn embed_one(&self, text: &str) -> Result<Embedding> {
            let mut b = self.embed_batch(vec![text.to_string()])?;
            b.pop().ok_or_else(|| anyhow::anyhow!("empty batch"))
        }
    }

    fn norm(v: Vec<f32>) -> Embedding {
        let mut arr = [0.0f32; EMBEDDING_DIM];
        let len = v.len().min(EMBEDDING_DIM);
        arr[..len].copy_from_slice(&v[..len]);
        let n: f32 = arr.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 1e-9 { arr.iter_mut().for_each(|x| *x /= n); }
        arr
    }
}

// ─── Fallback embedder (TF-IDF bag-of-words, no native deps) ─────────────────

#[cfg(not(feature = "cortex"))]
mod fallback {
    use super::{Embedding, EMBEDDING_DIM};
    use anyhow::Result;
    use std::collections::HashMap;

    pub struct Embedder;

    impl Embedder {
        pub fn init() -> Result<Self> { Ok(Self) }

        pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
            Ok(texts.into_iter().map(|t| bow_embed(&t)).collect())
        }

        pub fn embed_one(&self, text: &str) -> Result<Embedding> {
            Ok(bow_embed(text))
        }
    }

    /// Very fast bag-of-words embedding into EMBEDDING_DIM buckets (FNV hash).
    /// Quality is much lower than real embeddings but non-zero and deterministic.
    fn bow_embed(text: &str) -> Embedding {
        let mut arr = [0.0f32; EMBEDDING_DIM];
        let mut tf: HashMap<u64, u32> = HashMap::new();

        for token in tokenise(text) {
            *tf.entry(fnv1a(&token)).or_insert(0) += 1;
        }

        for (hash, count) in tf {
            let idx = (hash as usize) % EMBEDDING_DIM;
            arr[idx] += (count as f32).ln_1p();
        }

        // L2 normalise
        let n: f32 = arr.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 1e-9 { arr.iter_mut().for_each(|x| *x /= n); }
        arr
    }

    fn tokenise(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() >= 3)
            .map(|s| s.to_lowercase())
            .collect()
    }

    fn fnv1a(s: &str) -> u64 {
        let mut h: u64 = 14_695_981_039_346_656_037;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1_099_511_628_211);
        }
        h
    }
}

// ─── Public re-export ─────────────────────────────────────────────────────────

#[cfg(feature = "cortex")]
pub use real::Embedder;

#[cfg(not(feature = "cortex"))]
pub use fallback::Embedder;
