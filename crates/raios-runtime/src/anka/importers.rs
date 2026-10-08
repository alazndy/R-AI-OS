//! Harness importers: stream sources, classify provenance, count outcomes.
//!
//! Every importer streams its source and records how many entries were malformed,
//! empty, or unsupported. Entry contents are never written to logs or counters —
//! only counts are carried.

use super::provenance::{
    claude_cwd, claude_cwd_from_content, codex_cwd_map, next_line_bounded, open_regular_file,
    skip_rest_of_line, AnkaRoots, BoundedLine, CwdMatch, Provenance, TimeSource,
    HISTORY_LINE_MAX_BYTES,
};
use super::{modified_at, record, AnkaRecord, RecordSpec};
use anyhow::{Context, Result};
use raios_core::anka::AnkaHarness;
use raios_core::security::redact_secrets;
use serde_json::Value;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Outcome counts for one import pass. Carried, never logged with content.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    /// Source files that yielded at least one record.
    pub sources: usize,
    pub accepted: usize,
    pub malformed: usize,
    pub empty: usize,
    pub unsupported: usize,
    /// Entries skipped because they are not regular files (symlinked transcripts).
    /// Never resolved through, so a link planted in a history tree cannot redirect a read.
    pub skipped_non_regular: usize,
    /// Entries whose source line exceeded [`HISTORY_LINE_MAX_BYTES`], or whose
    /// whole file exceeded [`TRANSCRIPT_MAX_BYTES`]. The content is skipped
    /// through a fixed bound and never materialized, so it counts as its own
    /// outcome rather than as malformed or empty.
    pub oversized: usize,
}

/// Hard ceiling for one transcript's whole-file read in [`discover_claude`].
/// Every other read in the import path is line-bounded; this is the only
/// whole-file read and it runs on the least-validated input (any `.jsonl` the
/// walk accepted), so a hostile or corrupt multi-gigabyte transcript would
/// otherwise be fully materialized — and the daily refresh timer runs under
/// `MemoryMax=512M`, turning such a file into a silent OOM-kill. Past the cap
/// the file counts as [`ImportReport::oversized`]: refused whole, never read.
pub const TRANSCRIPT_MAX_BYTES: u64 = 32 * 1024 * 1024;

pub fn discover_claude(roots: &AnkaRoots) -> Result<(Vec<AnkaRecord>, ImportReport)> {
    let root = roots.claude_projects();
    let mut report = ImportReport::default();
    let mut records = Vec::new();
    if !root.exists() {
        return Ok((records, report));
    }

    // Depth 4 reaches `<slug>/<session>/subagents/<agent>.jsonl`; depth 2 is the
    // normal `<slug>/<session>.jsonl`.
    for entry in WalkDir::new(&root)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        // `entry.file_type()` is the symlink's own type, whereas `Path::is_file()`
        // resolves a symlink whose target is a regular file. Checking the entry type
        // is what actually keeps a symlinked transcript out of both the index and the
        // `cwd` scan below; `follow_links(false)` alone only stops traversal from
        // *entering* a symlinked directory.
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        if entry.file_type().is_symlink() {
            report.skipped_non_regular += 1;
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        let components = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let Some(project_slug) = components.first().cloned() else {
            continue;
        };

        // `<slug>/<session>.jsonl` -> 2 components; anything deeper must be the
        // documented `subagents` layout or it is counted unsupported.
        let (session_id, parent_session, agent_id) = match components.len() {
            2 => (file_stem(&components[1]), None, None),
            len if len >= 4 && components[2] == "subagents" => (
                format!("{}:{}", components[1], file_stem(&components[3])),
                Some(components[1].clone()),
                Some(file_stem(&components[3])),
            ),
            _ => {
                report.unsupported += 1;
                continue;
            }
        };

        // One verified descriptor per transcript: `open_regular_file` (O_NOFOLLOW
        // + fstat identity) and a single read feed BOTH the extracted content and
        // the `cwd` provenance below, so a file swapped in after this walk's
        // stat can never contribute one file's bytes as another file's label —
        // the exact window two independent path opens used to leave open.
        let mut raw = match open_regular_file(path) {
            Ok(file) => file,
            // The transcript vanished between the walk and the open: the same
            // nothing-to-import outcome an unreadable path has always had here.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                report.empty += 1;
                continue;
            }
            // Refused as no longer the regular file the walk saw — a symlink
            // swapped in, a different file swapped in, or unreadable as a
            // regular file at all: skipped wholesale, never half-read.
            Err(_) => {
                report.skipped_non_regular += 1;
                continue;
            }
        };
        // The ceiling comes from the verified descriptor itself — the same open
        // that feeds the read below — so the size check and the read share one
        // file and there is no window between them.
        match raw.metadata() {
            Ok(metadata) if metadata.len() > TRANSCRIPT_MAX_BYTES => {
                report.oversized += 1;
                continue;
            }
            Ok(_) => {}
            Err(_) => {
                report.skipped_non_regular += 1;
                continue;
            }
        }
        let mut bytes = String::new();
        if raw.read_to_string(&mut bytes).is_err() {
            report.empty += 1;
            continue;
        }
        let content = crate::session_memory::extract_transcript_content(&bytes);
        if content.trim().is_empty() {
            report.empty += 1;
            continue;
        }

        let provenance = match claude_cwd_from_content(&bytes) {
            // `cwd` written by this transcript's own records: harness metadata.
            Some(cwd) => Provenance::direct(&cwd, roots).with_slug(&project_slug),
            // A subagent may inherit provenance from the session that spawned it.
            None => match parent_session.as_ref().and_then(|session| {
                claude_cwd(&root.join(&project_slug).join(format!("{session}.jsonl")))
            }) {
                Some(cwd) => Provenance::session_cwd(&cwd, roots, "parent_session.cwd")
                    .with_slug(&project_slug),
                None => Provenance::from_slug(&project_slug),
            },
        };

        report.sources += 1;
        report.accepted += 1;
        records.push(record(RecordSpec {
            harness: AnkaHarness::Claude,
            project: project_slug.clone(),
            identity_project: project_slug,
            session_id,
            occurred_at: modified_at(path),
            content,
            provenance: provenance.with_time(TimeSource::FilesystemObservation),
            parent_session,
            agent_id,
            discriminator: relative.to_string_lossy().into_owned(),
            prompt_fingerprint: false,
        }));
    }
    Ok((records, report))
}

