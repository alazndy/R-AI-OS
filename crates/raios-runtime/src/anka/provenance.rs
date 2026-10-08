//! Source roots, project provenance, and harness-header readers.
//!
//! Provenance is always established from harness metadata (`cwd`, `workspace`,
//! directory slug). Prompt text and transcript content never assign scope.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Strict per-header byte bound (plan Phase 1: initial proposal 256 KiB).
///
/// Measured against this machine: the largest `session_meta` line is 23,225 bytes,
/// so the bound leaves >10x headroom while still capping how much a single header
/// can materialize.
pub const HEADER_MAX_BYTES: usize = 256 * 1024;

/// Lines scanned for a Claude `cwd`. Every transcript in the current corpus exposes
/// `cwd` within its first 22 lines (57 of 126 on line 0), so 50 is generous.
pub const CLAUDE_CWD_LINES: usize = 50;

/// Per-line bound for the Claude `cwd` scan. The largest line inside the first 50
/// lines of any transcript is 686,151 bytes, and none reaches 1 MiB.
pub const CLAUDE_LINE_MAX_BYTES: usize = 1024 * 1024;

/// Per-line bound when streaming prompt-history files.
///
/// Measured against this machine: the longest history line is 81,021 bytes (Codex),
/// against 4,774 (OpenCode) and 3,974 (Antigravity) — so the bound keeps >3x
/// headroom over the largest legitimate entry while capping what a single line can
/// materialize. A line past the bound is skipped and counted, never parsed.
pub const HISTORY_LINE_MAX_BYTES: usize = 256 * 1024;

/// How the record's timestamp was obtained, kept separate from the value itself so a
/// filesystem observation time is never presented as an event time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeSource {
    /// The harness supplied an event timestamp with the record.
    EventTimestamp,
    /// No event timestamp exists; the value is the source file's modification time.
    FilesystemObservation,
    #[default]
    Unavailable,
}

/// Single source of every filesystem root.
///
/// Only [`AnkaRoots::live`] consults the process environment. Tests construct this
/// struct directly so no fixture can touch real history.
#[derive(Debug, Clone)]
pub struct AnkaRoots {
    pub home: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl AnkaRoots {
    /// Environment-derived roots. The only constructor that reads the environment.
    pub fn live() -> Self {
        Self {
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            config: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from(".config"))
                .join("raios"),
            cache: raios_core::anka::default_cache_path(),
        }
    }

    pub fn claude_projects(&self) -> PathBuf {
        self.home.join(".claude").join("projects")
    }

    pub fn codex_history(&self) -> PathBuf {
        self.home.join(".codex").join("history.jsonl")
    }

    /// The active rollout root.
    pub fn codex_sessions(&self) -> PathBuf {
        self.home.join(".codex").join("sessions")
    }

    /// The archived rollout root.
    pub fn codex_archived_sessions(&self) -> PathBuf {
        self.home.join(".codex").join("archived_sessions")
    }

    /// Every supported active/archive rollout root, in a fixed order so discovery
    /// order cannot vary between builds. Missing roots are still returned and are
    /// skipped by the walker rather than conditionally omitted here.
    pub fn codex_session_roots(&self) -> Vec<PathBuf> {
        vec![self.codex_sessions(), self.codex_archived_sessions()]
    }

    pub fn opencode_history(&self) -> PathBuf {
        self.home
            .join(".local")
            .join("state")
            .join("opencode")
            .join("prompt-history.jsonl")
    }

    pub fn antigravity_history(&self) -> PathBuf {
        self.home
            .join(".gemini")
            .join("antigravity-cli")
            .join("history.jsonl")
    }

    pub fn config_file(&self, name: &str) -> PathBuf {
        self.config.join(name)
    }
}

/// What is actually claimed about the record's project.
///
/// [`ProjectScope::Slug`] is deliberately distinct from [`ProjectScope::Scoped`]: a
/// directory slug is lossy evidence and is never promoted to a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectScope {
    /// A real filesystem path asserted by harness metadata.
    Scoped(String),
    /// The raw directory slug exactly as stored by the harness (e.g. `-home-alaz-dev-ai-svp`).
    /// Slug evidence only; no path was reconstructed from it.
    Slug(String),
    /// The asserted path resolved exactly to `$HOME`.
    HomeUnscoped,
    /// No provable project.
    Unknown,
}

impl ProjectScope {
    /// Display and filter value. Never invents a path for [`ProjectScope::Slug`].
    pub fn as_display(&self) -> String {
        match self {
            Self::Scoped(path) => path.clone(),
            Self::Slug(slug) => slug.clone(),
            Self::HomeUnscoped => "$HOME".to_string(),
            Self::Unknown => String::new(),
        }
    }
}

/// How strongly the project was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProvenanceQuality {
    /// `cwd` or `workspace` field reported by the harness itself.
    Direct,
    /// A history record whose project came from its session header, not the record.
    SessionCwd,
    /// Evidence is the raw directory slug only; no path was reconstructed.
    SlugEncoded,
    #[default]
    Unknown,
}

