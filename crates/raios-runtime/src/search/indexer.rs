use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Single source of truth for which file extensions raios's search engines
/// (BM25, trigram, and Cortex — see `cortex::INDEXED_EXTS` re-export) index.
/// Extended 2026-07-10 after discovering raios grep/search were blind to an
/// entire Android/Kotlin project (GT-Launcher): only doc/config-adjacent
/// languages were covered, no mainstream compiled/mobile languages at all.
pub(crate) const INDEXED_EXTS: &[&str] = &[
    "md", "rs", "ts", "tsx", "js", "jsx", "py", "toml", "json", "yaml", "yml",
    "go", "kt", "kts", "java", "swift", "c", "cc", "cpp", "h", "hpp", "cs",
    "rb", "php", "sh", "sql", "dart",
];

/// Single source of truth for which directories raios's search engines never
/// descend into (see `cortex::SKIP_DIRS` re-export). Extended 2026-07-10 after
/// discovering a real 5.7GB Python `venv/` directory (ultimatevocalremovergui)
/// was being walked to death — this list previously lacked `.venv`/`venv` and
/// several other dependency/build directories that cortex/mod.rs's own
/// (separately maintained, already more complete) copy already had.
pub(crate) const SKIP_DIRS: &[&str] = &[
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
    ".fastembed_cache",
    // Ephemeral git-worktree checkouts (Claude Code's isolated-worktree
    // feature lands these at <repo>/.claude/worktrees/<id>/, a full
    // duplicate checkout). Found 2026-07-11: raios locate returned every
    // match twice on GT-Launcher — once from the real source, once from a
    // stale 56MB worktree copy — because "worktrees" wasn't skipped.
    "worktrees",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub path: PathBuf,
    pub project: String,
    pub snippet: String,
    pub score: f32,
    pub line: usize,
}

// (path_index, line_no, snippet)
type Posting = (usize, usize, String);

#[derive(Debug, Clone)]
pub struct ProjectIndex {
    files: Vec<PathBuf>,
    doc_lengths: Vec<usize>,
    inverted: HashMap<String, Vec<Posting>>,
    pub doc_count: usize,
    #[allow(dead_code)]
    db_path: Option<PathBuf>,
}

