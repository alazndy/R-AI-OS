//! Keeps a project's `memory.md` small without ever losing anything.
//!
//! `memory.md` is read by agents as context, and the agent wrapper keeps
//! appending to it, so it only grows: on this repository it reached ~200 KB
//! (~50k tokens), of which the sections that actually guide work were ~10 KB.
//! The bulk was two append-only logs: `## Change Log …` and `## Instincts`.
//!
//! [`archive_memory`] moves the *oldest* entries of those two sections into a
//! sibling `memory.archive.md` once the section exceeds a byte budget. Nothing
//! is deleted:
//!
//! * the moved text is appended to the archive and synced to disk **before**
//!   `memory.md` is rewritten, so a crash in between duplicates, never loses;
//! * `memory.md` is replaced atomically (temp file + rename), and only if it
//!   is unchanged since it was read — if an agent wrote to it meanwhile, it is
//!   left alone and the next run retries;
//! * before anything is written the plan is checked: every non-blank original
//!   line must be present in the new `memory.md` or in the moved text;
//! * a section whose entry order cannot be determined is not touched.
//!
//! Sections other than the two logs are never modified.

use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MEMORY_FILE: &str = "memory.md";
pub const ARCHIVE_FILE: &str = "memory.archive.md";

/// How much of each log stays in `memory.md`.
#[derive(Debug, Clone)]
pub struct ArchivePolicy {
    /// Byte budget for the Change Log entries kept in `memory.md`.
    pub change_log_bytes: usize,
    /// Byte budget for the Instincts entries kept in `memory.md`.
    pub instinct_bytes: usize,
    /// Always keep at least this many entries per section, even over budget.
    pub min_entries: usize,
    /// [`archive_if_oversized`] does nothing while `memory.md` is at or below this.
    pub trigger_bytes: usize,
}

impl Default for ArchivePolicy {
    fn default() -> Self {
        Self {
            change_log_bytes: 24 * 1024,
            instinct_bytes: 16 * 1024,
            min_entries: 5,
            trigger_bytes: 64 * 1024,
        }
    }
}

/// What an archive run did (or, for a dry run, would do).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveReport {
    pub change_log_moved: usize,
    pub instincts_moved: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
    /// `memory.md` was actually rewritten.
    pub applied: bool,
    /// Why nothing was applied although entries were selected.
    pub skipped_reason: Option<String>,
    /// Change-log-shaped lines found inside `## Instincts` (misfiled entries).
    pub misfiled_in_instincts: usize,
}

impl ArchiveReport {
    pub fn moved(&self) -> usize {
        self.change_log_moved + self.instincts_moved
    }
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    NewestFirst,
    OldestFirst,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    ChangeLog,
    Instincts,
}

fn section_kind(heading: &str) -> Option<Kind> {
    let h = heading.trim_end();
    if h.starts_with("## Change Log") {
        Some(Kind::ChangeLog)
    } else if h == "## Instincts" {
        Some(Kind::Instincts)
    } else {
        None
    }
}

/// Line index ranges `[heading, next_heading)` of every `## ` section.
fn sections(lines: &[&str]) -> Vec<(usize, usize)> {
    let heads: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("## "))
        .map(|(i, _)| i)
        .collect();
    heads
        .iter()
        .enumerate()
        .map(|(n, &start)| (start, heads.get(n + 1).copied().unwrap_or(lines.len())))
        .collect()
}

/// A section body split into its parts. Concatenating `preamble`, every
/// entry's lines and `trailing` reproduces the body exactly.
struct Body<'a> {
    preamble: Vec<&'a str>,
    entries: Vec<Vec<&'a str>>,
    /// Blank lines at the end of the section (the gap before the next heading).
    trailing: Vec<&'a str>,
}

fn split_body<'a>(body: &[&'a str]) -> Body<'a> {
    let mut end = body.len();
    while end > 0 && body[end - 1].trim().is_empty() {
        end -= 1;
    }
    let (content, trailing) = body.split_at(end);
    let mut preamble = Vec::new();
    let mut entries: Vec<Vec<&str>> = Vec::new();
    for &line in content {
        if line.starts_with("- ") {
            entries.push(vec![line]);
        } else if let Some(current) = entries.last_mut() {
            current.push(line);
        } else {
            preamble.push(line);
        }
    }
    Body {
        preamble,
        entries,
        trailing: trailing.to_vec(),
    }
}