/// Provenance of a single record. The authority for privacy decisions; `source.project`
/// remains the display string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub scope: ProjectScope,
    /// Raw directory slug when the harness layout provides one. Kept alongside a
    /// resolved [`ProjectScope::Scoped`] so policy can match either without the slug
    /// being mistaken for a path.
    pub slug: Option<String>,
    /// The normalized path the harness metadata asserted, whenever it provided one:
    /// `Some` for path-scoped records — including ones classified
    /// [`ProjectScope::HomeUnscoped`] — and `None` for slug-only or unknown
    /// provenance. Policy compares *this* against the consent's `home_path` and
    /// against substring rules, because the import-time `ProjectScope` label alone
    /// cannot say *which* home it spoke for: it was computed against whatever `$HOME`
    /// the indexing process saw. `default` keeps caches written before this field
    /// existed readable.
    #[serde(default)]
    pub path: Option<String>,
    pub quality: ProvenanceQuality,
    /// Field that produced the scope, e.g. `"cwd"`, `"workspace"`, `"directory_slug"`.
    /// `"none"` when no metadata existed. Never prompt content.
    pub observed_via: String,
    pub time_source: TimeSource,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            scope: ProjectScope::Unknown,
            slug: None,
            path: None,
            quality: ProvenanceQuality::Unknown,
            observed_via: "none".to_string(),
            time_source: TimeSource::Unavailable,
        }
    }
}

impl Provenance {
    pub fn direct(path: &str, roots: &AnkaRoots) -> Self {
        Self {
            scope: classify_asserted_path(path, roots),
            quality: ProvenanceQuality::Direct,
            observed_via: "cwd".to_string(),
            time_source: TimeSource::Unavailable,
            slug: None,
            path: normalize_path(path),
        }
    }

    /// Project derived from a session header rather than the record itself, so the
    /// weaker quality is preserved instead of being reported as direct.
    pub fn session_cwd(cwd: &str, roots: &AnkaRoots, observed_via: &'static str) -> Self {
        Self {
            scope: classify_asserted_path(cwd, roots),
            quality: ProvenanceQuality::SessionCwd,
            observed_via: observed_via.to_string(),
            time_source: TimeSource::Unavailable,
            slug: None,
            path: normalize_path(cwd),
        }
    }

    pub fn with_slug(mut self, slug: &str) -> Self {
        self.slug = Some(slug.to_string());
        self
    }

    pub fn with_time(mut self, time_source: TimeSource) -> Self {
        self.time_source = time_source;
        self
    }

    /// Project known only from the harness directory slug.
    pub fn from_slug(slug: &str) -> Self {
        Self {
            scope: ProjectScope::Slug(slug.to_string()),
            slug: Some(slug.to_string()),
            path: None,
            quality: ProvenanceQuality::SlugEncoded,
            observed_via: "directory_slug".to_string(),
            time_source: TimeSource::Unavailable,
        }
    }

    /// Project not provable from metadata.
    pub fn unknown() -> Self {
        Self::default()
    }

    pub fn with_field(mut self, field: &'static str) -> Self {
        self.observed_via = field.to_string();
        self
    }
}

/// Normalization contract — deliberately narrow and entirely filesystem-free:
///
/// 1. surrounding whitespace is trimmed;
/// 2. every run of `/` collapses to a single separator and a trailing separator is
///    removed, unless the path *is* `/` — POSIX resolves `/home//alaz` exactly like
///    `/home/alaz`, so an interior `//` must not dodge an exact-path privacy
///    comparison;
/// 3. nothing else is rewritten: no symlink resolution, no `.`/`..` collapsing,
///    no case folding, no `~` expansion, no locale tricks;
/// 4. the filesystem is never consulted, so classification cannot be redirected by
///    a symlink swap and stays reproducible across rebuilds.
///
/// Returns `None` for an empty value, a relative path, or a path carrying a `.`/`..`
/// component. Those are "no provable project", never a guessed location.
pub fn normalize_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return None;
    }
    let mut normalized = String::with_capacity(trimmed.len());
    let mut previous_was_slash = false;
    for ch in trimmed.chars() {
        if ch == '/' {
            if !previous_was_slash {
                normalized.push(ch);
            }
            previous_was_slash = true;
        } else {
            normalized.push(ch);
            previous_was_slash = false;
        }
    }
    if normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    if normalized
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        // Split on `/` rather than using `Path::components()`: that API normalizes
        // interior `.` away, so it would silently hide the very case being rejected.
        return None;
    }
    Some(normalized)
}

/// Classify a path asserted by the harness.
///
/// Only an absolute, already-normalized path can be [`ProjectScope::Scoped`]. Exactly
/// `$HOME` becomes [`ProjectScope::HomeUnscoped`]. Anything unnormalizable — relative
/// path, `.`, `..`, empty — becomes [`ProjectScope::Unknown`].
///
/// This never reconstructs a path from a slug — callers must pass harness metadata.
pub fn classify_asserted_path(raw: &str, roots: &AnkaRoots) -> ProjectScope {
    let Some(path) = normalize_path(raw) else {
        return ProjectScope::Unknown;
    };
    let home = normalize_path(&roots.home.to_string_lossy());
    if home.as_deref() == Some(path.as_str()) {
        return ProjectScope::HomeUnscoped;
    }
    ProjectScope::Scoped(path)
}

/// Open a path only when it is a regular file, re-checked immediately after opening.
///
/// `WalkDir::follow_links(false)` stops traversal from *entering* a symlinked
/// directory, but `Path::is_file()` and `File::open` both resolve a symlink whose
/// target is a regular file. Discovery therefore keeps only entries whose own file
/// type is regular, and every open goes through here.
///
/// What this does **not** prove: the identity of the resulting descriptor. Replacing
/// the path *after* `File::open` is harmless — an open fd keeps pointing at whatever
/// it was opened as, and no later rename can retarget it. The real gap is earlier:
/// the path can be swapped to a symlink between the first check and the `open` (so
/// the fd lands on the target) and restored before the second check. Both path checks
/// then pass while the descriptor refers to something else, because path metadata is
/// never a proof about an fd.
///
/// Closing that window requires `O_NOFOLLOW` (or `openat` plus an `fstat` identity
/// comparison), which needs a `libc` dependency this crate does not carry. **Must be
/// closed before the live migration**; until then this is a documented gap, not a
/// solved one.
pub fn open_regular_file(path: &Path) -> io::Result<fs::File> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let file = fs::File::open(path)?;
    let after = fs::symlink_metadata(path)?;
    if !after.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path changed during open",
        ));
    }
    Ok(file)
}