impl ProjectIndex {
    pub fn build(root: &Path) -> Result<Self> {
        let mut idx = Self {
            files: Vec::new(),
            doc_lengths: Vec::new(),
            inverted: HashMap::new(),
            doc_count: 0,
            db_path: None,
        };

        let walker = WalkDir::new(root)
            .max_depth(12) // Android/Java-style package trees (com/lcars/launcher/...) need real depth
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&n.as_ref())
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file());

        for entry in walker {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !INDEXED_EXTS.contains(&ext) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(path) {
                idx.index_file(path.to_path_buf(), &content);
            }
        }

        Ok(idx)
    }

    fn index_file(&mut self, path: PathBuf, content: &str) -> Vec<(String, usize, String)> {
        let file_id = self.files.len();
        self.files.push(path);
        self.doc_count += 1;

        let mut total_tokens = 0usize;
        let mut new_postings: Vec<(String, usize, String)> = Vec::new();

        for (line_no, line) in content.lines().enumerate() {
            let tokens = tokenize(line);
            total_tokens += tokens.len();
            let snippet: String = line.trim().chars().take(100).collect();
            for token in tokens {
                self.inverted.entry(token.clone()).or_default().push((
                    file_id,
                    line_no + 1,
                    snippet.clone(),
                ));
                new_postings.push((token, line_no + 1, snippet.clone()));
            }
        }

        self.doc_lengths.push(total_tokens.max(1));
        new_postings
    }

    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return vec![];
        }

        // score[file_id] = (total_score, best_line, best_snippet)
        let mut scores: Vec<Option<(f32, usize, String)>> = vec![None; self.files.len()];

        for token in &tokens {
            let Some(postings) = self.inverted.get(token.as_str()) else {
                continue;
            };

            // BM25-inspired IDF
            let idf = ((self.doc_count as f32 + 1.0) / (postings.len() as f32 + 1.0))
                .ln()
                .max(0.0);

            for &(file_id, line_no, ref snippet) in postings {
                let doc_len = self.doc_lengths[file_id] as f32;
                let tf = 1.0 / (1.0 + doc_len.sqrt() / 80.0);
                let entry = scores[file_id].get_or_insert((0.0, line_no, snippet.clone()));
                entry.0 += tf * idf;
            }
        }

        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .enumerate()
            .filter_map(|(id, opt)| {
                opt.map(|(score, line, snippet)| {
                    let path = &self.files[id];
                    let project = path
                        .components()
                        .rev()
                        .nth(1)
                        .and_then(|c| c.as_os_str().to_str())
                        .unwrap_or("?")
                        .to_string();
                    SearchResult {
                        path: path.clone(),
                        project,
                        snippet,
                        score,
                        line,
                    }
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(15);
        results
    }

    pub fn load_or_build(root: &Path, db_path: &Path, force: bool) -> Result<Self> {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let conn = open_bm25_db(db_path)?;
        let fs = fs_mtimes(&root);
        let mut cached = load_cached_bm25_files(&conn);
        cached.retain(|path, _| Path::new(path).starts_with(&root));

        let mut idx = Self {
            files: Vec::new(),
            doc_lengths: Vec::new(),
            inverted: HashMap::new(),
            doc_count: 0,
            db_path: Some(db_path.to_path_buf()),
        };

        let mut stale_ids: Vec<i64> = Vec::new();
        let mut warm_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

        if !force {
            for (path, (file_id, cached_mtime, _)) in &cached {
                match fs.get(path.as_str()) {
                    Some(&fs_mtime) if fs_mtime == *cached_mtime => {
                        warm_paths.insert(path.clone());
                    }
                    _ => {
                        stale_ids.push(*file_id);
                    }
                }
            }
        }

        let tx = conn.unchecked_transaction()?;
        for id in &stale_ids {
            let _ = tx.execute("DELETE FROM bm25_files WHERE id = ?1", params![id]);
        }

        let mut id_to_slot: HashMap<i64, usize> = HashMap::new();
        for (path, (file_id, _, doc_len)) in &cached {
            if warm_paths.contains(path) {
                let slot = idx.files.len();
                id_to_slot.insert(*file_id, slot);
                idx.files.push(PathBuf::from(path));
                idx.doc_lengths.push(*doc_len);
                idx.doc_count += 1;
            }
        }
        if let Ok(mut stmt) =
            conn.prepare("SELECT token, file_id, line_no, snippet FROM bm25_postings")
        {
            let _ = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map(|rows| {
                    for row in rows.flatten() {
                        let (token, file_id, line_no, snippet) = row;
                        if let Some(&slot) = id_to_slot.get(&file_id) {
                            idx.inverted.entry(token).or_default().push((
                                slot,
                                line_no as usize,
                                snippet,
                            ));
                        }
                    }
                });
        }

        for (path_str, &fs_mtime) in &fs {
            if warm_paths.contains(path_str) {
                continue;
            }
            let path = PathBuf::from(path_str);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let postings = idx.index_file(path.clone(), &content);
                let doc_len = idx.doc_lengths.last().copied().unwrap_or(1);

                let _ = tx.execute(
                    "INSERT OR REPLACE INTO bm25_files (path, mtime_secs, doc_length) VALUES (?1,?2,?3)",
                    params![path_str, fs_mtime as i64, doc_len as i64],
                );
                let file_id = tx.last_insert_rowid();
                for (token, line_no, snippet) in &postings {
                    let _ = tx.execute(
                        "INSERT INTO bm25_postings (token, file_id, line_no, snippet) VALUES (?1,?2,?3,?4)",
                        params![token, file_id, *line_no as i64, snippet],
                    );
                }
            }
        }
        tx.commit()?;

        Ok(idx)
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                current.push(lc);
            }
        } else {
            if current.len() >= 3 {
                tokens.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 3 {
        tokens.push(current);
    }
    tokens
}

fn open_bm25_db(db_path: &Path) -> anyhow::Result<Connection> {
    if let Some(p) = db_path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
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
         CREATE INDEX IF NOT EXISTS idx_bm25_token ON bm25_postings(token);",
    )?;
    Ok(conn)
}

pub(crate) fn fs_mtimes(root: &Path) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let walker = WalkDir::new(root)
        .max_depth(12) // Android/Java-style package trees (com/lcars/launcher/...) need real depth
        .into_iter()
        .filter_entry(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());
    for entry in walker {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !INDEXED_EXTS.contains(&ext) {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        map.insert(path.to_string_lossy().to_string(), mtime);
    }
    map
}

fn load_cached_bm25_files(conn: &Connection) -> HashMap<String, (i64, u64, usize)> {
    let mut map = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, path, mtime_secs, doc_length FROM bm25_files") {
        let _ = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map(|rows| {
                for row in rows.flatten() {
                    map.insert(row.1, (row.0, row.2 as u64, row.3 as usize));
                }
            });
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_workspace(tmp: &TempDir) -> PathBuf {
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();
        fs::write(
            ws.join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
        )
        .unwrap();
        ws
    }

    #[test]
    fn load_or_build_creates_index() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");
        let idx = ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        assert!(idx.doc_count >= 2);
        let results = idx.search("println");
        assert!(!results.is_empty());
    }

    #[test]
    fn second_load_uses_cache() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");
        let idx1 = ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        let count1 = idx1.doc_count;
        let idx2 = ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        assert_eq!(idx2.doc_count, count1);
    }

    #[test]
    fn modified_file_triggers_reindex() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");
        ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        fs::write(ws.join("main.rs"), "fn main() { eprintln!(\"changed\"); }").unwrap();
        let idx2 = ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        let results = idx2.search("changed");
        assert!(!results.is_empty());
    }

    #[test]
    fn scope_isolation_survives_a_different_project_being_indexed() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let ws_a = tmp.path().join("a");
        fs::create_dir_all(&ws_a).unwrap();
        fs::write(ws_a.join("main.rs"), "fn main() { println!(\"from a\"); }").unwrap();

        let ws_b = tmp.path().join("b");
        fs::create_dir_all(&ws_b).unwrap();
        fs::write(ws_b.join("main.rs"), "fn main() { println!(\"from b\"); }").unwrap();

        let idx_a1 = ProjectIndex::load_or_build(&ws_a, &db, false).unwrap();
        assert_eq!(idx_a1.doc_count, 1);

        // Indexing B must not evict A's cached rows.
        ProjectIndex::load_or_build(&ws_b, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let a_path = ws_a.join("main.rs").canonicalize().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bm25_files WHERE path = ?1",
                params![a_path.to_string_lossy().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "project A's cached row must survive indexing project B");

        // Re-loading A must warm-reuse the surviving cache, not rebuild from scratch.
        let idx_a2 = ProjectIndex::load_or_build(&ws_a, &db, false).unwrap();
        assert_eq!(idx_a2.doc_count, idx_a1.doc_count);
        let results = idx_a2.search("println");
        assert!(!results.is_empty());
    }

    #[test]
    fn scope_filter_does_not_match_sibling_dir_with_shared_prefix() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");

        let ws = tmp.path().join("R-AI-OS");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("main.rs"), "fn main() { println!(\"core\"); }").unwrap();

        let ws_fork = tmp.path().join("R-AI-OS-fork");
        fs::create_dir_all(&ws_fork).unwrap();
        fs::write(ws_fork.join("main.rs"), "fn main() { println!(\"fork\"); }").unwrap();

        ProjectIndex::load_or_build(&ws, &db, false).unwrap();
        ProjectIndex::load_or_build(&ws_fork, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let core_path = ws.join("main.rs").canonicalize().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bm25_files WHERE path = ?1",
                params![core_path.to_string_lossy().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "R-AI-OS's cached row must survive indexing the sibling R-AI-OS-fork directory"
        );
    }

    #[test]
    fn cold_build_writes_exactly_the_right_number_of_postings() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(&ws.join("a.rs"), "alpha bravo").unwrap();
        fs::write(&ws.join("b.rs"), "charlie delta echo").unwrap();

        ProjectIndex::load_or_build(&ws, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_postings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 5, "posting count must equal exact per-file token occurrences, no duplication or loss");
    }

    #[test]
    fn force_rebuild_replaces_rather_than_duplicates() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ProjectIndex::load_or_build(&ws, &db, true).unwrap();
        let idx2 = ProjectIndex::load_or_build(&ws, &db, true).unwrap();

        let conn = Connection::open(&db).unwrap();
        let file_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bm25_files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(file_count, 2, "REPLACE semantics must not duplicate rows across repeated force rebuilds");

        let results = idx2.search("println");
        assert!(!results.is_empty());
    }
}
