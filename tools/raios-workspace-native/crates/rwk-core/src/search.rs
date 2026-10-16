//! Project search: the RAIOS CLI's indexed `locate`, run as a child process.
//!
//! The search itself happens inside `raios locate` (trigram index, embedding-free). This module
//! owns the contract with that CLI: one child process per search, a bounded wait, JSON parsing,
//! and paths made project-relative so the editor can open them.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, ErrorCode, Result};

/// How long `raios locate` may run before the child is killed.
///
/// A warm search answers in tens of milliseconds through the resident daemon; the first search
/// in a scope pays a one-time index build (measured ~10 s on this machine) and a search with the
/// daemon down pays its silent local fallback. The budget covers both and still bounds a hang.
pub const LOCATE_TIMEOUT: Duration = Duration::from_secs(15);

/// Most hits one search hands to the view.
pub const MAX_HITS: usize = 200;

/// The longest accepted pattern, in bytes.
pub const MAX_PATTERN_BYTES: usize = 512;

/// One match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Project-relative path with `/` separators, as the editor expects. A hit outside the
    /// project root keeps its absolute path (the CLI scans the root, so this is a fallback).
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// The text of that line.
    pub text: String,
}

/// The answer of one search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// At most [`MAX_HITS`] matches.
    pub hits: Vec<SearchHit>,
    /// More matches existed than were returned.
    pub truncated: bool,
}

/// One search over a project; views and tests can script answers behind this.
pub trait SearchSource: Send + Sync {
    /// Searches `root` for `pattern` (`ignore_case` switches the CLI to `-i`).
    fn locate(&self, root: &Path, pattern: &str, ignore_case: bool) -> Result<SearchResult>;
}

/// The production source: runs the `raios` CLI.
#[derive(Debug, Clone, Default)]
pub struct CliSearch {
    program: Option<PathBuf>,
}

impl CliSearch {
    /// A source that runs `program`; `None` resolves the first `raios` on `PATH` per call.
    pub fn new(program: Option<PathBuf>) -> Self {
        Self { program }
    }
}

impl SearchSource for CliSearch {
    fn locate(&self, root: &Path, pattern: &str, ignore_case: bool) -> Result<SearchResult> {
        match &self.program {
            Some(program) => locate_with(program, root, pattern, ignore_case, MAX_HITS),
            None => locate(root, pattern, ignore_case, MAX_HITS),
        }
    }
}

/// Searches `root` with the first `raios` on `PATH`.
pub fn locate(root: &Path, pattern: &str, ignore_case: bool, limit: usize) -> Result<SearchResult> {
    let program = find_in_path(OsStr::new("raios")).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Arama için Raios CLI gerekli; `raios` PATH üzerinde bulunamadı.",
        )
    })?;
    locate_with(&program, root, pattern, ignore_case, limit)
}

/// Searches `root` by running `program`; the seam tests script a fake CLI with.
pub fn locate_with(
    program: &Path,
    root: &Path,
    pattern: &str,
    ignore_case: bool,
    limit: usize,
) -> Result<SearchResult> {
    locate_with_timeout(program, root, pattern, ignore_case, limit, LOCATE_TIMEOUT)
}

/// Like [`locate_with`] but with an explicit time budget; the slow-path tests use it.
pub fn locate_with_timeout(
    program: &Path,
    root: &Path,
    pattern: &str,
    ignore_case: bool,
    limit: usize,
    timeout: Duration,
) -> Result<SearchResult> {
    validate_pattern(pattern)?;
    let mut child = Command::new(program)
        .arg("locate")
        .arg(pattern)
        .arg("--dir")
        .arg(root)
        .arg("-j")
        .args(ignore_case.then_some("-i"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::new(
                ErrorCode::Io,
                format!("Arama başlatılamadı ({}): {error}", program.display()),
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "Arama çıktı akışı açılamadı."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "Arama hata akışı açılamadı."))?;
    // Both pipes are drained on their own threads: a child that fills one before the parent
    // reads the other would otherwise block forever.
    let (output_tx, output_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::BufReader::new(stdout).read_to_end(&mut bytes);
        let _ = output_tx.send(bytes);
    });
    let (error_tx, error_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::BufReader::new(stderr).read_to_end(&mut bytes);
        let _ = error_tx.send(bytes);
    });
    let output = match output_rx.recv_timeout(timeout) {
        Ok(bytes) => bytes,
        Err(_) => {
            // A hung CLI must not hold the caller; the reader thread ends with the closed pipe.
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::new(
                ErrorCode::Io,
                format!(
                    "Arama zaman aşımına uğradı ({} sn); Raios CLI yanıt vermedi.",
                    timeout.as_secs()
                ),
            ));
        }
    };
    let status = child.wait().map_err(|error| {
        AppError::new(ErrorCode::Io, format!("Arama beklemesi başarısız: {error}"))
    })?;
    if !status.success() {
        let errors = error_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_default();
        let message = match failure_detail(&errors) {
            Some(detail) => format!("Arama başarısız: {detail}"),
            None => match status.code() {
                Some(code) => format!("Arama başarısız (çıkış kodu {code})."),
                None => "Arama başarısız.".to_owned(),
            },
        };
        return Err(AppError::new(ErrorCode::Io, message));
    }
    parse_hits(&output, root, limit)
}