/// Outcome of one bounded line read.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BoundedLine {
    /// A line, with trailing `\r`/`\n` removed.
    Line(String),
    /// The line exceeded the bound.
    ///
    /// `line_complete` says whether the stream is still *inside* that line:
    /// - `false` — the budget ran out mid-line; the caller **must** call
    ///   [`skip_rest_of_line`] before the next read.
    /// - `true` — the terminator (or end of file) was already consumed, so the reader
    ///   sits on the next line and skipping would swallow it.
    ///
    /// No further bytes are ever read past the budget to reach this verdict: it is
    /// decided from what the bounded read already produced.
    Oversized { line_complete: bool },
    /// No more input: the reader is at end of file.
    Eof,
}

/// Read one line through a byte budget, so a pathological line can never be
/// materialized whole. Never reads past `max_bytes + 2` bytes (`+2` covers a `\r\n`
/// terminator), and on an oversized line it **stops reading there** rather than
/// consuming the rest.
///
/// What the caller does on [`BoundedLine::Oversized`] depends on `line_complete`
/// — see the variant's docs. A header reader rejects the file outright; a scanning
/// caller skips only when the line is still open, because an already-terminated
/// oversized line has left the reader on the *next* line and a skip would delete it.
pub(super) fn next_line_bounded(
    reader: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    max_bytes: usize,
) -> io::Result<BoundedLine> {
    buffer.clear();
    // Two bytes of slack: the content itself plus a possible `\r\n` terminator.
    let budget = max_bytes as u64 + 2;
    let read = {
        let mut limited = reader.take(budget);
        limited.read_until(b'\n', buffer)?
    };
    if read == 0 {
        return Ok(BoundedLine::Eof);
    }

    let content = trim_line_ending(buffer);
    if content.len() > max_bytes {
        // Either the terminator arrived inside the budget (then it is already
        // consumed and the reader is on the next line), or the read stopped exactly
        // at the budget without a terminator (then the line is still open). A short
        // read means end of file, so nothing is left to skip either way.
        let line_complete = buffer.ends_with(b"\n") || (read as u64) < budget;
        buffer.clear();
        return Ok(BoundedLine::Oversized { line_complete });
    }
    let line = String::from_utf8_lossy(content).into_owned();
    buffer.clear();
    Ok(BoundedLine::Line(line))
}

/// Skip to the end of the current line **without allocating**.
///
/// Memory stays at the reader's own buffer capacity regardless of how long the line
/// is — `fill_buf`/`consume` only walk the existing buffer, so a multi-megabyte or
/// newline-free line never becomes a `Vec`. Returns `Ok` at EOF when no newline
/// remains.
///
/// Only correct when the reader is genuinely mid-line: see
/// [`BoundedLine::Oversized::line_complete`]. Calling it on a reader that already
/// sits on the next line consumes that line.
pub(super) fn skip_rest_of_line(reader: &mut impl BufRead) -> io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(()); // EOF: nothing left to skip.
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                reader.consume(index + 1);
                return Ok(());
            }
            None => {
                let len = available.len();
                reader.consume(len);
            }
        }
    }
}

fn trim_line_ending(buffer: &[u8]) -> &[u8] {
    let without_lf = buffer.strip_suffix(b"\n").unwrap_or(buffer);
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

/// Which header key produced a Codex cwd match. Recorded so the join key the plan
/// mandates (`history.session_id -> session_meta.payload.id`) can be measured rather
/// than assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CwdMatch {
    ById,
    BySessionId,
}

#[derive(Debug, Default, Clone)]
struct KeyEntry {
    cwd: String,
    from_id: bool,
    from_alias: bool,
    conflicted: bool,
}

/// Session-id to `cwd` map built from rollout **headers only**.
///
/// Reading the rollout transcript corpus is deliberately out of scope: only the first
/// line of each rollout is read, and only when it is a `session_meta` header.
///
/// Ambiguity is decided at build time and is order-independent: a key observed with
/// two different normalized cwds is marked conflicted and never resolves, so the
/// order in which files happen to be walked cannot change attribution.
#[derive(Debug, Default)]
pub struct CodexCwdMap {
    keys: HashMap<String, KeyEntry>,
    /// Rollout files whose first line was not a usable `session_meta` header
    /// (wrong type, malformed, non-object payload, missing `payload.id`, relative
    /// cwd, oversized, or unreadable).
    pub rejected_headers: usize,
    /// Files skipped because the entry is not a regular file (symlink, device, dir
    /// inside a `.jsonl` name check). Never resolved through.
    pub skipped_non_regular: usize,
    /// Filename stem disagreed with `payload.id`. Diagnostics only — filenames are
    /// never authoritative for joining.
    pub filename_disagreements: usize,
    /// Distinct keys excluded because more than one cwd was observed for them.
    pub ambiguous_ids: usize,
}

