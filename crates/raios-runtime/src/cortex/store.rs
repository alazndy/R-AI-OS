use super::embedder::{Embedding, EMBEDDING_DIM};
use instant_distance::{Builder, HnswMap, Point, Search};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── DB helpers ───────────────────────────────────────────────────────────────

pub fn default_db_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("raios")
        .join("workspace.db")
}

fn open_conn(db_path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS cortex_chunks (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             path       TEXT    NOT NULL,
             mtime_secs INTEGER NOT NULL,
             start_line INTEGER NOT NULL,
             chunk_text TEXT    NOT NULL,
             embedding  BLOB    NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_cortex_path ON cortex_chunks(path);",
    )?;
    Ok(conn)
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ChunkMeta {
    pub path: String,
    pub start_line: usize,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VectorResult {
    pub path: String,
    pub start_line: usize,
    pub text: String,
    /// Cosine similarity in [0, 1].
    pub score: f32,
}

// ─── HNSW point ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct EmbPoint(Embedding);

impl Point for EmbPoint {
    fn distance(&self, other: &Self) -> f32 {
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        1.0 - dot.clamp(-1.0, 1.0)
    }
}

// ─── VectorEngine ─────────────────────────────────────────────────────────────

pub struct VectorEngine {
    db_path: PathBuf,
    metas: Vec<ChunkMeta>,
    embeddings: Vec<Embedding>,
    indexed_files: HashMap<String, u64>,
    hnsw: Option<HnswMap<EmbPoint, usize>>,
    dirty: bool,
}

impl VectorEngine {
    /// Load from the default workspace database or create empty.
    pub fn load() -> Self {
        Self::load_from(&default_db_path())
    }

    /// Load from an explicit database path (primarily for testing).
    pub fn load_from(db_path: &Path) -> Self {
        let db_path = db_path.to_path_buf();

        // One-time migration: remove legacy JSON store
        let old_json = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("raios")
            .join("cortex_store.json");
        if old_json.exists() {
            let _ = std::fs::remove_file(&old_json);
        }

        let Ok(conn) = open_conn(&db_path) else {
            return Self::empty(db_path);
        };

        let mut metas = Vec::new();
        let mut embeddings = Vec::new();
        let mut indexed_files: HashMap<String, u64> = HashMap::new();

        if let Ok(mut stmt) = conn.prepare(
            "SELECT path, mtime_secs, start_line, chunk_text, embedding
             FROM cortex_chunks
             ORDER BY id",
        ) {
            let _ = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                    ))
                })
                .map(|rows| {
                    for row in rows.flatten() {
                        let (path, mtime, start_line, text, blob) = row;
                        indexed_files.insert(path.clone(), mtime as u64);
                        metas.push(ChunkMeta {
                            path,
                            start_line: start_line as usize,
                            text,
                        });
                        embeddings.push(blob_to_embedding(&blob));
                    }
                });
        }

        let mut engine = Self {
            db_path,
            metas,
            embeddings,
            indexed_files,
            hnsw: None,
            dirty: false,
        };
        engine.rebuild_hnsw();
        engine
    }

    fn empty(db_path: PathBuf) -> Self {
        Self {
            db_path,
            metas: Vec::new(),
            embeddings: Vec::new(),
            indexed_files: HashMap::new(),
            hnsw: None,
            dirty: false,
        }
    }

    /// No-op: writes are committed immediately in `upsert_file`.
    pub fn save(&mut self) {
        self.dirty = false;
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
        // Write to SQLite first — if it fails, don't touch in-memory state.
        if !self.write_to_db(file_path, mtime_secs, &pairs) {
            eprintln!("[VectorEngine] DB write failed for {file_path}, skipping in-memory update");
            return;
        }

        // Remove old in-memory entries for this file.
        let mut keep = vec![true; self.metas.len()];
        for (i, meta) in self.metas.iter().enumerate() {
            if meta.path == file_path {
                keep[i] = false;
            }
        }

        let mut new_metas = Vec::new();
        let mut new_embs: Vec<Embedding> = Vec::new();
        for (i, (meta, emb)) in self
            .metas
            .drain(..)
            .zip(self.embeddings.drain(..))
            .enumerate()
        {
            if keep[i] {
                new_metas.push(meta);
                new_embs.push(emb);
            }
        }

        for (emb, meta) in pairs {
            new_metas.push(meta);
            new_embs.push(emb);
        }

        self.metas = new_metas;
        self.embeddings = new_embs;
        self.indexed_files.insert(file_path.to_string(), mtime_secs);
        self.dirty = true;
        self.rebuild_hnsw();
    }

    fn write_to_db(
        &self,
        file_path: &str,
        mtime_secs: u64,
        pairs: &[(Embedding, ChunkMeta)],
    ) -> bool {
        let Ok(conn) = open_conn(&self.db_path) else {
            eprintln!("[VectorEngine] failed to open DB connection");
            return false;
        };

        let tx = match conn.unchecked_transaction() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[VectorEngine] transaction start failed: {e}");
                return false;
            }
        };

        if let Err(e) = tx.execute(
            "DELETE FROM cortex_chunks WHERE path = ?1",
            params![file_path],
        ) {
            eprintln!("[VectorEngine] DELETE failed: {e}");
            return false;
        }

        for (emb, meta) in pairs {
            let blob = embedding_to_blob(emb);
            if let Err(e) = tx.execute(
                "INSERT INTO cortex_chunks
                     (path, mtime_secs, start_line, chunk_text, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    meta.path,
                    mtime_secs as i64,
                    meta.start_line as i64,
                    meta.text,
                    blob
                ],
            ) {
                eprintln!("[VectorEngine] INSERT failed: {e}");
                return false;
            }
        }

        match tx.commit() {
            Ok(_) => true,
            Err(e) => {
                eprintln!("[VectorEngine] commit failed: {e}");
                false
            }
        }
    }

    /// Rebuild the HNSW in-memory index from current embeddings.
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
        let Some(ref hnsw) = self.hnsw else {
            return vec![];
        };

        let point = EmbPoint(*query_emb);
        let mut search = Search::default();
        let mut results = Vec::new();

        for item in hnsw.search(&point, &mut search) {
            let idx: usize = *item.value;
            if let Some(meta) = self.metas.get(idx) {
                let dist = point.distance(item.point);
                let score = (1.0 - dist).clamp(0.0, 1.0);
                results.push(VectorResult {
                    path: meta.path.clone(),
                    start_line: meta.start_line,
                    text: meta.text.clone(),
                    score,
                });
                if results.len() >= top_k {
                    break;
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    pub fn chunk_count(&self) -> usize {
        self.metas.len()
    }

    pub fn file_count(&self) -> usize {
        self.indexed_files.len()
    }
}

// ─── BLOB encoding (little-endian f32) ───────────────────────────────────────

fn embedding_to_blob(emb: &Embedding) -> Vec<u8> {
    let mut out = Vec::with_capacity(EMBEDDING_DIM * 4);
    for &f in emb.iter() {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_embedding(blob: &[u8]) -> Embedding {
    let mut arr = [0.0f32; EMBEDDING_DIM];
    for (i, chunk) in blob.chunks_exact(4).take(EMBEDDING_DIM).enumerate() {
        arr[i] = f32::from_le_bytes(chunk.try_into().unwrap_or([0u8; 4]));
    }
    arr
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine_with_db(db_path: &std::path::Path) -> VectorEngine {
        VectorEngine::load_from(db_path)
    }

    #[test]
    fn round_trip_upsert_and_query() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let emb: Embedding = {
            let mut e = [0.0f32; EMBEDDING_DIM];
            e[0] = 1.0;
            e
        };

        let mut engine = make_engine_with_db(&db);
        engine.upsert_file(
            "src/main.rs",
            12345,
            vec![(
                emb,
                ChunkMeta {
                    path: "src/main.rs".into(),
                    start_line: 1,
                    text: "fn main() {}".into(),
                },
            )],
        );
        engine.save();

        let engine2 = make_engine_with_db(&db);
        assert_eq!(engine2.chunk_count(), 1);
        assert_eq!(engine2.file_count(), 1);
    }

    #[test]
    fn upsert_replaces_old_chunks_for_same_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let emb = [0.0f32; EMBEDDING_DIM];
        let mut engine = make_engine_with_db(&db);

        engine.upsert_file(
            "a.rs",
            1,
            vec![(
                emb,
                ChunkMeta {
                    path: "a.rs".into(),
                    start_line: 1,
                    text: "old".into(),
                },
            )],
        );
        engine.save();
        engine.upsert_file(
            "a.rs",
            2,
            vec![(
                emb,
                ChunkMeta {
                    path: "a.rs".into(),
                    start_line: 1,
                    text: "new".into(),
                },
            )],
        );
        engine.save();

        let engine2 = make_engine_with_db(&db);
        assert_eq!(engine2.chunk_count(), 1);
    }

    #[test]
    fn is_indexed_uses_mtime() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("test.db");
        let emb = [0.0f32; EMBEDDING_DIM];
        let mut engine = make_engine_with_db(&db);
        engine.upsert_file(
            "x.rs",
            999,
            vec![(
                emb,
                ChunkMeta {
                    path: "x.rs".into(),
                    start_line: 1,
                    text: "x".into(),
                },
            )],
        );
        engine.save();
        assert!(engine.is_indexed("x.rs", 999));
        assert!(!engine.is_indexed("x.rs", 1000));
    }
}
