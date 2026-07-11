use anyhow::{anyhow, Result};
use regex::RegexBuilder;
use rusqlite::{params, Connection, ToSql};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::search::indexer::fs_mtimes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocateMatch {
    pub path: PathBuf,
    pub line_no: usize,
    pub line: String,
}

/// Extract literal runs that MUST appear in any match of `pattern`.
/// Conservative: anything ambiguous means shorter runs or `None`. Never over-claims.
pub(crate) fn extract_required_literals(pattern: &str) -> Option<Vec<String>> {
    if pattern.is_empty() {
        return None;
    }

    let mut runs: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('d' | 'D' | 'w' | 'W' | 's' | 'S' | 'b' | 'B' | 'A' | 'z') | None => {
                    if !cur.is_empty() {
                        runs.push(std::mem::take(&mut cur));
                    }
                }
                Some(esc) => cur.push(esc),
            },
            '|' => return None,
            '(' | ')' => {
                if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
            }
            '?' | '*' => {
                cur.pop();
                if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
            }
            '{' => {
                cur.pop();
                if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        break;
                    }
                }
            }
            '.' | '+' | '[' | ']' | '^' | '$' => {
                if c == '[' {
                    for c2 in chars.by_ref() {
                        if c2 == ']' {
                            break;
                        }
                    }
                }
                if c == '+' {
                    continue;
                }
                if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }

    if !cur.is_empty() {
        runs.push(cur);
    }

    Some(
        runs.into_iter()
            .filter(|run| run.chars().count() >= 3)
            .collect(),
    )
}

pub(crate) fn ensure_index(root: &Path, db_path: &Path, force: bool) -> Result<PathBuf> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let conn = open_trigram_db(db_path)?;
    let fs = fs_mtimes(&root);
    let mut cached = load_cached_trigram_files(&conn);
    cached.retain(|path, _| Path::new(path).starts_with(&root));

    let mut stale_ids: Vec<i64> = Vec::new();
    let mut warm_paths: HashSet<String> = HashSet::new();

    for (path, (file_id, cached_mtime)) in &cached {
        match fs.get(path.as_str()) {
            Some(&fs_mtime) if !force && fs_mtime == *cached_mtime => {
                warm_paths.insert(path.clone());
            }
            _ => stale_ids.push(*file_id),
        }
    }

    let tx = conn.unchecked_transaction()?;
    for id in &stale_ids {
        let _ = tx.execute("DELETE FROM trigram_files WHERE id = ?1", params![id]);
    }

    for (path_str, &fs_mtime) in &fs {
        if warm_paths.contains(path_str) {
            continue;
        }

        let path = PathBuf::from(path_str);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let trigrams = trigrams_of(&content);
        let _ = tx.execute(
            "INSERT OR REPLACE INTO trigram_files (path, mtime_secs) VALUES (?1, ?2)",
            params![path_str, fs_mtime as i64],
        );
        let file_id = tx.last_insert_rowid();
        for trigram in trigrams {
            let _ = tx.execute(
                "INSERT OR IGNORE INTO trigram_postings (trigram, file_id) VALUES (?1, ?2)",
                params![trigram, file_id],
            );
        }
    }

    tx.commit()?;
    Ok(root)
}

pub fn locate(
    root: &Path,
    db_path: &Path,
    pattern: &str,
    case_insensitive: bool,
    force: bool,
) -> Result<Vec<LocateMatch>> {
    let root = ensure_index(root, db_path, force)?;
    let re = RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| anyhow!("invalid pattern: {e}"))?;

    let candidates = match extract_required_literals(pattern) {
        Some(literals) if !literals.is_empty() => {
            let mut required_trigrams: HashSet<String> = HashSet::new();
            for literal in &literals {
                required_trigrams.extend(trigrams_of(literal));
            }
            if required_trigrams.is_empty() {
                all_in_scope_files(&root)
            } else {
                candidates_for_trigrams(db_path, &root, &required_trigrams)?
            }
        }
        _ => all_in_scope_files(&root),
    };

    let mut out = Vec::new();
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                out.push(LocateMatch {
                    path: path.clone(),
                    line_no: i + 1,
                    line: line.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line_no.cmp(&b.line_no)));
    Ok(out)
}

fn all_in_scope_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs_mtimes(root).keys().map(PathBuf::from).collect();
    files.sort();
    files
}