impl CodexCwdMap {
    fn register(&mut self, key: &str, cwd: &str, via_id: bool) {
        if key.is_empty() {
            return;
        }
        match self.keys.get_mut(key) {
            None => {
                self.keys.insert(
                    key.to_string(),
                    KeyEntry {
                        cwd: cwd.to_string(),
                        from_id: via_id,
                        from_alias: !via_id,
                        conflicted: false,
                    },
                );
            }
            Some(entry) => {
                if entry.cwd != cwd {
                    // First seen loses: conflict is sticky, so discovery order is
                    // irrelevant and the session is ambiguous either way.
                    entry.conflicted = true;
                }
                entry.from_id |= via_id;
                entry.from_alias |= !via_id;
            }
        }
    }

    /// Resolve a history `session_id` to the cwd its rollout started in.
    ///
    /// `payload.id` is the primary key; `payload.session_id` is a documented legacy
    /// alias, usable only when it never conflicts. A conflicted key resolves to
    /// `None`, which callers surface as unknown provenance.
    pub fn resolve(&self, session_id: &str) -> Option<(String, CwdMatch)> {
        let entry = self.keys.get(session_id)?;
        if entry.conflicted {
            return None;
        }
        let matched = if entry.from_id {
            CwdMatch::ById
        } else {
            CwdMatch::BySessionId
        };
        Some((entry.cwd.clone(), matched))
    }

    /// Keys that resolve, split by whether they came from `payload.id` or its alias.
    pub fn primary_keys(&self) -> usize {
        self.keys
            .values()
            .filter(|entry| !entry.conflicted && entry.from_id)
            .count()
    }

    pub fn fallback_keys(&self) -> usize {
        self.keys
            .values()
            .filter(|entry| !entry.conflicted && !entry.from_id)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.values().all(|entry| entry.conflicted)
    }
}

/// Diagnostics only: does the filename carry an id that disagrees with `payload.id`?
///
/// Rollout names look like `rollout-2026-10-05T18-26-07-<uuid>`, so comparing the
/// whole stem to `payload.id` disagrees on *every* file and would drown a real
/// mismatch. Only a trailing UUID-shaped token is compared, and `None` means the
/// filename carries nothing comparable — which is not a disagreement.
fn filename_disagreement(path: &Path, payload_id: &str) -> Option<bool> {
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    let tail = stem.get(stem.len().checked_sub(36)?..)?;
    if !is_uuid_like(tail) {
        return None;
    }
    Some(tail != payload_id)
}

fn is_uuid_like(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.len() == 36
        && matches!(&bytes[8], b'-')
        && matches!(&bytes[13], b'-')
        && matches!(&bytes[18], b'-')
        && matches!(&bytes[23], b'-')
        && candidate
            .bytes()
            .enumerate()
            .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
            .all(|(_, byte)| byte.is_ascii_hexdigit())
}

/// The type required of a rollout's first line. Anything else is not a header.
const CODEX_ROLLOUT_HEADER_TYPE: &str = "session_meta";

struct RolloutHeader {
    id: String,
    alias: Option<String>,
    cwd: String,
}

/// Build the Codex session header map across every supported active/archive root.
///
/// Only each rollout's **first** line is read, bounded by [`HEADER_MAX_BYTES`]; the
/// rest of the file is never touched, and symlinks are never resolved through.
pub fn codex_cwd_map(roots: &AnkaRoots) -> Result<CodexCwdMap> {
    let mut map = CodexCwdMap::default();
    for root in roots.codex_session_roots() {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            // `entry.file_type()` is the symlink's own type when links are not
            // followed, so a symlinked rollout is rejected rather than resolved.
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if entry.file_type().is_symlink() {
                map.skipped_non_regular += 1;
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(header) = read_rollout_header(path) else {
                map.rejected_headers += 1;
                continue;
            };
            if let Some(disagreement) = filename_disagreement(path, &header.id) {
                map.filename_disagreements += disagreement as usize;
            }
            map.register(&header.id, &header.cwd, true);
            if let Some(alias) = &header.alias {
                map.register(alias, &header.cwd, false);
            }
        }
    }
    map.ambiguous_ids = map.keys.values().filter(|entry| entry.conflicted).count();
    Ok(map)
}

/// Read a rollout's header: **only the first line**, and only if it is a
/// `session_meta` record with an object payload, a non-empty string `payload.id`,
/// and an absolute `payload.cwd`.
///
/// A later event's `payload.cwd` is never consulted — the loop that would allow that
/// does not exist here. An oversized first line rejects the file immediately: the
/// reader is dropped without consuming one byte past [`HEADER_MAX_BYTES`].
fn read_rollout_header(path: &Path) -> Option<RolloutHeader> {
    let file = open_regular_file(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let BoundedLine::Line(line) =
        next_line_bounded(&mut reader, &mut buffer, HEADER_MAX_BYTES).ok()?
    else {
        // `Oversized` or EOF: reject now, deliberately without draining the rest.
        return None;
    };

    let value: serde_json::Value = serde_json::from_str(&line).ok()?;
    if value.get("type").and_then(|kind| kind.as_str()) != Some(CODEX_ROLLOUT_HEADER_TYPE) {
        return None;
    }
    let payload = value.get("payload")?;
    if !payload.is_object() {
        return None;
    }
    let id = payload.get("id").and_then(|id| id.as_str())?.trim();
    if id.is_empty() {
        return None;
    }
    // Absolute and already normalized, or the header is rejected outright.
    let cwd = normalize_path(payload.get("cwd").and_then(|cwd| cwd.as_str())?)?;
    let alias = payload
        .get("session_id")
        .and_then(|session_id| session_id.as_str())
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string);

    Some(RolloutHeader {
        id: id.to_string(),
        alias,
        cwd,
    })
}