/// The first interesting line of the CLI's stderr, without its own error prefix.
fn failure_detail(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = line.strip_prefix("locate failed: ").unwrap_or(line);
    Some(line.chars().take(200).collect())
}

/// A pattern the CLI would misread (an option) or that would match everything is refused.
fn validate_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Arama deseni boş olamaz.",
        ));
    }
    if pattern.starts_with('-') {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Arama deseni '-' ile başlayamaz.",
        ));
    }
    if pattern.contains('\0') || pattern.len() > MAX_PATTERN_BYTES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            format!("Arama deseni en çok {MAX_PATTERN_BYTES} bayt olabilir ve \\0 içeremez."),
        ));
    }
    Ok(())
}

/// One entry of the CLI's JSON, exactly as it arrives.
#[derive(Deserialize)]
struct RawHit {
    line: usize,
    path: String,
    text: String,
}

/// Parses the CLI's answer; whitespace only (no matches at all) is an empty result.
fn parse_hits(bytes: &[u8], root: &Path, limit: usize) -> Result<SearchResult> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(SearchResult {
            hits: Vec::new(),
            truncated: false,
        });
    }
    let raw: Vec<RawHit> = serde_json::from_slice(bytes).map_err(|error| {
        AppError::new(
            ErrorCode::Internal,
            format!("Arama sonucu okunamadı: {error}"),
        )
    })?;
    let truncated = raw.len() > limit;
    let hits = raw
        .into_iter()
        .take(limit)
        .map(|hit| SearchHit {
            path: relative_path(root, &hit.path),
            line: hit.line,
            text: hit.text,
        })
        .collect();
    Ok(SearchResult { hits, truncated })
}

/// The CLI answers with absolute paths under `--dir`; the editor wants project-relative ones
/// with `/` separators. A root that does not literally prefix the path is retried canonicalized.
fn relative_path(root: &Path, path: &str) -> String {
    let path = Path::new(path);
    let relative = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .ok()
        .or_else(|| {
            let path = std::fs::canonicalize(path).ok()?;
            let root = std::fs::canonicalize(root).ok()?;
            path.strip_prefix(root).map(Path::to_path_buf).ok()
        });
    match relative {
        Some(relative) => relative.to_string_lossy().replace('\\', "/"),
        None => path.to_string_lossy().to_string(),
    }
}

/// Resolves a program name against `PATH`; a name containing a slash is checked as given.
///
/// The terminal module has the same helper, but it only builds on Linux while search is
/// portable (the CLI runs on every platform RAIOS supports); the usage module reuses this one.
pub(crate) fn find_in_path(name: &OsStr) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pattern_is_refused_because_it_matches_every_line() {
        let error = validate_pattern("").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("boş olamaz"));
    }

    #[test]
    fn a_pattern_that_looks_like_an_option_is_refused() {
        let error = validate_pattern("--reindex").unwrap_err();
        assert!(error.message.contains("'-' ile başlayamaz"));
    }

    #[test]
    fn an_oversized_pattern_is_refused_and_a_space_is_a_pattern() {
        assert!(validate_pattern(" ").is_ok());
        assert!(validate_pattern(&"a".repeat(MAX_PATTERN_BYTES)).is_ok());
        assert!(validate_pattern(&"a".repeat(MAX_PATTERN_BYTES + 1)).is_err());
        assert!(validate_pattern("a\0b").is_err());
    }

    #[test]
    fn a_hit_under_the_root_is_relative_and_outside_it_keeps_its_path() {
        let root = Path::new("/home/user/project");
        assert_eq!(
            relative_path(root, "/home/user/project/src/main.rs"),
            "src/main.rs"
        );
        assert_eq!(
            relative_path(root, "/elsewhere/project/src/main.rs"),
            "/elsewhere/project/src/main.rs"
        );
        // Canonicalizing can save a path whose text does not literally start with the root.
        let root = Path::new("/home/user/project");
        let hit = format!("{}/src/main.rs", root.display());
        assert_eq!(relative_path(root, &hit), "src/main.rs");
    }

    #[test]
    fn a_whitespace_only_answer_is_an_empty_result() {
        let result = parse_hits(b"   \n", Path::new("/root"), 10).unwrap();
        assert!(result.hits.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn hits_over_the_limit_are_truncated_and_paths_become_relative() {
        let json = br#"[
            {"line": 3, "path": "/root/src/a.rs", "text": "alpha"},
            {"line": 9, "path": "/root/src/b.rs", "text": "beta"}
        ]"#;
        let result = parse_hits(json, Path::new("/root"), 1).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.truncated);
        assert_eq!(result.hits[0].path, "src/a.rs");
        assert_eq!(result.hits[0].line, 3);
        assert_eq!(result.hits[0].text, "alpha");
        let result = parse_hits(json, Path::new("/root"), 2).unwrap();
        assert_eq!(result.hits.len(), 2);
        assert!(!result.truncated);
    }

    #[test]
    fn garbage_is_an_internal_error_not_a_panic() {
        let error = parse_hits(b"not json", Path::new("/root"), 10).unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("okunamadı"));
    }
}