pub fn discover_codex(roots: &AnkaRoots) -> Result<(Vec<AnkaRecord>, ImportReport)> {
    let cwd_map = codex_cwd_map(roots)?;
    discover_history(
        &roots.codex_history(),
        AnkaHarness::Codex,
        "codex-history",
        |value| {
            let text = pick_text(value, &["text", "input"])?;
            Some((text.to_string(), timestamp(value, "ts")))
        },
        |value| {
            match nonempty(value, "session_id").and_then(|id| cwd_map.resolve(id)) {
                Some((cwd, CwdMatch::ById)) => {
                    Provenance::session_cwd(&cwd, roots, "session_meta.payload.id")
                }
                Some((cwd, CwdMatch::BySessionId)) => {
                    Provenance::session_cwd(&cwd, roots, "session_meta.payload.session_id")
                }
                // Unresolvable session: project is unknown, never the fallback label.
                None => Provenance::unknown(),
            }
        },
    )
}

pub fn discover_opencode(roots: &AnkaRoots) -> Result<(Vec<AnkaRecord>, ImportReport)> {
    discover_history(
        &roots.opencode_history(),
        AnkaHarness::Opencode,
        "opencode-history",
        |value| {
            // `text` is the current shape; `input` is the legacy fallback. A blank
            // `text` must not mask a populated `input`, so the pick is by first
            // *non-blank* key rather than by first present key.
            let text = pick_text(value, &["text", "input"])?;
            Some((text.to_string(), timestamp(value, "timestamp")))
        },
        |value| match nonempty(value, "project") {
            // A present project is metadata, not an inference from prompt text.
            Some(project) => Provenance::direct(project, roots).with_field("project"),
            // Absent project stays unknown: the fallback display label must not
            // promote an unscoped prompt to a scoped one.
            None => Provenance::unknown(),
        },
    )
}

pub fn discover_antigravity(roots: &AnkaRoots) -> Result<(Vec<AnkaRecord>, ImportReport)> {
    discover_history(
        &roots.antigravity_history(),
        AnkaHarness::Antigravity,
        "antigravity-history",
        |value| {
            let text = pick_text(value, &["display", "input"])?;
            Some((text.to_string(), timestamp(value, "timestamp")))
        },
        |value| match nonempty(value, "workspace") {
            Some(workspace) => Provenance::direct(workspace, roots).with_field("workspace"),
            None => Provenance::unknown(),
        },
    )
}

