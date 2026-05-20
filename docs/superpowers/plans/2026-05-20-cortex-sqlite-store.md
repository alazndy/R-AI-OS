# Cortex SQLite Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `cortex_store.json` with a SQLite table that stores embeddings as binary BLOBs, eliminating JSON parse overhead and enabling per-file atomic updates.

**Architecture:** Embeddings are stored as raw `[f32; 384]` little-endian BLOBs (1536 bytes/chunk) in a new `cortex_chunks` table in the existing `workspace.db`. File mtime is stored alongside each chunk row, so cache invalidation is O(1) per file rather than scanning the full JSON. The HNSW graph is still rebuilt in-memory at load time — the bottleneck was always JSON parsing, not HNSW construction.

**Tech Stack:** `rusqlite` (already a dependency), `r_ai_os::db::open_db`, no new crates needed.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Modify | `src/cortex/store.rs` | Replace JSON persistence with SQLite BLOB storage |
| Modify | `src/db.rs` | Add `cortex_chunks` table to `migrate()` |
| No change | `src/cortex/mod.rs` | Public API unchanged |
| No change | `src/cortex/embedder.rs` | Unchanged |
| No change | `src/cortex/chunker.rs` | Unchanged |

---

### Task 1: Add `cortex_chunks` table to SQLite schema

**Files:**
- Modify: `src/db.rs` (inside `migrate()` fn, after existing CREATE TABLE statements)

- [ ] **Step 1: Add table creation SQL to `migrate()`**

Open `src/db.rs`. Inside `migrate()`, append after the existing `CREATE TABLE IF NOT EXISTS tasks` block:

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS cortex_chunks (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        path        TEXT    NOT NULL,
        mtime_secs  INTEGER NOT NULL,
        start_line  INTEGER NOT NULL,
        chunk_text  TEXT    NOT NULL,
        embedding   BLOB    NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_cortex_path ON cortex_chunks(path);
    ",
)?;
```

- [ ] **Step 2: Write the failing test in `src/db.rs`**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn cortex_table_exists_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cortex_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 3: Run the test, verify it passes**

```
cargo test db::tests::cortex_table_exists_after_migrate
```

Expected: PASS

- [ ] **Step 4: Commit**

```
git add src/db.rs
git commit -m "feat: add cortex_chunks table to SQLite schema"
```

---

### Task 2: Replace JSON persistence in `VectorEngine`

**Files:**
- Modify: `src/cortex/store.rs` — replace `PersistedStore` + JSON save/load with SQLite BLOB I/O

- [ ] **Step 1: Write failing tests first**

Add at the bottom of `src/cortex/store.rs`:

```rust
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
            vec![(emb, ChunkMeta { path: "src/main.rs".into(), start_line: 1, text: "fn main() {}".into() })],
        );
        engine.save();

        // Reload from disk
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

        engine.upsert_file("a.rs", 1, vec![(emb, ChunkMeta { path: "a.rs".into(), start_line: 1, text: "old".into() })]);
        engine.save();
        engine.upsert_file("a.rs", 2, vec![(emb, ChunkMeta { path: "a.rs".into(), start_line: 1, text: "new".into() })]);
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
        engine.upsert_file("x.rs", 999, vec![(emb, ChunkMeta { path: "x.rs".into(), start_line: 1, text: "x".into() })]);
        engine.save();
        assert!(engine.is_indexed("x.rs", 999));
        assert!(!engine.is_indexed("x.rs", 1000));
    }
}
```

- [ ] **Step 2: Run tests, confirm they fail**

```
cargo test cortex::store::tests
```

Expected: compile error (`load_from` not defined yet)

- [ ] **Step 3: Rewrite `src/cortex/store.rs`**

Replace the entire file content with:

```rust
use super::embedder::{Embedding, EMBEDDING_DIM};
use instant_distance::{Builder, HnswMap, Point, Search};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── DB path ─────────────────────────────────────────────────────────────────

fn default_db_path() -> PathBuf {
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

#[derive(Debug, Clone)]
pub struct VectorResult {
    pub path: String,
    pub start_line: usize,
    pub text: String,
    pub score: f32,
}

// ─── HNSW point wrapper ───────────────────────────────────────────────────────

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
    /// In-memory mirrors for search.
    metas: Vec<ChunkMeta>,
    embeddings: Vec<Embedding>,
    /// path → mtime_secs (loaded from DB at startup, kept in sync after upserts).
    indexed_files: HashMap<String, u64>,
    hnsw: Option<HnswMap<EmbPoint, usize>>,
    dirty: bool,
}