/// Read at most `max_lines` lines, returning the first non-empty string found under
/// the top-level `key`. Nested `payload` keys are deliberately not considered: that
/// is rollout-header shape, which has its own strict reader above.
///
/// Malformed, unreadable, and oversized lines are skipped without their contents
/// being logged.
pub(super) fn first_top_level_string(
    path: &Path,
    key: &str,
    max_lines: usize,
    max_line_bytes: usize,
) -> Option<String> {
    let file = open_regular_file(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    for _ in 0..max_lines {
        match next_line_bounded(&mut reader, &mut buffer, max_line_bytes).ok()? {
            BoundedLine::Eof => break,
            BoundedLine::Oversized { line_complete } => {
                // Keep scanning. Skipping is needed only when the budget ran out
                // mid-line: an oversized line that already carried its terminator has
                // left the reader on the *next* line, and skipping would delete it.
                if !line_complete && skip_rest_of_line(&mut reader).is_err() {
                    break;
                }
            }
            BoundedLine::Line(line) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if let Some(found) = value.get(key).and_then(|value| value.as_str()) {
                    if !found.trim().is_empty() {
                        return Some(found.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

/// `cwd` asserted by a Claude transcript's own records.
pub fn claude_cwd(path: &Path) -> Option<String> {
    first_top_level_string(path, "cwd", CLAUDE_CWD_LINES, CLAUDE_LINE_MAX_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Synthetic roots only: no fixture reads real history or the real cache.
    fn roots_in(home: &Path) -> AnkaRoots {
        AnkaRoots {
            home: home.to_path_buf(),
            config: home.join(".config").join("raios"),
            cache: home.join(".cache").join("raios").join("anka"),
        }
    }

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture dir");
        fs::write(path, body).expect("write fixture");
    }

    fn session_meta(id: &str, alias: Option<&str>, cwd: &str) -> String {
        let mut payload = json!({"id": id, "cwd": cwd});
        if let Some(alias) = alias {
            payload["session_id"] = json!(alias);
        }
        format!("{}\n", json!({"type": "session_meta", "payload": payload}))
    }

    #[test]
    fn rollout_header_is_read_from_the_first_line_only_and_requires_session_meta() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let rollout = roots.codex_sessions().join("2026/01/01/rollout-x.jsonl");
        // A later event carries a `payload.cwd`; it must never become provenance.
        write(
            &rollout,
            &format!(
                "{}{}\n",
                json!({"type": "message", "payload": {"cwd": "/later/must/not/win"}}),
                session_meta("SID", Some("SID"), "/home/alaz/dev/ai"),
            ),
        );
        write(
            &roots.codex_history(),
            &format!("{}\n", json!({"session_id": "SID", "text": "p", "ts": 1})),
        );

        let map = codex_cwd_map(&roots).expect("map");

        assert_eq!(map.rejected_headers, 1, "first line was not session_meta");
        assert!(
            map.resolve("SID").is_none(),
            "a non-header first line must leave provenance unknown"
        );
    }

    #[test]
    fn oversized_rollout_header_is_rejected_without_materializing_it() {
        // Rejection end-to-end. The byte bound that makes this cheap is asserted
        // separately in `an_oversized_line_stops_reading_at_the_budget_and_a_skip_stays_fixed_size`,
        // which measures exactly what `read_rollout_header` reads before it gives up.
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let pad = "A".repeat(HEADER_MAX_BYTES + 1024);
        let rollout = roots.codex_sessions().join("2026/01/01/rollout-big.jsonl");
        write(
            &rollout,
            &format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"SID\",\"session_id\":\"SID\",\"cwd\":\"/home/alaz/dev/ai\",\"pad\":\"{pad}\"}}}}\n"
            ),
        );

        let map = codex_cwd_map(&roots).expect("map");

        assert_eq!(map.rejected_headers, 1);
        assert!(map.resolve("SID").is_none());
    }

    #[test]
    fn conflicting_cwd_for_one_id_is_excluded_whichever_order_it_is_seen_in() {
        let first = "/home/alaz/dev/first";
        let second = "/home/alaz/dev/second";

        let mut forward = CodexCwdMap::default();
        forward.register("SID", first, true);
        forward.register("SID", second, true);

        let mut reverse = CodexCwdMap::default();
        reverse.register("SID", second, true);
        reverse.register("SID", first, true);

        assert!(forward.resolve("SID").is_none(), "first-seen must not win");
        assert!(reverse.resolve("SID").is_none(), "order must not decide");
        assert_eq!(forward.primary_keys(), 0);
        assert_eq!(reverse.primary_keys(), 0);
    }

    #[test]
    fn identical_id_cwd_pairs_deduplicate_instead_of_conflicting() {
        let mut map = CodexCwdMap::default();
        map.register("SID", "/home/alaz/dev/ai", true);
        map.register("SID", "/home/alaz/dev/ai", true);

        assert_eq!(
            map.resolve("SID"),
            Some(("/home/alaz/dev/ai".to_string(), CwdMatch::ById))
        );
        assert_eq!(map.primary_keys(), 1);
        assert!(map.ambiguous_ids == 0);
    }

    #[test]
    fn an_id_and_an_alias_pointing_at_different_cwds_are_both_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        // `SHARED` appears as `payload.id` in one rollout and as `payload.session_id`
        // in another, with a different cwd: an alias conflict, not a coincidence.
        write(
            &roots.codex_sessions().join("2026/01/01/rollout-a.jsonl"),
            &session_meta("SHARED", Some("OTHER-A"), "/home/alaz/dev/first"),
        );
        write(
            &roots.codex_sessions().join("2026/01/01/rollout-b.jsonl"),
            &session_meta("OTHER-B", Some("SHARED"), "/home/alaz/dev/second"),
        );

        let map = codex_cwd_map(&roots).expect("map");

        assert!(map.resolve("SHARED").is_none(), "conflicting alias");
        assert_eq!(map.ambiguous_ids, 1);
        // Non-conflicting keys beside it stay usable.
        assert!(map.resolve("OTHER-A").is_some());
        assert!(map.resolve("OTHER-B").is_some());
    }

    #[test]
    fn only_absolute_paths_are_scoped_and_normalization_stays_narrow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let home = temp.path().to_string_lossy().into_owned();

        assert_eq!(
            classify_asserted_path(&format!("{home}/dev/ai"), &roots),
            ProjectScope::Scoped(format!("{home}/dev/ai"))
        );
        assert_eq!(
            classify_asserted_path(&format!("{home}/dev"), &roots),
            ProjectScope::Scoped(format!("{home}/dev"))
        );
        // Trailing slashes normalize; the exact home match still lands on HomeUnscoped.
        assert_eq!(
            classify_asserted_path(&format!("{home}/"), &roots),
            ProjectScope::HomeUnscoped
        );
        assert_eq!(
            classify_asserted_path(&format!("{home}//"), &roots),
            ProjectScope::HomeUnscoped
        );

        // Relative, dot, parent, and empty values are never promoted to a project.
        assert_eq!(
            classify_asserted_path("dev/ai", &roots),
            ProjectScope::Unknown
        );
        assert_eq!(
            classify_asserted_path(&format!("{home}/dev/./ai"), &roots),
            ProjectScope::Unknown
        );
        assert_eq!(
            classify_asserted_path(&format!("{home}/dev/../ai"), &roots),
            ProjectScope::Unknown
        );
        assert_eq!(classify_asserted_path("", &roots), ProjectScope::Unknown);
        assert_eq!(classify_asserted_path("   ", &roots), ProjectScope::Unknown);
        assert_eq!(
            classify_asserted_path(&format!("  {home}/dev/ai  "), &roots),
            ProjectScope::Scoped(format!("{home}/dev/ai"))
        );
    }

    #[test]
    fn interior_double_slashes_collapse_because_posix_resolves_them_as_one() {
        assert_eq!(
            normalize_path("/home//alaz"),
            Some("/home/alaz".to_string())
        );
        assert_eq!(
            normalize_path("/home/alaz//dev/"),
            Some("/home/alaz/dev".to_string())
        );
        assert_eq!(normalize_path("//"), Some("/".to_string()));
        assert_eq!(normalize_path("/"), Some("/".to_string()));
        // Collapsing runs never resurrects a component the contract rejects.
        assert_eq!(normalize_path("/home//../alaz"), None);
    }

    #[test]
    fn archived_rollouts_are_discovered_as_well_as_active_ones() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        write(
            &roots
                .codex_archived_sessions()
                .join("2025/01/01/rollout-old.jsonl"),
            &session_meta("OLD", Some("OLD"), "/home/alaz/dev/archived"),
        );

        let map = codex_cwd_map(&roots).expect("map");

        assert_eq!(
            map.resolve("OLD").map(|(cwd, _)| cwd),
            Some("/home/alaz/dev/archived".to_string())
        );
    }

    #[test]
    fn filename_stems_are_diagnostics_only_and_never_authoritative() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        // A stem carrying a *different* UUID: a real disagreement worth reporting.
        write(
            &roots.codex_sessions().join(
                "2026/01/01/rollout-2026-10-05T18-26-07-00000000-0000-4000-8000-000000000000.jsonl",
            ),
            &session_meta("REAL-ID", Some("REAL-ID"), "/home/alaz/dev/ai"),
        );
        // A stem whose trailing UUID *is* the payload id: agreeing, not a finding.
        write(
            &roots.codex_sessions().join(
                "2026/01/02/rollout-2026-10-05T18-26-07-11111111-1111-4111-8111-111111111111.jsonl",
            ),
            &session_meta(
                "11111111-1111-4111-8111-111111111111",
                None,
                "/home/alaz/dev/other",
            ),
        );
        // A stem with nothing UUID-shaped is not comparable, so not a disagreement.
        write(
            &roots
                .codex_sessions()
                .join("2026/01/03/misleading-stem.jsonl"),
            &session_meta("PLAIN-ID", None, "/home/alaz/dev/plain"),
        );

        let map = codex_cwd_map(&roots).expect("map");

        assert_eq!(
            map.resolve("REAL-ID").map(|(cwd, _)| cwd),
            Some("/home/alaz/dev/ai".to_string()),
            "payload.id is the join key"
        );
        assert!(
            map.resolve("00000000-0000-4000-8000-000000000000")
                .is_none(),
            "an id that exists only in the filename must never resolve"
        );
        assert_eq!(
            map.filename_disagreements, 1,
            "exactly one real mismatch; agreeing and non-comparable filenames are silent"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_rollout_files_are_skipped_instead_of_resolved_through() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        // Target lives *outside* the supported rollout root.
        let outside = temp.path().join("outside-target.jsonl");
        write(
            &outside,
            &session_meta("SID", Some("SID"), "/home/alaz/dev/ai"),
        );
        let link = roots.codex_sessions().join("2026/01/01/rollout-link.jsonl");
        fs::create_dir_all(link.parent().expect("link parent")).expect("link parent");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        let map = codex_cwd_map(&roots).expect("map");

        assert_eq!(map.skipped_non_regular, 1);
        assert_eq!(map.rejected_headers, 0);
        assert!(
            map.resolve("SID").is_none(),
            "the symlink target must never contribute provenance"
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_cwd_is_refused_for_a_symlinked_transcript() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let outside = temp.path().join("outside-transcript.jsonl");
        write(
            &outside,
            &format!(
                "{}\n",
                json!({"type": "assistant", "cwd": "/home/alaz/dev/ai", "message": {"content": "hi"}})
            ),
        );
        let link = roots.claude_projects().join("-proj/sess.jsonl");
        fs::create_dir_all(link.parent().expect("link parent")).expect("link parent");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        assert!(
            claude_cwd(&link).is_none(),
            "a symlinked transcript must not yield provenance"
        );
    }

    #[test]
    fn an_oversized_claude_line_is_skipped_and_scanning_stays_aligned() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let transcript = roots.claude_projects().join("-proj/sess.jsonl");
        // Four times the per-line budget: the second line still has to be found, which
        // only works if the oversized first line is skipped through a fixed buffer.
        let oversized = json!({"type": "assistant", "cwd": "/home/alaz/dev/too-big", "blob": "A".repeat(CLAUDE_LINE_MAX_BYTES * 4)});
        let usable = json!({"type": "assistant", "cwd": "/home/alaz/dev/real", "message": {"content": "hi"}});
        write(&transcript, &format!("{oversized}\n{usable}\n"));

        assert_eq!(
            claude_cwd(&transcript).as_deref(),
            Some("/home/alaz/dev/real")
        );
    }

    /// The exact shape that broke: an oversized line whose **terminator fits the
    /// budget**. The reader is left on the next line, so an unconditional skip would
    /// erase it — and that next line is the one carrying `cwd`.
    #[test]
    fn an_oversized_claude_line_that_ends_inside_the_budget_does_not_swallow_the_next_one() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let transcript = roots.claude_projects().join("-proj/sess.jsonl");

        const HEAD: &str =
            "{\"type\": \"assistant\", \"cwd\": \"/home/alaz/dev/too-big\", \"blob\": \"";
        const TAIL: &str = "\"}";
        // Exactly one byte over the line budget, so `content + \n` fills the read
        // budget precisely and the newline is consumed along with it.
        let pad = (CLAUDE_LINE_MAX_BYTES + 1) - (HEAD.len() + TAIL.len());
        let oversized = format!("{HEAD}{}{TAIL}", "A".repeat(pad));
        assert_eq!(oversized.len(), CLAUDE_LINE_MAX_BYTES + 1);
        let usable = json!({"type": "assistant", "cwd": "/home/alaz/dev/real", "message": {"content": "hi"}});
        write(&transcript, &format!("{oversized}\n{usable}\n"));

        assert_eq!(
            claude_cwd(&transcript).as_deref(),
            Some("/home/alaz/dev/real"),
            "the cwd line right after an already-terminated oversized line must survive"
        );
    }

    /// Wraps a reader and records the two observable bounds:
    ///
    /// - `bytes_read` — every byte handed out to the caller, whether it arrived
    ///   through `read()` or through `fill_buf()`/`consume()`. This is the number
    ///   that proves a read stopped at its budget.
    /// - `max_fill_len` — the largest single slice ever returned by `fill_buf`, which
    ///   is what a walking consumer actually touches. A consumer that never sees more
    ///   than the fixed capacity cannot have been handed the whole line.
    struct CountingReader<R> {
        inner: R,
        bytes_read: usize,
        max_fill_len: usize,
    }

    impl<R: Read> CountingReader<R> {
        fn new(inner: R) -> Self {
            Self {
                inner,
                bytes_read: 0,
                max_fill_len: 0,
            }
        }
    }

    impl<R: Read> Read for CountingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(buf)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    impl<R: BufRead> BufRead for CountingReader<R> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            let available = self.inner.fill_buf()?;
            self.max_fill_len = self.max_fill_len.max(available.len());
            Ok(available)
        }

        fn consume(&mut self, amount: usize) {
            self.inner.consume(amount);
            self.bytes_read += amount;
        }
    }

    fn session_meta_line(id: &str, cwd: &str) -> String {
        format!(
            "{}\n",
            json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd}})
        )
    }

    #[test]
    fn an_oversized_line_stops_reading_at_the_budget_and_a_skip_stays_fixed_size() {
        // Line 1 is 16x the header budget; line 2 is a valid `session_meta`.
        let mut bytes = "A".repeat(HEADER_MAX_BYTES * 16).into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(session_meta_line("sid-1", "/home/alaz/dev/ai").as_bytes());

        let mut reader =
            CountingReader::new(BufReader::with_capacity(4096, io::Cursor::new(bytes)));
        let mut buffer = Vec::new();

        let outcome =
            next_line_bounded(&mut reader, &mut buffer, HEADER_MAX_BYTES).expect("first line");
        let BoundedLine::Oversized { line_complete } = outcome else {
            panic!("expected the oversized line to be reported, got {outcome:?}");
        };
        assert!(
            !line_complete,
            "a line cut by the budget mid-way must report itself as still open"
        );
        assert!(
            reader.bytes_read <= HEADER_MAX_BYTES + 2,
            "reading must stop at the budget, consumed {} bytes instead",
            reader.bytes_read
        );
        assert!(
            buffer.len() <= HEADER_MAX_BYTES + 2,
            "the oversized content must never be materialized, buffer holds {} bytes",
            buffer.len()
        );

        // The skip is where an unbounded drain would show up: it must only ever walk
        // the reader's own fixed buffer, never an allocation that grows with the line.
        skip_rest_of_line(&mut reader).expect("skip the oversized line");
        assert!(
            reader.max_fill_len <= 4096,
            "the skip must be bounded by the reader's buffer, observed {} bytes",
            reader.max_fill_len
        );

        // Resynchronisation: the line after the oversized one is still readable.
        let next =
            next_line_bounded(&mut reader, &mut buffer, HEADER_MAX_BYTES).expect("next line");
        let BoundedLine::Line(next) = next else {
            panic!("line 2 must be readable after the skip, got {next:?}");
        };
        let header: serde_json::Value = serde_json::from_str(&next).expect("valid header json");
        assert_eq!(header["type"], "session_meta");
        assert_eq!(header["payload"]["cwd"], "/home/alaz/dev/ai");
    }

    #[test]
    fn skipping_a_line_that_never_ends_still_uses_a_fixed_buffer() {
        // No newline anywhere: the skip has to reach EOF without materializing it.
        let bytes = "A".repeat(HEADER_MAX_BYTES * 16).into_bytes();
        let mut reader =
            CountingReader::new(BufReader::with_capacity(4096, io::Cursor::new(bytes)));
        let mut buffer = Vec::new();

        let first =
            next_line_bounded(&mut reader, &mut buffer, HEADER_MAX_BYTES).expect("first line");
        assert_eq!(
            first,
            BoundedLine::Oversized {
                line_complete: false
            },
            "a newline-free over-budget line must be reported as oversized and still open"
        );
        assert!(
            reader.bytes_read <= HEADER_MAX_BYTES + 2,
            "reading must stop at the budget even without a newline, consumed {} bytes",
            reader.bytes_read
        );

        skip_rest_of_line(&mut reader).expect("skip reaches EOF");
        assert!(
            reader.max_fill_len <= 4096,
            "the skip must be bounded by the reader's buffer, observed {} bytes",
            reader.max_fill_len
        );

        assert!(
            matches!(
                next_line_bounded(&mut reader, &mut buffer, HEADER_MAX_BYTES).expect("eof"),
                BoundedLine::Eof
            ),
            "the reader must report EOF after the unterminated line"
        );
    }

    /// Walk a whole input the way a scanning caller is contracted to: skip **only**
    /// when the oversized line reports itself as still open.
    fn read_skipping_oversized(reader: &mut impl BufRead, max_bytes: usize) -> Vec<String> {
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        loop {
            match next_line_bounded(reader, &mut buffer, max_bytes).expect("bounded read") {
                BoundedLine::Eof => break,
                BoundedLine::Line(line) => lines.push(line),
                BoundedLine::Oversized { line_complete } => {
                    if !line_complete {
                        skip_rest_of_line(reader).expect("skip an open line");
                    }
                }
            }
        }
        lines
    }

    /// The exact boundary, LF and CRLF and end of file: whether the terminator was
    /// already consumed decides whether the caller may skip. Skipping a line that
    /// already terminated deletes the *next* line — that is the regression this pins.
    #[test]
    fn line_complete_separates_a_consumed_newline_from_a_line_still_open() {
        const LIMIT: usize = 8;
        let cases: &[(&str, BoundedLine, &[&str])] = &[
            // At the limit: nothing oversized, the terminator is consumed.
            (
                "12345678\nNEXT\n",
                BoundedLine::Line("12345678".to_string()),
                &["12345678", "NEXT"],
            ),
            (
                "12345678\r\nNEXT\n",
                BoundedLine::Line("12345678".to_string()),
                &["12345678", "NEXT"],
            ),
            (
                "12345678",
                BoundedLine::Line("12345678".to_string()),
                &["12345678"],
            ),
            // One byte over, LF inside the budget: the terminator is already consumed,
            // the reader sits on NEXT, and an unconditional skip would erase it.
            (
                "123456789\nNEXT\n",
                BoundedLine::Oversized {
                    line_complete: true,
                },
                &["NEXT"],
            ),
            // One byte over, CRLF split by the budget: the LF is still in the stream,
            // so the line is open and must be skipped before the next read.
            (
                "123456789\r\nNEXT\n",
                BoundedLine::Oversized {
                    line_complete: false,
                },
                &["NEXT"],
            ),
            // Over-budget content with no terminator: still open until end of file.
            (
                "123456789",
                BoundedLine::Oversized {
                    line_complete: true,
                },
                &[],
            ),
            (
                "123456789\r\n",
                BoundedLine::Oversized {
                    line_complete: false,
                },
                &[],
            ),
        ];

        for (input, expected_first, expected_lines) in cases {
            let first = {
                let mut reader = BufReader::with_capacity(4, io::Cursor::new(input.as_bytes()));
                let mut buffer = Vec::new();
                next_line_bounded(&mut reader, &mut buffer, LIMIT).expect("first line")
            };
            assert_eq!(&first, expected_first, "first read of {input:?}");

            let expected: Vec<String> = expected_lines
                .iter()
                .map(|line| (*line).to_string())
                .collect();
            let mut reader = BufReader::with_capacity(4, io::Cursor::new(input.as_bytes()));
            assert_eq!(
                read_skipping_oversized(&mut reader, LIMIT),
                expected,
                "contract walk of {input:?}"
            );
        }
    }
}