/// Stream one prompt-history file.
///
/// `extract` decides what a supported entry looks like; returning `None` counts the
/// line as unsupported. `provenance` must derive scope from metadata only.
fn discover_history<E, P>(
    path: &Path,
    harness: AnkaHarness,
    fallback_project: &'static str,
    extract: E,
    provenance: P,
) -> Result<(Vec<AnkaRecord>, ImportReport)>
where
    E: Fn(&Value) -> Option<(String, Option<u64>)>,
    P: Fn(&Value) -> Provenance,
{
    let mut report = ImportReport::default();
    let mut records = Vec::new();
    // The history root is a fixed, harness-declared path — but opening it still goes
    // through the regular-file check so a symlink planted there is reported rather
    // than silently resolved into an unrelated file.
    let file = match open_regular_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((records, report));
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            report.skipped_non_regular += 1;
            return Ok((records, report));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };
    report.sources = 1;

    let source_file = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("history")
        .to_string();

    // Bounded streaming: each line is read through `HISTORY_LINE_MAX_BYTES`, so one
    // pathological line can never become an allocation. An oversized line is skipped
    // through a fixed buffer, counted separately, and never parsed.
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut index = 0usize;
    loop {
        let bounded = match next_line_bounded(&mut reader, &mut buffer, HISTORY_LINE_MAX_BYTES) {
            Ok(BoundedLine::Eof) => break,
            Ok(other) => other,
            Err(_) => {
                // A read failure is a property of the file, not of one entry: there is
                // no trustworthy way to keep streaming past it.
                report.malformed += 1;
                break;
            }
        };
        // Counted for every line, oversized ones included, so discriminators keep
        // matching source line numbers.
        index += 1;

        let line = match bounded {
            BoundedLine::Line(line) => line,
            BoundedLine::Oversized { line_complete } => {
                report.oversized += 1;
                if !line_complete && skip_rest_of_line(&mut reader).is_err() {
                    break;
                }
                continue;
            }
            BoundedLine::Eof => break,
        };

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            report.malformed += 1;
            continue;
        };
        let Some((text, event_time)) = extract(&value) else {
            report.unsupported += 1;
            continue;
        };
        if text.trim().is_empty() {
            report.empty += 1;
            continue;
        }

        let (occurred_at, time_source) = match event_time {
            Some(seconds) => (seconds.to_string(), TimeSource::EventTimestamp),
            None => (modified_at(path), TimeSource::FilesystemObservation),
        };

        let provenance = provenance(&value).with_time(time_source);
        let display = provenance.scope.as_display();
        let project = if display.is_empty() {
            fallback_project.to_string()
        } else {
            display
        };
        // Native session ids are preserved for display; the line number is carried
        // only as the record discriminator so distinct prompts never share an id.
        let session_id = nonempty(&value, "session_id")
            .map(str::to_string)
            .unwrap_or_else(|| source_file.clone());

        report.accepted += 1;
        records.push(record(RecordSpec {
            harness: harness.clone(),
            project,
            identity_project: source_file.clone(),
            session_id,
            occurred_at,
            content: text,
            provenance,
            parent_session: None,
            agent_id: None,
            discriminator: index.to_string(),
            prompt_fingerprint: true,
        }));
    }
    Ok((records, report))
}

/// Identity inputs recovered for a record written by a pre-`forget_key` cache.
pub(super) struct RecoveredHistoryIdentity {
    /// History file stem — the same namespace `discover_history` derives today.
    pub identity_project: String,
    /// The entry's native `session_id`, or the file stem when it has none.
    pub session_id: String,
}