impl VectorEngine {
    /// Load from the default workspace.db path.
    pub fn load() -> Self {
        Self::load_from(&default_db_path())
    }

    /// Load from an explicit DB path (used in tests).
    pub fn load_from(db_path: &Path) -> Self {
        let db_path = db_path.to_path_buf();
        let Ok(conn) = open_conn(&db_path) else {
            return Self::empty(db_path);
        };

        let mut metas = Vec::new();
        let mut embeddings = Vec::new();
        let mut indexed_files = HashMap::new();

        let mut stmt = conn
            .prepare(
                "SELECT path, mtime_secs, start_line, chunk_text, embedding
                 FROM cortex_chunks ORDER BY id",
            )
            .unwrap();

        let _ = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let mtime: i64 = row.get(1)?;
            let start_line: i64 = row.get(2)?;
            let text: String = row.get(3)?;
            let blob: Vec<u8> = row.get(4)?;
            Ok((path, mtime as u64, start_line as usize, text, blob))
        }).map(|rows| {
            for row in rows.flatten() {
                let (path, mtime, start_line, text, blob) = row;
                indexed_files.insert(path.clone(), mtime);
                metas.push(ChunkMeta { path, start_line, text });
                embeddings.push(blob_to_embedding(&blob));
            }
        });

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

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        // dirty flag was set by upsert_file which already wrote to DB
        self.dirty = false;
    }

    pub fn is_indexed(&self, file_path: &str, mtime_secs: u64) -> bool {
        self.indexed_files.get(file_path).copied() == Some(mtime_secs)
    }

    pub fn upsert_file(
        &mut self,
        file_path: &str,
        mtime_secs: u64,
        pairs: Vec<(Embedding, ChunkMeta)>,
    ) {
        // Remove from DB
        if let Ok(conn) = open_conn(&self.db_path) {
            let _ = conn.execute("DELETE FROM cortex_chunks WHERE path = ?1", params![file_path]);
        }

        // Remove from in-memory mirrors
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

        // Insert new chunks
        if let Ok(conn) = open_conn(&self.db_path) {
            for (emb, meta) in &pairs {
                let blob = embedding_to_blob(emb);
                let _ = conn.execute(
                    "INSERT INTO cortex_chunks (path, mtime_secs, start_line, chunk_text, embedding)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![meta.path, mtime_secs as i64, meta.start_line as i64, meta.text, blob],
                );
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

    pub fn rebuild_hnsw(&mut self) {
        if self.embeddings.is_empty() {
            self.hnsw = None;
            return;
        }
        let points: Vec<EmbPoint> = self.embeddings.iter().map(|e| EmbPoint(*e)).collect();
        let values: Vec<usize> = (0..points.len()).collect();
        self.hnsw = Some(Builder::default().build(points, values));
    }

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

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    pub fn chunk_count(&self) -> usize { self.metas.len() }
    pub fn file_count(&self) -> usize { self.indexed_files.len() }
}

// ─── BLOB encoding (little-endian f32 array) ──────────────────────────────────

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
```

- [ ] **Step 4: Add `tempfile` to `[dev-dependencies]` if not present**

Check `Cargo.toml` — `tempfile = "3"` should already be there. If not, add it.

- [ ] **Step 5: Run tests, verify they pass**

```
cargo test cortex::store::tests
```

Expected: 3 tests PASS

- [ ] **Step 6: Run full test suite, confirm no regressions**

```
cargo test
```

Expected: same pass count as before (3 pre-existing git failures allowed)

- [ ] **Step 7: Commit**

```
git add src/cortex/store.rs src/db.rs
git commit -m "feat: migrate cortex vector store from JSON to SQLite BLOB"
```

---

### Task 3: Migration — delete old cortex_store.json on first run

**Files:**
- Modify: `src/cortex/store.rs` — add one-time migration in `load_from()`

- [ ] **Step 1: Add migration at top of `load_from()`**

Inside `load_from()`, after `let db_path = db_path.to_path_buf();`, insert:

```rust
// One-time: delete the old JSON store if it exists
let old_json = dirs::config_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join("raios")
    .join("cortex_store.json");
if old_json.exists() {
    let _ = std::fs::remove_file(&old_json);
}
```

- [ ] **Step 2: Run tests**

```
cargo test cortex::store::tests
```

Expected: PASS (no regression)

- [ ] **Step 3: Commit**

```
git add src/cortex/store.rs
git commit -m "chore: remove legacy cortex_store.json on startup"
```
