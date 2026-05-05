use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use anyhow::Result;

const INDEXED_EXTS: &[&str] = &[
    "md", "rs", "ts", "tsx", "js", "jsx", "py", "toml", "json", "yaml", "yml",
];

const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", ".git", "dist", "build", ".next", "__pycache__", ".turbo",
];

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub project: String,
    pub snippet: String,
    pub score: f32,
    pub line: usize,
}

// (path_index, line_no, snippet)
type Posting = (usize, usize, String);

pub struct ProjectIndex {
    files: Vec<PathBuf>,
    doc_lengths: Vec<usize>,
    inverted: HashMap<String, Vec<Posting>>,
    pub doc_count: usize,
}

impl ProjectIndex {
    pub fn build(root: &Path) -> Result<Self> {
        let mut idx = Self {
            files: Vec::new(),
            doc_lengths: Vec::new(),
            inverted: HashMap::new(),
            doc_count: 0,
        };

        let walker = WalkDir::new(root)
            .max_depth(6)
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

    fn index_file(&mut self, path: PathBuf, content: &str) {
        let file_id = self.files.len();
        self.files.push(path);
        self.doc_count += 1;

        let mut total_tokens = 0usize;

        for (line_no, line) in content.lines().enumerate() {
            let tokens = tokenize(line);
            total_tokens += tokens.len();
            let snippet: String = line.trim().chars().take(100).collect();
            for token in tokens {
                self.inverted
                    .entry(token)
                    .or_default()
                    .push((file_id, line_no + 1, snippet.clone()));
            }
        }

        self.doc_lengths.push(total_tokens.max(1));
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