/// Recover the identity inputs a pre-identity cache did not store for a
/// prompt-history record.
///
/// The legacy cache recorded `session_id` as `"<file-stem>:<line>"` and dropped the
/// entry's native `session_id` — which the current importer does use — so the source
/// line is the only place that mapping can be verified. The line is re-read through
/// the same byte bound as the importer, the redacted prompt must still equal the
/// content the cache stored (a rotated or edited file shifts line numbers, and
/// identity derived from the wrong line would silently stop hiding the record after
/// the next rebuild), and the line must be shown to be the *cached event*: a Codex
/// entry must carry `ts` equal to the timestamp the cache recorded
/// (`expected_occurred_at`), while an OpenCode/Antigravity entry may omit its
/// timestamp only when it also omits a native `session_id` — the derived key is
/// then the fixed file-stem namespace plus the prompt group, identical for every
/// line of the file, so no wrong event can be named even though the event itself
/// is unverifiable. A native session with no timestamp cannot be tied back to the
/// cached event and stops the mapping; mtime is never event evidence. Only then is
/// the native session recovered — or the file stem when the entry has none,
/// exactly like `discover_history`.
///
/// `Ok(None)` means the mapping is unverifiable: Claude (never a history source),
/// a malformed legacy session, a wrong stem, a missing or moved line, an oversized
/// or unparseable entry, a content mismatch, a timestamp that does not identify
/// the cached event, or a native session that cannot be tied to it. `Err` is a
/// real I/O failure opening the file, which is worth surfacing on its own.
pub(super) fn recover_history_identity(
    roots: &AnkaRoots,
    harness: &AnkaHarness,
    legacy_session: &str,
    expected_content: &str,
    expected_occurred_at: &str,
) -> Result<Option<RecoveredHistoryIdentity>> {
    if matches!(harness, AnkaHarness::Claude) {
        return Ok(None);
    }
    let path = history_path(roots, harness);
    let Some((stem, line_number)) = legacy_session.rsplit_once(':') else {
        return Ok(None);
    };
    let Ok(line_number) = line_number.parse::<usize>() else {
        return Ok(None);
    };
    if line_number == 0 {
        return Ok(None);
    }
    let source_file = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("history");
    if stem != source_file {
        return Ok(None);
    }

    let file = match open_regular_file(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };

    // Same bounded streaming as `discover_history`, and the same counting: every
    // line — oversized included — advances the index, so the legacy line number
    // lands on the same entry the old importer counted.
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut index = 0usize;
    loop {
        let bounded = match next_line_bounded(&mut reader, &mut buffer, HISTORY_LINE_MAX_BYTES) {
            Ok(BoundedLine::Eof) => break,
            Ok(other) => other,
            Err(_) => return Ok(None),
        };
        index += 1;
        match bounded {
            BoundedLine::Line(line) => {
                if index == line_number {
                    return Ok(recover_from_line(
                        harness,
                        source_file,
                        &line,
                        expected_content,
                        expected_occurred_at,
                    ));
                }
            }
            BoundedLine::Oversized { line_complete } => {
                // The target line itself cannot be verified through the bound; treat
                // the mapping as unproven rather than guessed.
                if index == line_number {
                    return Ok(None);
                }
                if !line_complete && skip_rest_of_line(&mut reader).is_err() {
                    return Ok(None);
                }
            }
            BoundedLine::Eof => break,
        }
    }
    // The line no longer exists (rotation/truncation): unverifiable.
    Ok(None)
}

/// Verify one history line against the cached record and recover its session.
///
/// Three facts have to hold before a line may stand for the cached event:
/// the prompt text, the event identity, and (implicitly) the line's position —
/// and text alone is not enough. After a rotation the *same line* can hold
/// another session's entry with an identical prompt, which would derive a key
/// for the wrong event and quietly stop hiding the record after the next rebuild.
/// The event is identified by its timestamp where the harness records one; when
/// the timestamp is absent the line may still stand for the event only if it
/// claims nothing event-specific (no native `session_id`), because the derived
/// key is then the fixed file-stem namespace plus the prompt group either way.
fn recover_from_line(
    harness: &AnkaHarness,
    source_file: &str,
    line: &str,
    expected_content: &str,
    expected_occurred_at: &str,
) -> Option<RecoveredHistoryIdentity> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let text = history_text(harness, &value)?;
    if text.trim().is_empty() {
        return None;
    }
    if redact_secrets(text) != expected_content {
        return None;
    }
    // Event-timestamp verification: the cache stored this line's event time as
    // `occurred_at` (`discover_history` prefers it over the file's mtime), so a
    // line that disagrees is a *different event* — stop, never derive from it.
    match harness {
        AnkaHarness::Claude => return None, // never a history source; guarded by the caller
        AnkaHarness::Codex => {
            // Codex history entries always carry `ts`, so a line without one cannot
            // be shown to be the cached event either: undistinguishable, stop.
            let seconds = timestamp(&value, "ts")?;
            if seconds.to_string() != expected_occurred_at {
                return None;
            }
        }
        AnkaHarness::Opencode | AnkaHarness::Antigravity => {
            match timestamp(&value, "timestamp") {
                // A present timestamp that disagrees is a *different event* — stop.
                Some(seconds) if seconds.to_string() == expected_occurred_at => {}
                Some(_) => return None,
                None => {
                    // No timestamp to compare: a legacy cache may predate
                    // timestamped entries (the old importer fell back to the file's
                    // mtime), so absence alone is not proof of a different event —
                    // but only when the line claims nothing event-specific. Without
                    // a native `session_id` the derived key is the fixed file-stem
                    // namespace plus the prompt group, identical for every line in
                    // the file, so no wrong event can be named. With a native
                    // session the key would name a session this line cannot be tied
                    // back to the cached event: stop instead of guessing which
                    // session the old record belongs to. mtime is never event
                    // evidence either way.
                    if nonempty(&value, "session_id").is_some() {
                        return None;
                    }
                }
            }
        }
    }
    let session_id = nonempty(&value, "session_id")
        .map(str::to_string)
        .unwrap_or_else(|| source_file.to_string());
    Some(RecoveredHistoryIdentity {
        identity_project: source_file.to_string(),
        session_id,
    })
}