fn candidates_for_trigrams(
    db_path: &Path,
    root: &Path,
    trigrams: &HashSet<String>,
) -> Result<Vec<PathBuf>> {
    let conn = open_trigram_db(db_path)?;
    let mut sorted_trigrams: Vec<&String> = trigrams.iter().collect();
    sorted_trigrams.sort();
    let placeholders = vec!["?"; sorted_trigrams.len()].join(",");
    let sql = format!(
        "SELECT f.path FROM trigram_postings p
         JOIN trigram_files f ON f.id = p.file_id
         WHERE p.trigram IN ({placeholders})
         GROUP BY p.file_id
         HAVING COUNT(DISTINCT p.trigram) = ?"
    );
    let mut params_vec: Vec<&dyn ToSql> = sorted_trigrams
        .iter()
        .map(|trigram| *trigram as &dyn ToSql)
        .collect();
    let required_count = sorted_trigrams.len() as i64;
    params_vec.push(&required_count);

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_vec.as_slice(), |row| row.get::<_, String>(0))?;
    let mut candidates: Vec<PathBuf> = rows
        .flatten()
        .map(PathBuf::from)
        .filter(|path| path.starts_with(root))
        .collect();
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn open_trigram_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS trigram_files (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             path TEXT UNIQUE NOT NULL,
             mtime_secs INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS trigram_postings (
             trigram TEXT NOT NULL,
             file_id INTEGER NOT NULL REFERENCES trigram_files(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_trigram ON trigram_postings(trigram);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_trigram_file ON trigram_postings(trigram, file_id);",
    )?;
    Ok(conn)
}

fn load_cached_trigram_files(conn: &Connection) -> HashMap<String, (i64, u64)> {
    let mut map = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, path, mtime_secs FROM trigram_files") {
        let _ = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map(|rows| {
                for row in rows.flatten() {
                    map.insert(row.1, (row.0, row.2 as u64));
                }
            });
    }
    map
}