/// `YYYY-MM-DD` at the start of an entry (`- [2026-10-26] …` or `- 2026-08-17 …`).
fn entry_date(first_line: &str) -> Option<&str> {
    let rest = first_line.strip_prefix("- ")?;
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    let date = rest.get(..10)?;
    let b = date.as_bytes();
    let shaped = b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit);
    shaped.then_some(date)
}

fn detect_order(entries: &[Vec<&str>]) -> Order {
    let mut dates = entries.iter().filter_map(|e| entry_date(e[0]));
    let (Some(first), Some(last)) = (dates.next(), dates.next_back()) else {
        return Order::Unknown;
    };
    match first.cmp(last) {
        std::cmp::Ordering::Greater => Order::NewestFirst,
        std::cmp::Ordering::Less => Order::OldestFirst,
        std::cmp::Ordering::Equal => Order::Unknown,
    }
}

fn entry_bytes(entry: &[&str]) -> usize {
    entry.iter().map(|l| l.len()).sum()
}

/// Index range of the entries to *move*, given which end is newest.
/// Keeps entries from the newest end while they fit the budget (always at
/// least `min_entries`).
fn split_point(entries: &[Vec<&str>], order: Order, budget: usize, min_entries: usize) -> usize {
    let n = entries.len();
    let mut kept = 0usize;
    let mut bytes = 0usize;
    let size = |i: usize| entry_bytes(&entries[i]);
    let idx = |k: usize| match order {
        Order::NewestFirst => k,
        _ => n - 1 - k,
    };
    while kept < n {
        let s = size(idx(kept));
        if kept >= min_entries && bytes + s > budget {
            break;
        }
        bytes += s;
        kept += 1;
    }
    kept
}

// ─── Planning ────────────────────────────────────────────────────────────────

struct Plan {
    new_content: String,
    change_log: Vec<String>,
    instincts: Vec<String>,
    misfiled: usize,
}

fn plan(content: &str, policy: &ArchivePolicy) -> Plan {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut out = String::with_capacity(content.len());
    let mut change_log = Vec::new();
    let mut instincts = Vec::new();
    let mut misfiled = 0;

    let secs = sections(&lines);
    // Anything before the first `## ` heading (title, intro) is copied as is.
    let first_heading = secs.first().map(|s| s.0).unwrap_or(lines.len());
    lines[..first_heading].iter().for_each(|l| out.push_str(l));

    for (start, end) in secs {
        let heading = lines[start];
        out.push_str(heading);
        let body = &lines[start + 1..end];
        let Some(kind) = section_kind(heading) else {
            body.iter().for_each(|l| out.push_str(l));
            continue;
        };

        let parts = split_body(body);
        if kind == Kind::Instincts {
            misfiled += parts
                .entries
                .iter()
                .filter(|e| entry_date(e[0]).is_some())
                .count();
        }
        let order = match kind {
            // `instinct::append_to_memory_md` inserts directly under the heading.
            Kind::Instincts => Order::NewestFirst,
            Kind::ChangeLog => detect_order(&parts.entries),
        };
        let budget = match kind {
            Kind::ChangeLog => policy.change_log_bytes,
            Kind::Instincts => policy.instinct_bytes,
        };

        let kept = if order == Order::Unknown {
            parts.entries.len()
        } else {
            split_point(&parts.entries, order, budget, policy.min_entries)
        };
        let n = parts.entries.len();
        // Entries kept sit at the newest end; the rest are moved, in file order.
        let (keep_range, move_range) = match order {
            Order::OldestFirst => (n - kept..n, 0..n - kept),
            _ => (0..kept, kept..n),
        };

        parts.preamble.iter().for_each(|l| out.push_str(l));
        let moved: Vec<String> = parts.entries[move_range.clone()]
            .iter()
            .map(|e| e.concat())
            .collect();
        // Kept entries are the newest ones, still in file order.
        for e in &parts.entries[keep_range] {
            e.iter().for_each(|l| out.push_str(l));
        }
        ensure_final_newline(&mut out, !parts.trailing.is_empty());
        parts.trailing.iter().for_each(|l| out.push_str(l));

        match kind {
            Kind::ChangeLog => change_log.extend(moved),
            Kind::Instincts => instincts.extend(moved),
        }
    }

    Plan {
        new_content: out,
        change_log,
        instincts,
        misfiled,
    }
}