/// Prompt text per harness — the same key selection the `discover_*` extractors use.
fn history_text<'a>(harness: &AnkaHarness, value: &'a Value) -> Option<&'a str> {
    match harness {
        AnkaHarness::Codex | AnkaHarness::Opencode => pick_text(value, &["text", "input"]),
        AnkaHarness::Antigravity => pick_text(value, &["display", "input"]),
        AnkaHarness::Claude => None,
    }
}

/// The prompt-history file a harness reads — same paths the importers use.
fn history_path(roots: &AnkaRoots, harness: &AnkaHarness) -> PathBuf {
    match harness {
        AnkaHarness::Codex => roots.codex_history(),
        AnkaHarness::Opencode => roots.opencode_history(),
        AnkaHarness::Antigravity => roots.antigravity_history(),
        // Never a history source: `recover_history_identity` returns before calling.
        AnkaHarness::Claude => roots.claude_projects(),
    }
}

/// First **non-blank** string among `keys`.
///
/// Shape detection and content quality stay separate in both directions:
/// - a key that exists but is blank does not mask a populated legacy fallback;
/// - when every present key is blank this still returns `Some("")`, so the entry is
///   counted `empty` instead of being miscounted `unsupported`;
/// - when no key exists at all it returns `None` (unsupported shape).
fn pick_text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let mut present = false;
    for key in keys {
        if let Some(found) = value.get(key).and_then(|field| field.as_str()) {
            present = true;
            if !found.trim().is_empty() {
                return Some(found);
            }
        }
    }
    present.then_some("")
}

/// Non-empty trimmed metadata string, or `None` when absent or blank.
fn nonempty<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Validated optional event timestamp: a positive integer is required.
fn timestamp(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(|value| value.as_u64())
        .filter(|seconds| *seconds > 0)
}

