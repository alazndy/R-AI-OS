//! Cortex — Text Chunker
//!
//! Splits source files into semantically meaningful, overlapping chunks
//! suitable for embedding. Strategy:
//!   - Markdown: split on headings (##, ###) and blank-line paragraphs.
//!   - Code (rs/ts/py/js): split on top-level `fn`/`def`/`function`/`impl` blocks.
//!   - Fallback: sliding window of CHUNK_LINES lines, OVERLAP_LINES overlap.

use std::path::Path;

const CHUNK_LINES: usize = 30;
const OVERLAP_LINES: usize = 5;
const MAX_CHUNK_CHARS: usize = 1500;
const MAX_FILE_BYTES: usize = 512 * 1024; // 512 KB — skip large blobs

/// A single text chunk extracted from a file.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Absolute path of the source file.
    pub path: String,
    /// 1-indexed line where this chunk starts.
    pub start_line: usize,
    /// Text content (trimmed, max MAX_CHUNK_CHARS chars).
    pub text: String,
}

/// Split `content` into chunks according to the file's extension.
pub fn chunk_file(path: &Path, content: &str) -> Vec<Chunk> {
    if content.len() > MAX_FILE_BYTES {
        return vec![];
    }

    let path_str = path.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "md" | "txt" => chunk_markdown(&path_str, content),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "cpp" | "c" => {
            chunk_code(&path_str, content)
        }
        _ => chunk_sliding_window(&path_str, content),
    }
}

// ─── Markdown chunker ─────────────────────────────────────────────────────────

fn chunk_markdown(path: &str, content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut start_line = 1usize;
    let mut current_start = 1usize;

    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;

        let is_heading = line.starts_with("## ") || line.starts_with("### ") || line.starts_with("# ");
        let is_blank = line.trim().is_empty();

        // Flush on heading or after accumulating enough blank-paragraph text
        if (is_heading && !current_lines.is_empty()) || (is_blank && current_lines.len() >= CHUNK_LINES) {
            flush_chunk(path, &current_lines, current_start, &mut chunks);
            current_lines.clear();
            current_start = line_no;
        }

        current_lines.push(line);
        start_line = line_no;
    }

    if !current_lines.is_empty() {
        flush_chunk(path, &current_lines, current_start, &mut chunks);
    }

    let _ = start_line; // suppress warning
    chunks
}

// ─── Code chunker ─────────────────────────────────────────────────────────────

fn chunk_code(path: &str, content: &str) -> Vec<Chunk> {
    // Top-level definition starters — language-agnostic heuristic
    let def_patterns = ["pub fn ", "fn ", "async fn ", "impl ", "struct ", "enum ",
                        "def ", "class ", "function ", "const ", "export "];

    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_start = 1usize;

    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim_start();

        let is_def = def_patterns.iter().any(|p| trimmed.starts_with(p));

        if is_def && current_lines.len() > OVERLAP_LINES {
            flush_chunk(path, &current_lines, current_start, &mut chunks);
            // Overlap: keep last OVERLAP_LINES lines
            let keep = current_lines.len().saturating_sub(OVERLAP_LINES);
            let overlap: Vec<&str> = current_lines[keep..].to_vec();
            let overlap_len = overlap.len();
            current_lines = overlap;
            current_start = line_no.saturating_sub(overlap_len);
        }

        current_lines.push(line);

        // Hard size limit
        if current_lines.len() >= CHUNK_LINES * 2 {
            flush_chunk(path, &current_lines, current_start, &mut chunks);
            let keep = current_lines.len().saturating_sub(OVERLAP_LINES);
            let overlap: Vec<&str> = current_lines[keep..].to_vec();
            let overlap_len = overlap.len();
            current_lines = overlap;
            current_start = line_no.saturating_sub(overlap_len);
        }
    }

    if !current_lines.is_empty() {
        flush_chunk(path, &current_lines, current_start, &mut chunks);
    }

    chunks
}

// ─── Sliding window fallback ──────────────────────────────────────────────────

fn chunk_sliding_window(path: &str, content: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let end = (i + CHUNK_LINES).min(lines.len());
        let window = &lines[i..end];
        flush_chunk(path, window, i + 1, &mut chunks);
        if end == lines.len() { break; }
        i += CHUNK_LINES - OVERLAP_LINES;
    }

    chunks
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn flush_chunk(path: &str, lines: &[&str], start_line: usize, out: &mut Vec<Chunk>) {
    let raw: String = lines.join("\n");
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() < 20 {
        return; // Skip trivially small chunks
    }
    let text: String = trimmed.chars().take(MAX_CHUNK_CHARS).collect();
    out.push(Chunk {
        path: path.to_string(),
        start_line,
        text,
    });
}
