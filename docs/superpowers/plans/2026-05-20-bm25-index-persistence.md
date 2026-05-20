# BM25 Index Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist the BM25 inverted index to SQLite so restarts don't re-scan the workspace from scratch.

**Architecture:** The inverted index is stored as three tables: `bm25_files` (path + mtime + doc_length), `bm25_postings` (token → file_id, line, snippet). On startup, compare stored file mtimes against the filesystem — files that haven't changed are loaded from the cache, changed/new files are re-indexed and written back. The `ProjectIndex` struct gains a `load_or_build()` constructor that handles this logic.

**Tech Stack:** `rusqlite` (already a dependency), no new crates needed.

---

## File Map

| Action | File | What changes |
|--------|------|-------------|
| Modify | `src/indexer.rs` | Add `load_or_build()`, `save()`, SQLite persistence |
| Modify | `src/db.rs` | Add `bm25_files` + `bm25_postings` tables to `migrate()` |
| Modify | `src/daemon/state.rs` | Pass DB path through to `ProjectIndex::load_or_build()` |

---

### Task 1: Add BM25 tables to SQLite schema

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Add tables inside `migrate()`**

After the `cortex_chunks` table (or after `tasks` if cortex plan not done yet), add:

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS bm25_files (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        path       TEXT UNIQUE NOT NULL,
        mtime_secs INTEGER NOT NULL,
        doc_length INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS bm25_postings (
        token   TEXT    NOT NULL,
        file_id INTEGER NOT NULL REFERENCES bm25_files(id) ON DELETE CASCADE,
        line_no INTEGER NOT NULL,
        snippet TEXT    NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_bm25_token ON bm25_postings(token);
    ",
)?;
```

- [ ] **Step 2: Write failing test**

In `src/db.rs` test module:

```rust
#[test]
fn bm25_tables_exist_after_migrate() {
    let conn = in_memory();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count2: i64 = conn
        .query_row("SELECT COUNT(*) FROM bm25_postings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count2, 0);
}
```

- [ ] **Step 3: Run test**

```
cargo test db::tests::bm25_tables_exist_after_migrate
```

Expected: PASS

- [ ] **Step 4: Commit**

```
git add src/db.rs
git commit -m "feat: add bm25_files and bm25_postings tables to SQLite schema"
```

---

### Task 2: Add `load_or_build()` and `save()` to `ProjectIndex`

**Files:**
- Modify: `src/indexer.rs`

- [ ] **Step 1: Add `db_path` field and helper imports to `ProjectIndex`**

At the top of `src/indexer.rs`, add imports:

```rust
use rusqlite::{params, Connection};
use std::path::Path;
```

Change the `ProjectIndex` struct to:

```rust
#[derive(Debug, Clone)]
pub struct ProjectIndex {
    files: Vec<PathBuf>,
    doc_lengths: Vec<usize>,
    inverted: HashMap<String, Vec<Posting>>,
    pub doc_count: usize,
    db_path: Option<PathBuf>,
}
```

Update the existing `build()` to set `db_path: None` in the constructor:

```rust
pub fn build(root: &Path) -> Result<Self> {
    let mut idx = Self {
        files: Vec::new(),
        doc_lengths: Vec::new(),
        inverted: HashMap::new(),
        doc_count: 0,
        db_path: None,
    };
    // ... rest unchanged
```

- [ ] **Step 2: Write failing tests**

Add at the bottom of `src/indexer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn make_workspace(tmp: &TempDir) -> PathBuf {
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();
        fs::write(ws.join("lib.rs"),  "pub fn add(a: i32, b: i32) -> i32 { a + b }").unwrap();
        ws
    }

    #[test]
    fn load_or_build_creates_index() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        let idx = ProjectIndex::load_or_build(&ws, &db).unwrap();
        assert!(idx.doc_count >= 2);

        let results = idx.search("println");
        assert!(!results.is_empty());
    }

    #[test]
    fn second_load_uses_cache() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        // First build: hits disk
        let idx1 = ProjectIndex::load_or_build(&ws, &db).unwrap();
        let count1 = idx1.doc_count;

        // Second build: should load from cache, same count
        let idx2 = ProjectIndex::load_or_build(&ws, &db).unwrap();
        assert_eq!(idx2.doc_count, count1);
    }

    #[test]
    fn modified_file_triggers_reindex() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ProjectIndex::load_or_build(&ws, &db).unwrap();

        // Modify one file
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(ws.join("main.rs"), "fn main() { eprintln!(\"changed\"); }").unwrap();

        let idx2 = ProjectIndex::load_or_build(&ws, &db).unwrap();
        let results = idx2.search("changed");
        assert!(!results.is_empty());
    }
}
```

- [ ] **Step 3: Run tests — confirm compile failure**

```
cargo test indexer::tests
```

Expected: compile error (`load_or_build` not defined)

- [ ] **Step 4: Implement `load_or_build()` and `save()`**

Add these methods to the `impl ProjectIndex` block:

```rust
/// Open or create a SQLite DB at `db_path`, load cached postings for
/// unchanged files, and re-index changed/new files.
pub fn load_or_build(root: &Path, db_path: &Path) -> Result<Self> {
    let conn = open_index_db(db_path)?;

    // Get current filesystem state: path → mtime
    let fs_files = collect_files(root);

    // Load cached file records: path → (id, mtime, doc_length)
    let cached = load_cached_files(&conn)?;

    let mut idx = Self {
        files: Vec::new(),
        doc_lengths: Vec::new(),
        inverted: HashMap::new(),
        doc_count: 0,
        db_path: Some(db_path.to_path_buf()),
    };

    // Files in cache that are still valid (mtime unchanged)
    let mut warm_ids: Vec<i64> = Vec::new();
    let mut stale_ids: Vec<i64> = Vec::new();

    for (path, (file_id, cached_mtime, _doc_len)) in &cached {
        match fs_files.get(path.as_str()) {
            Some(&fs_mtime) if fs_mtime == *cached_mtime => warm_ids.push(*file_id),
            _ => stale_ids.push(*file_id),
        }
    }

    // Delete stale entries
    for id in &stale_ids {
        conn.execute("DELETE FROM bm25_files WHERE id = ?1", params![id])?;
    }

    // Load warm postings from DB
    load_warm_postings(&conn, &warm_ids, &mut idx, &cached)?;

    // Index new/changed files and insert into DB
    for (path_str, fs_mtime) in &fs_files {
        let path = PathBuf::from(path_str);
        if let Some((file_id, cached_mtime, _)) = cached.get(path_str) {
            if *cached_mtime == *fs_mtime && warm_ids.contains(file_id) {
                continue; // already loaded from cache
            }
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            let file_id = idx.files.len();
            idx.index_file(path.clone(), &content);
            let doc_len = idx.doc_lengths[file_id];
            save_file_to_db(&conn, path_str, *fs_mtime, doc_len, file_id, &idx)?;
        }
    }

    Ok(idx)
}
```

Add the helper functions (outside `impl` block):

```rust
fn open_index_db(db_path: &Path) -> Result<Connection> {
    if let Some(p) = db_path.parent() { let _ = std::fs::create_dir_all(p); }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS bm25_files (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             path TEXT UNIQUE NOT NULL,
             mtime_secs INTEGER NOT NULL,
             doc_length INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS bm25_postings (
             token TEXT NOT NULL,
             file_id INTEGER NOT NULL REFERENCES bm25_files(id) ON DELETE CASCADE,
             line_no INTEGER NOT NULL,
             snippet TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_bm25_token ON bm25_postings(token);"
    )?;
    Ok(conn)
}

fn collect_files(root: &Path) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let walker = walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in walker {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !INDEXED_EXTS.contains(&ext) { continue; }
        let mtime = entry.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        map.insert(path.to_string_lossy().to_string(), mtime);
    }
    map
}