fn file_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::provenance::{ProjectScope, ProvenanceQuality, TimeSource};
    use super::*;
    use serde_json::json;
    use std::fs;

    /// Synthetic roots only: no fixture may read real history or the real cache.
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

    fn claude_line(cwd: &str, text: &str) -> String {
        format!(
            "{}\n",
            json!({"type": "assistant", "cwd": cwd, "message": {"content": text}})
        )
    }

    #[test]
    fn opencode_parses_current_and_legacy_shapes_and_counts_the_rest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        write(
            &roots.opencode_history(),
            concat!(
                r#"{"text":"current shape"}"#,
                "\n",
                r#"{"input":"legacy shape"}"#,
                "\n",
                "{not json",
                "\n",
                r#"{"text":"   "}"#,
                "\n",
            ),
        );

        let (records, report) = discover_opencode(&roots).expect("opencode import");

        assert_eq!(
            (
                report.sources,
                report.accepted,
                report.malformed,
                report.empty,
                report.unsupported,
            ),
            (1, 2, 1, 1, 0)
        );
        assert_eq!(records.len(), 2);
        // Both prompts carry no project metadata, so neither may be promoted to scoped
        // by the fallback display label.
        for record in &records {
            assert_eq!(record.provenance.scope, ProjectScope::Unknown);
            assert_eq!(record.provenance.quality, ProvenanceQuality::Unknown);
            assert_eq!(record.provenance.observed_via, "none");
            assert_eq!(record.source.project, "opencode-history");
            // No event timestamp exists in this shape.
            assert_eq!(
                record.provenance.time_source,
                TimeSource::FilesystemObservation
            );
        }
        // Distinct prompts never share a record id.
        assert_ne!(records[0].id, records[1].id);
    }

    /// One history line past the budget: counted on its own, never parsed, skipped
    /// through a fixed buffer - and the lines after it keep their original source line
    /// numbers. Two controls pin that: a *malformed* line 2 must yield the exact same
    /// id for line 3, and dropping line 2 entirely must yield a different one.
    #[test]
    fn an_oversized_history_line_is_counted_and_the_lines_after_it_survive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let path = roots.opencode_history();
        let first = r#"{"timestamp":1700000000,"text":"first"}"#;
        let third = r#"{"timestamp":1700000003,"text":"third"}"#;
        let oversized = format!(
            "{}{}{}",
            r#"{"text":"#,
            "A".repeat(HISTORY_LINE_MAX_BYTES),
            r#"}"#
        );
        assert!(oversized.len() > HISTORY_LINE_MAX_BYTES);

        write(
            &path,
            &format!(
                "{first}
{oversized}
{third}
"
            ),
        );
        let (records, report) = discover_opencode(&roots).expect("oversized run");
        assert_eq!(
            (report.oversized, report.accepted, report.malformed),
            (1, 2, 0),
            "the oversized line is its own outcome, not malformed"
        );
        let contents: Vec<&str> = records
            .iter()
            .map(|record| record.content.as_str())
            .collect();
        assert_eq!(contents, ["first", "third"]);
        let after_oversized = records[1].id.clone();

        write(
            &path,
            &format!(
                "{first}
{{not json
{third}
"
            ),
        );
        let (records, report) = discover_opencode(&roots).expect("malformed control");
        assert_eq!(
            (report.oversized, report.accepted, report.malformed),
            (0, 2, 1)
        );
        assert_eq!(
            records[1].id, after_oversized,
            "how line 2 was skipped must not shift the numbering of line 3"
        );

        write(
            &path,
            &format!(
                "{first}
{third}
"
            ),
        );
        let (records, _) = discover_opencode(&roots).expect("absent control");
        assert_ne!(
            records[1].id, after_oversized,
            "an absent line 2 must move line 3 up, so the discriminator is the line number"
        );
    }

    #[test]
    fn opencode_project_metadata_is_used_only_when_present() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        write(
            &roots.opencode_history(),
            concat!(
                r#"{"project":"/home/alaz/dev/ai/svp","text":"scoped"}"#,
                "\n",
                r#"{"text":"unscoped"}"#,
                "\n",
            ),
        );

        let (records, _) = discover_opencode(&roots).expect("opencode import");

        assert_eq!(records[0].provenance.quality, ProvenanceQuality::Direct);
        assert_eq!(
            records[0].provenance.scope,
            ProjectScope::Scoped("/home/alaz/dev/ai/svp".to_string())
        );
        assert_eq!(records[1].provenance.scope, ProjectScope::Unknown);
    }

    #[test]
    fn claude_nested_agents_keep_the_owning_project_and_parent_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();
        let cwd = "/home/alaz/dev/demo";

        write(
            &root.join("-home-alaz-dev-demo").join("sess-a.jsonl"),
            &claude_line(cwd, "parent turn"),
        );
        // Identical agent filename in two different sessions.
        write(
            &root
                .join("-home-alaz-dev-demo")
                .join("sess-a")
                .join("subagents")
                .join("agent-1.jsonl"),
            &claude_line(cwd, "agent work a"),
        );
        write(
            &root
                .join("-home-alaz-dev-other")
                .join("sess-b")
                .join("subagents")
                .join("agent-1.jsonl"),
            &claude_line(cwd, "agent work b"),
        );

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(report.sources, 3);
        assert_eq!(records.len(), 3);

        let agents = records
            .iter()
            .filter(|record| record.agent_id.is_some())
            .collect::<Vec<_>>();
        assert_eq!(agents.len(), 2);

        // Owning project is the first component under `.claude/projects`, never `subagents`.
        let projects = agents
            .iter()
            .map(|record| record.source.project.as_str())
            .collect::<Vec<_>>();
        assert!(projects.contains(&"-home-alaz-dev-demo"));
        assert!(projects.contains(&"-home-alaz-dev-other"));

        // Equal filenames in different sessions must not collide.
        assert_ne!(agents[0].id, agents[1].id);
        assert_ne!(agents[0].source.session_id, agents[1].source.session_id);

        for agent in &agents {
            let parent = agent.parent_session.as_deref().expect("parent session");
            assert!(agent.source.session_id.starts_with(parent));
            assert_eq!(agent.provenance.quality, ProvenanceQuality::Direct);
            assert_eq!(
                agent.provenance.scope,
                ProjectScope::Scoped(cwd.to_string())
            );
            // The raw slug is retained beside the resolved path so policy can match
            // either without the slug being read as a path.
            assert_eq!(
                agent.provenance.slug.as_deref(),
                Some(agent.source.project.as_str())
            );
        }
    }

    #[test]
    fn claude_without_cwd_reports_the_raw_slug_and_never_a_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();
        write(
            &root.join("-home-alaz-dev-demo").join("sess-a.jsonl"),
            &claude_line("", "no cwd anywhere"),
        );

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(report.sources, 1);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].provenance.scope,
            ProjectScope::Slug("-home-alaz-dev-demo".to_string())
        );
        assert_eq!(
            records[0].provenance.quality,
            ProvenanceQuality::SlugEncoded
        );
        assert_eq!(records[0].provenance.observed_via, "directory_slug");
        assert_eq!(records[0].source.project, "-home-alaz-dev-demo");
    }

    #[test]
    fn codex_history_resolves_project_from_the_rollout_session_header() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        write(
            &roots
                .codex_sessions()
                .join("2026")
                .join("01")
                .join("01")
                .join("rollout-alpha.jsonl"),
            &format!(
                "{}\n{}\n",
                json!({"type": "session_meta", "payload": {
                    "id": "SID-1", "session_id": "OTHER", "cwd": "/home/alaz/dev/ai/svp"
                }}),
                json!({"type": "message", "payload": {"cwd": "/wrong"}})
            ),
        );
        write(
            &roots.codex_history(),
            &format!(
                "{}\n{}\n",
                json!({"session_id": "SID-1", "text": "first prompt", "ts": 1700000000}),
                json!({"session_id": "UNMATCHED", "text": "second prompt", "ts": 1700000001})
            ),
        );

        let (records, report) = discover_codex(&roots).expect("codex import");

        assert_eq!(report.accepted, 2);
        assert_eq!(records.len(), 2);

        let resolved = records
            .iter()
            .find(|record| record.source.session_id == "SID-1")
            .expect("resolved record");
        assert_eq!(resolved.provenance.quality, ProvenanceQuality::SessionCwd);
        assert_eq!(
            resolved.provenance.scope,
            ProjectScope::Scoped("/home/alaz/dev/ai/svp".to_string())
        );
        assert_eq!(resolved.provenance.observed_via, "session_meta.payload.id");
        assert_eq!(resolved.provenance.time_source, TimeSource::EventTimestamp);
        assert_eq!(resolved.source.project, "/home/alaz/dev/ai/svp");

        let unresolved = records
            .iter()
            .find(|record| record.source.session_id == "UNMATCHED")
            .expect("unresolved record");
        assert_eq!(unresolved.provenance.scope, ProjectScope::Unknown);
        assert_eq!(unresolved.provenance.observed_via, "none");
        // The fallback label is display only; it never claims a project.
        assert_eq!(unresolved.source.project, "codex-history");
    }

    #[test]
    fn home_is_home_unscoped_not_a_ordinary_project() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let home = temp.path().to_string_lossy().into_owned();
        write(
            &roots
                .codex_sessions()
                .join("2026")
                .join("01")
                .join("01")
                .join("rollout-home.jsonl"),
            &format!(
                "{}\n",
                json!({"type": "session_meta", "payload": {"id": "SID-HOME", "cwd": home}})
            ),
        );
        write(
            &roots.codex_history(),
            &format!(
                "{}\n",
                json!({"session_id": "SID-HOME", "text": "unscoped prompt", "ts": 1700000000})
            ),
        );

        let (records, _) = discover_codex(&roots).expect("codex import");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provenance.scope, ProjectScope::HomeUnscoped);
        assert_eq!(records[0].provenance.quality, ProvenanceQuality::SessionCwd);
    }

    #[test]
    fn opencode_blank_text_falls_back_to_the_legacy_input_field() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        write(
            &roots.opencode_history(),
            concat!(
                r#"{"text":"","input":"real legacy prompt"}"#,
                "\n",
                r#"{"text":"","input":""}"#,
                "\n",
                r#"{"input":"only legacy present"}"#,
                "\n",
            ),
        );

        let (records, report) = discover_opencode(&roots).expect("opencode import");

        // A blank `text` must not mask a populated `input`.
        assert_eq!(report.accepted, 2);
        assert_eq!(report.empty, 1, "both blank -> empty, not unsupported");
        assert_eq!(report.unsupported, 0);
        let contents = records
            .iter()
            .map(|r| r.content.as_str())
            .collect::<Vec<_>>();
        assert!(contents.contains(&"real legacy prompt"));
        assert!(contents.contains(&"only legacy present"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_symlinked_transcripts_are_never_indexed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();

        // A real transcript and a symlink to a file outside the projects root.
        write(
            &root.join("-home-alaz-dev-demo").join("sess-real.jsonl"),
            &claude_line("/home/alaz/dev/demo", "real"),
        );
        let outside = temp.path().join("outside-agent.jsonl");
        write(
            &outside,
            &claude_line("/home/alaz/dev/outside", "secret outside"),
        );
        let link = root.join("-home-alaz-dev-demo").join("sess-link.jsonl");
        fs::create_dir_all(link.parent().expect("link parent")).expect("link parent");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        // A symlinked *directory* must not be descended into either.
        let outside_dir = temp.path().join("outside-dir");
        fs::create_dir_all(&outside_dir).expect("outside dir");
        fs::write(
            outside_dir.join("sess-hidden.jsonl"),
            claude_line("/home/alaz/dev/hidden", "hidden"),
        )
        .expect("hidden");
        std::os::unix::fs::symlink(
            &outside_dir,
            root.join("-home-alaz-dev-demo").join("linked-dir"),
        )
        .expect("dir symlink");

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(
            report.skipped_non_regular, 1,
            "only the file symlink counts"
        );
        assert_eq!(records.len(), 1, "symlinks must contribute no records");
        assert_eq!(records[0].source.session_id, "sess-real");
        assert!(records
            .iter()
            .all(|r| !r.content.contains("secret outside")));
        assert!(records.iter().all(|r| !r.content.contains("hidden")));
    }

    /// The walk stats a real transcript, then — inside the open, in the window
    /// no path check can see — the path is swapped for a symlink to an
    /// attacker's file. Content and `cwd` come from one verified descriptor
    /// only, so the transcript is refused wholesale: no record may carry the
    /// original's bytes under the planted file's provenance, or vice versa.
    #[cfg(unix)]
    #[test]
    fn a_transcript_symlinked_between_the_walk_and_the_open_is_never_indexed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();
        let session = root.join("-home-alaz-dev-demo").join("sess-swap.jsonl");
        let planted = temp.path().join("planted.jsonl");
        write(
            &session,
            &claude_line("/home/alaz/dev/trusted", "original content"),
        );
        write(
            &planted,
            &claude_line("/home/alaz/planted", "planted content"),
        );

        let swap_at = session.clone();
        let swap_to = planted.clone();
        super::super::provenance::after_path_check_once(move || {
            let _ = fs::remove_file(&swap_at);
            std::os::unix::fs::symlink(&swap_to, &swap_at).expect("swap to symlink");
        });

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(
            report.skipped_non_regular, 1,
            "the swapped transcript must be refused as non-regular, never half-read"
        );
        assert!(
            records.is_empty(),
            "neither the original bytes nor the planted file may produce a record"
        );
    }

    /// The same window, but the path is replaced by a *different regular
    /// file* — `O_NOFOLLOW` alone cannot see that shape, only the
    /// descriptor-identity comparison (`fstat` dev+ino) can.
    #[cfg(unix)]
    #[test]
    fn a_transcript_replaced_by_another_regular_file_between_the_walk_and_the_open_is_never_indexed(
    ) {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();
        let session = root.join("-home-alaz-dev-demo").join("sess-replace.jsonl");
        let planted = temp.path().join("planted.jsonl");
        write(
            &session,
            &claude_line("/home/alaz/dev/trusted", "original content"),
        );
        write(
            &planted,
            &claude_line("/home/alaz/planted", "planted content"),
        );

        let swap_at = session.clone();
        let swap_to = planted.clone();
        super::super::provenance::after_path_check_once(move || {
            let planted_bytes = fs::read(&swap_to).expect("planted bytes");
            let _ = fs::remove_file(&swap_at);
            fs::write(&swap_at, planted_bytes).expect("swap to another regular file");
        });

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(
            report.skipped_non_regular, 1,
            "a different regular file at the same path must fail the descriptor identity check"
        );
        assert!(
            records.is_empty(),
            "swapped-in content must never be indexed"
        );
    }

    /// The whole-file ceiling refuses before reading: a transcript past
    /// `TRANSCRIPT_MAX_BYTES` must count as `oversized` with its bytes never
    /// materialized — not as `empty`, not as a record.
    #[test]
    fn a_transcript_past_the_whole_file_cap_is_never_materialized() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = roots_in(temp.path());
        let root = roots.claude_projects();
        let session = root.join("-home-alaz-dev-demo").join("sess-huge.jsonl");

        // Content is irrelevant on purpose: if the cap works, none of it is read.
        let mut bytes = vec![b'x'; TRANSCRIPT_MAX_BYTES as usize + 1];
        bytes.push(b'\n');
        fs::create_dir_all(session.parent().expect("fixture parent")).expect("fixture dir");
        fs::write(&session, bytes).expect("huge fixture");

        let (records, report) = discover_claude(&roots).expect("claude import");

        assert_eq!(
            report.oversized, 1,
            "an over-ceiling transcript is its own outcome, not malformed or empty"
        );
        assert_eq!(report.empty, 0, "the file was refused, not read as empty");
        assert!(records.is_empty(), "nothing past the cap may be indexed");
    }
}