fn trigrams_of(content: &str) -> HashSet<String> {
    let lower: Vec<char> = content.to_lowercase().chars().collect();
    lower
        .windows(3)
        .map(|window| window.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn make_workspace(tmp: &TempDir) -> PathBuf {
        let ws = tmp.path().join("ws");
        fs::create_dir_all(&ws).unwrap();
        write_file(&ws.join("main.rs"), "fn main() { println!(\"hello\"); }");
        write_file(
            &ws.join("lib.rs"),
            "pub fn getUserById(id: i32) -> i32 { id }",
        );
        ws
    }

    fn rowids(db: &Path) -> Vec<i64> {
        let conn = Connection::open(db).unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM trigram_files ORDER BY path")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .flatten()
            .collect()
    }

    fn file_count(db: &Path) -> i64 {
        let conn = Connection::open(db).unwrap();
        conn.query_row("SELECT COUNT(*) FROM trigram_files", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn literal_extraction_table() {
        let cases: Vec<(&str, Option<Vec<&str>>)> = vec![
            ("foobar", Some(vec!["foobar"])),
            ("error.*timeout", Some(vec!["error", "timeout"])),
            ("f.b", Some(vec![])),
            ("foo|bar", None),
            ("(foo|bar)baz", None),
            (r"foo\.bar", Some(vec!["foo.bar"])),
            ("colou?r", Some(vec!["colo"])),
            (r"\d+items", Some(vec!["items"])),
            ("getUser.*ById", Some(vec!["getUser", "ById"])),
            ("ab(cd)ef", Some(vec![])),
            ("", None),
        ];
        for (pat, want) in cases {
            let got = extract_required_literals(pat);
            let want = want.map(|v| v.into_iter().map(String::from).collect::<Vec<_>>());
            assert_eq!(got, want, "pattern: {pat:?}");
        }
    }

    #[test]
    fn trigrams_are_distinct_and_lowercased() {
        let got = trigrams_of("AbcAbc");
        let want = ["abc", "bca", "cab"]
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>();
        assert_eq!(got, want);
    }

    #[test]
    fn cold_build_then_cache_reuse() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ensure_index(&ws, &db, false).unwrap();
        let first = rowids(&db);
        ensure_index(&ws, &db, false).unwrap();
        let second = rowids(&db);

        assert_eq!(first, second, "warm cache must not rewrite file rows");
    }

    #[test]
    fn modified_file_reindexes() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ensure_index(&ws, &db, false).unwrap();
        let lib = ws.join("lib.rs").canonicalize().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_file(&ws.join("lib.rs"), "pub fn changedTimeout() {}");
        ensure_index(&ws, &db, false).unwrap();

        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM trigram_postings p
                 JOIN trigram_files f ON f.id = p.file_id
                 WHERE f.path = ?1 AND p.trigram = 'cha'",
                params![lib.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn scope_isolation_survives_other_project() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");
        let ws_a = tmp.path().join("a");
        let ws_b = tmp.path().join("b");
        write_file(&ws_a.join("main.rs"), "alpha project");
        write_file(&ws_b.join("main.rs"), "bravo project");

        ensure_index(&ws_a, &db, false).unwrap();
        ensure_index(&ws_b, &db, false).unwrap();

        let a_path = ws_a.join("main.rs").canonicalize().unwrap();
        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM trigram_files WHERE path = ?1",
                params![a_path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn sibling_prefix_not_matched() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("test.db");
        let ws = tmp.path().join("R-AI-OS");
        let ws_fork = tmp.path().join("R-AI-OS-fork");
        write_file(&ws.join("main.rs"), "core project");
        write_file(&ws_fork.join("main.rs"), "fork project");

        ensure_index(&ws, &db, false).unwrap();
        ensure_index(&ws_fork, &db, false).unwrap();

        let core_path = ws.join("main.rs").canonicalize().unwrap();
        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM trigram_files WHERE path = ?1",
                params![core_path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn force_rebuild_no_duplication() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        ensure_index(&ws, &db, true).unwrap();
        ensure_index(&ws, &db, true).unwrap();

        assert_eq!(file_count(&db), 2);
    }

    #[test]
    fn locate_end_to_end() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        let db = tmp.path().join("test.db");
        write_file(
            &ws.join("a.rs"),
            "fn getUserById() {}\nlet status = \"error then timeout\";\nlet fallback = \"abc\";",
        );
        write_file(
            &ws.join("b.rs"),
            "fn getProfileById() {}\nfn getUserById() {}\nlet loud = \"GETUSERBYID\";",
        );
        write_file(&ws.join("c.rs"), "let error = true;\nlet timeout = true;\n");

        let literal = locate(&ws, &db, "getUserById", false, false).unwrap();
        assert_eq!(literal.len(), 2);
        assert_eq!(
            literal.iter().map(|m| m.line_no).collect::<Vec<_>>(),
            vec![1, 2]
        );

        let regex = locate(&ws, &db, "fn get.*Id", false, false).unwrap();
        assert_eq!(regex.len(), 3);

        let case_sensitive = locate(&ws, &db, "getuserbyid", false, false).unwrap();
        assert!(case_sensitive.is_empty());
        let case_insensitive = locate(&ws, &db, "getuserbyid", true, false).unwrap();
        assert_eq!(case_insensitive.len(), 3);

        let fallback = locate(&ws, &db, "a.c", false, false).unwrap();
        assert_eq!(fallback.len(), 1);
        assert!(fallback[0].line.contains("abc"));

        let same_line = locate(&ws, &db, "error.*timeout", false, false).unwrap();
        assert_eq!(same_line.len(), 1);
        assert_eq!(same_line[0].path, ws.join("a.rs").canonicalize().unwrap());

        let mut sorted = same_line.clone();
        sorted.sort_by(|a, b| a.path.cmp(&b.path).then(a.line_no.cmp(&b.line_no)));
        assert_eq!(same_line, sorted);
    }

    #[test]
    fn locate_rejects_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let ws = make_workspace(&tmp);
        let db = tmp.path().join("test.db");

        let err = locate(&ws, &db, "(", false, false).unwrap_err();
        assert!(err.to_string().contains("invalid pattern"));
    }

    #[test]
    fn walker_uses_indexed_exts_and_skip_dirs() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        write_file(&ws.join("main.rs"), "indexed");
        write_file(&ws.join("target").join("generated.rs"), "skipped");
        write_file(&ws.join("notes.txt"), "skipped");
        let db = tmp.path().join("test.db");

        ensure_index(&ws, &db, false).unwrap();

        assert_eq!(file_count(&db), 1);
    }
}