fn load_cached_files(conn: &Connection) -> Result<HashMap<String, (i64, u64, usize)>> {
    let mut stmt = conn.prepare("SELECT id, path, mtime_secs, doc_length FROM bm25_files")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,i64>(2)?, row.get::<_,i64>(3)?))
    })?;
    let mut map = HashMap::new();
    for row in rows.flatten() {
        map.insert(row.1, (row.0, row.2 as u64, row.3 as usize));
    }
    Ok(map)
}

fn load_warm_postings(
    conn: &Connection,
    warm_ids: &[i64],
    idx: &mut ProjectIndex,
    cached: &HashMap<String, (i64, u64, usize)>,
) -> Result<()> {
    if warm_ids.is_empty() { return Ok(()); }

    // Rebuild files + doc_lengths from cached metadata
    let id_set: std::collections::HashSet<i64> = warm_ids.iter().copied().collect();
    let mut id_to_slot: HashMap<i64, usize> = HashMap::new();

    for (path, (file_id, _, doc_len)) in cached {
        if id_set.contains(file_id) {
            let slot = idx.files.len();
            id_to_slot.insert(*file_id, slot);
            idx.files.push(PathBuf::from(path));
            idx.doc_lengths.push(*doc_len);
            idx.doc_count += 1;
        }
    }

    // Load postings
    let mut stmt = conn.prepare(
        "SELECT token, file_id, line_no, snippet FROM bm25_postings WHERE file_id IN (SELECT id FROM bm25_files)"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_,String>(0)?, row.get::<_,i64>(1)?, row.get::<_,i64>(2)?, row.get::<_,String>(3)?))
    })?;
    for row in rows.flatten() {
        let (token, file_id, line_no, snippet) = row;
        if let Some(&slot) = id_to_slot.get(&file_id) {
            idx.inverted.entry(token).or_default().push((slot, line_no as usize, snippet));
        }
    }
    Ok(())
}