/// The last kept entry may lack a final newline only when it was the very last
/// line of the file; add one when something has to follow it.
fn ensure_final_newline(out: &mut String, something_follows: bool) {
    if something_follows && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Safety net: every non-blank original line must survive, either in the new
/// `memory.md` or in the text that goes to the archive.
fn nothing_lost(original: &str, new_content: &str, moved: &[&String]) -> bool {
    fn counts<'a>(texts: impl Iterator<Item = &'a str>) -> HashMap<&'a str, usize> {
        let mut map = HashMap::new();
        for text in texts {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                *map.entry(line).or_insert(0) += 1;
            }
        }
        map
    }
    let before = counts(std::iter::once(original));
    let after = counts(std::iter::once(new_content).chain(moved.iter().map(|s| s.as_str())));
    before
        .iter()
        .all(|(line, n)| after.get(*line).copied().unwrap_or(0) >= *n)
}

// ─── Applying ────────────────────────────────────────────────────────────────

fn archive_block(label: &str, entries: &[String], already: &HashSet<String>) -> String {
    let fresh: Vec<&String> = entries
        .iter()
        .filter(|e| {
            e.lines()
                .next()
                .is_none_or(|first| !already.contains(first.trim_end()))
        })
        .collect();
    if fresh.is_empty() {
        return String::new();
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let mut block = format!(
        "\n## Archived {stamp} — {label} ({} entries)\n\n",
        fresh.len()
    );
    for entry in fresh {
        block.push_str(entry);
        if !entry.ends_with('\n') {
            block.push('\n');
        }
    }
    block
}

fn archive_header(project_dir: &Path) -> String {
    let name = project_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    format!(
        "# Memory Archive: {name}\n\n\
         Older entries moved out of `memory.md` by `raios memory-archive` so agents read a \
         smaller file. Nothing here was deleted or edited; entries keep their original text. \
         Search this file when `memory.md` does not have what you need.\n"
    )
}

fn write_atomically(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp: PathBuf = dir.join(format!(".{MEMORY_FILE}.archive-tmp"));
    let mut file =
        std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

/// Moves the oldest `Change Log` / `Instincts` entries of
/// `<project_dir>/memory.md` into `memory.archive.md` per `policy`.
/// With `dry_run` nothing is written; the report says what would move.
pub fn archive_memory(
    project_dir: &Path,
    policy: &ArchivePolicy,
    dry_run: bool,
) -> Result<ArchiveReport> {
    archive_memory_with(project_dir, policy, dry_run, || {})
}

/// [`archive_memory`] with a hook that runs after the archive is written and
/// before `memory.md` is re-checked and replaced — the window in which a
/// concurrent writer can interfere. Exists so tests can exercise that window.
fn archive_memory_with(
    project_dir: &Path,
    policy: &ArchivePolicy,
    dry_run: bool,
    before_replace: impl FnOnce(),
) -> Result<ArchiveReport> {
    let memory_path = project_dir.join(MEMORY_FILE);
    let original = std::fs::read_to_string(&memory_path)
        .with_context(|| format!("reading {}", memory_path.display()))?;
    let plan = plan(&original, policy);

    let mut report = ArchiveReport {
        change_log_moved: plan.change_log.len(),
        instincts_moved: plan.instincts.len(),
        bytes_before: original.len(),
        bytes_after: plan.new_content.len(),
        applied: false,
        skipped_reason: None,
        misfiled_in_instincts: plan.misfiled,
    };
    if report.moved() == 0 || dry_run {
        return Ok(report);
    }

    let moved: Vec<&String> = plan
        .change_log
        .iter()
        .chain(plan.instincts.iter())
        .collect();
    if !nothing_lost(&original, &plan.new_content, &moved) {
        return Err(anyhow!(
            "refusing to archive: the plan would drop lines from {}",
            memory_path.display()
        ));
    }

    // 1. Archive first, durably. A crash after this duplicates, never loses.
    let archive_path = project_dir.join(ARCHIVE_FILE);
    let existing = std::fs::read_to_string(&archive_path).unwrap_or_default();
    let already: HashSet<String> = existing.lines().map(|l| l.trim_end().to_string()).collect();
    let mut addition = String::new();
    if existing.is_empty() {
        addition.push_str(&archive_header(project_dir));
    }
    addition.push_str(&archive_block("Change Log", &plan.change_log, &already));
    addition.push_str(&archive_block("Instincts", &plan.instincts, &already));
    let mut archive = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&archive_path)
        .with_context(|| format!("opening {}", archive_path.display()))?;
    archive.write_all(addition.as_bytes())?;
    archive.sync_all()?;

    before_replace();

    // 2. Replace memory.md only if nobody touched it since we read it.
    if std::fs::read_to_string(&memory_path).ok().as_deref() != Some(original.as_str()) {
        report.skipped_reason =
            Some("memory.md changed while archiving; left untouched, will retry".to_string());
        return Ok(report);
    }
    write_atomically(&memory_path, &plan.new_content)?;
    report.applied = true;
    Ok(report)
}

/// [`archive_memory`], but only when `memory.md` is larger than
/// `policy.trigger_bytes`. Returns `None` when there is nothing to do (no
/// file, or small enough).
pub fn archive_if_oversized(
    project_dir: &Path,
    policy: &ArchivePolicy,
) -> Result<Option<ArchiveReport>> {
    let Ok(meta) = std::fs::metadata(project_dir.join(MEMORY_FILE)) else {
        return Ok(None);
    };
    if meta.len() as usize <= policy.trigger_bytes {
        return Ok(None);
    }
    archive_memory(project_dir, policy, false).map(Some)
}

// ─── Inserting a Change Log entry ────────────────────────────────────────────

/// Returns `content` with `entry` (a single `- …` line) added to the
/// `## Change Log` section, on the side that section treats as newest.
///
/// Newest-first (or undetermined, e.g. an empty section) puts it directly
/// under the heading text; oldest-first puts it after the last entry. If the
/// file has no Change Log section, one is appended.
pub fn insert_change_log_entry(content: &str, entry: &str) -> String {
    let entry_line = format!("{}\n", entry.trim_end_matches('\n'));
    let lines: Vec<&str> = content.split_inclusive('\n').collect();

    let target = sections(&lines)
        .into_iter()
        .find(|&(start, _)| section_kind(lines[start]) == Some(Kind::ChangeLog));
    let Some((start, end)) = target else {
        let mut out = content.trim_end().to_string();
        out.push_str("\n\n## Change Log\n");
        out.push_str(&entry_line);
        return out;
    };

    let parts = split_body(&lines[start + 1..end]);
    let order = detect_order(&parts.entries);

    let mut out = String::with_capacity(content.len() + entry_line.len());
    lines[..=start].iter().for_each(|l| out.push_str(l));
    parts.preamble.iter().for_each(|l| out.push_str(l));
    let push_entries = |out: &mut String| {
        for e in &parts.entries {
            e.iter().for_each(|l| out.push_str(l));
        }
    };
    if order == Order::OldestFirst {
        push_entries(&mut out);
        ensure_final_newline(&mut out, true);
        out.push_str(&entry_line);
    } else {
        out.push_str(&entry_line);
        push_entries(&mut out);
    }
    ensure_final_newline(&mut out, !parts.trailing.is_empty());
    parts.trailing.iter().for_each(|l| out.push_str(l));
    lines[end..].iter().for_each(|l| out.push_str(l));
    out
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

/// `raios memory-archive`: archive now, regardless of the trigger size.
pub fn cmd_memory_archive(
    project: Option<&str>,
    dry_run: bool,
    policy: &ArchivePolicy,
    json: bool,
) {
    let dir = match project {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    match archive_memory(&dir, policy, dry_run) {
        Ok(report) => print_report(&dir, &report, dry_run, json),
        Err(e) => {
            if json {
                println!("{}", serde_json::json!({ "error": e.to_string() }));
            } else {
                eprintln!("memory-archive: {e:#}");
            }
            std::process::exit(1);
        }
    }
}

fn print_report(dir: &Path, r: &ArchiveReport, dry_run: bool, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "project": dir.display().to_string(),
                "dry_run": dry_run,
                "applied": r.applied,
                "change_log_moved": r.change_log_moved,
                "instincts_moved": r.instincts_moved,
                "bytes_before": r.bytes_before,
                "bytes_after": r.bytes_after,
                "skipped_reason": r.skipped_reason,
                "misfiled_in_instincts": r.misfiled_in_instincts,
            })
        );
        return;
    }
    if r.moved() == 0 {
        println!(
            "memory.md is already within budget ({} KB) — nothing to archive.",
            r.bytes_before / 1024
        );
        return;
    }
    let verb = if dry_run { "Would move" } else { "Moved" };
    println!(
        "{verb} {} Change Log and {} Instincts entries to {ARCHIVE_FILE}: {} KB → {} KB.",
        r.change_log_moved,
        r.instincts_moved,
        r.bytes_before / 1024,
        r.bytes_after / 1024
    );
    if let Some(reason) = &r.skipped_reason {
        println!("⚠ {reason}");
    }
    if r.misfiled_in_instincts > 0 {
        println!(
            "ℹ {} dated, Change-Log-shaped entries sit under ## Instincts (older versions \
             appended to the end of the file). They are archived by position like any other.",
            r.misfiled_in_instincts
        );
    }
}

#[cfg(test)]
mod tests;