fn save_file_to_db(
    conn: &Connection,
    path: &str,
    mtime: u64,
    doc_len: usize,
    file_slot: usize,
    idx: &ProjectIndex,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO bm25_files (path, mtime_secs, doc_length) VALUES (?1, ?2, ?3)",
        params![path, mtime as i64, doc_len as i64],
    )?;
    let file_id = conn.last_insert_rowid();

    // Insert postings for this file
    for (token, postings) in &idx.inverted {
        for &(slot, line_no, ref snippet) in postings {
            if slot == file_slot {
                conn.execute(
                    "INSERT INTO bm25_postings (token, file_id, line_no, snippet) VALUES (?1,?2,?3,?4)",
                    params![token, file_id, line_no as i64, snippet],
                )?;
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests**

```
cargo test indexer::tests
```

Expected: 3 tests PASS

- [ ] **Step 6: Full test suite**

```
cargo test
```

Expected: no new failures

- [ ] **Step 7: Commit**

```
git add src/indexer.rs src/db.rs
git commit -m "feat: persist BM25 index to SQLite for fast restarts"
```

---

### Task 3: Wire `load_or_build` into the daemon

**Files:**
- Modify: `src/bin/aiosd.rs`

- [ ] **Step 1: Replace `ProjectIndex::build` call with `load_or_build`**

In `src/bin/aiosd.rs`, find:

```rust
if let Ok(idx) = ProjectIndex::build(&config.dev_ops_path) {
```

Replace with:

```rust
let db_path = dirs::config_dir()
    .unwrap_or_else(|| std::path::PathBuf::from("."))
    .join("raios")
    .join("workspace.db");

if let Ok(idx) = ProjectIndex::load_or_build(&config.dev_ops_path, &db_path) {
```

- [ ] **Step 2: Add `dirs` import at top of `aiosd.rs` if missing**

```rust
// already in Cargo.toml as a dep — just use it
```

- [ ] **Step 3: Verify build**

```
cargo check
```

Expected: no errors

- [ ] **Step 4: Commit**

```
git add src/bin/aiosd.rs
git commit -m "chore: use cached BM25 index in daemon startup"
```
