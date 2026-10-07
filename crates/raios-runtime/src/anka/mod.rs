//! ANKA read-only transcript recall with a rebuildable sidecar cache.

pub mod provenance;

mod importers;
pub mod lock;
mod publish;

use anyhow::{bail, Context, Result};
use lock::AnkaLock;
use provenance::{AnkaRoots, Provenance};
// Re-exported so the CLI and MCP surfaces read provenance through one path instead of
// reaching into the module layout; the shared output types are part of the contract.
pub use provenance::{ProjectScope, ProvenanceQuality, TimeSource};
use publish::{atomic_write, ensure_private_dir};
use raios_core::anka::{
    default_cache_path, AnkaCacheState, AnkaConfidence, AnkaHarness, AnkaHarnessCoverage, AnkaHit,
    AnkaIndexStatus, AnkaSearchQuery, AnkaSourceRef,
};
use raios_core::security::redact_secrets;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const INDEX_FILE: &str = "index.json";
const EXCLUDE_FILE: &str = "anka-exclude";
const TOMBSTONE_FILE: &str = "anka-tombstones";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnkaRecord {
    id: String,
    /// Durable forget handle, derived from `SourceIdentity`. Survives append/mtime
    /// changes; never used as a dedup key. `serde(default)` keeps pre-identity caches
    /// readable until migration re-derives it.
    #[serde(default)]
    forget_key: String,
    source: AnkaSourceRef,
    /// Authority for privacy decisions. `serde(default)` keeps pre-provenance caches
    /// readable until Phase 2's migration re-derives it.
    #[serde(default)]
    provenance: Provenance,
    /// Session that spawned an agent transcript, when the record is a subagent run.
    #[serde(default)]
    parent_session: Option<String>,
    /// Agent file identity inside a subagent transcript.
    #[serde(default)]
    agent_id: Option<String>,
    content: String,
}

/// Everything needed to build one record. Keeps [`record`] honest: no caller can
/// construct a record without stating its provenance.
#[derive(Debug, Clone)]
struct RecordSpec {
    harness: AnkaHarness,
    project: String,
    /// Stable source namespace for `forget_key`. For history records this is the
    /// history file stem (never the resolved project/cwd); for Claude it is the
    /// project slug. Display project/cwd stays in `project` and is excluded from
    /// the durable identity.
    identity_project: String,
    session_id: String,
    occurred_at: String,
    content: String,
    provenance: Provenance,
    parent_session: Option<String>,
    agent_id: Option<String>,
    /// Unique-per-record discriminator (line number or source path). Included in
    /// `record_id` only; it is never a durable identity.
    discriminator: String,
    /// Whether this record's forget identity includes a fingerprint of the redacted
    /// prompt. True for prompt-history records (no native immutable ID); false for
    /// transcript-based records whose identity is the source itself.
    prompt_fingerprint: bool,
}

/// The cache as this build understands it — never serialized directly; the wire
/// shapes are [`AnkaIndexV1`] (legacy, read-only) and [`AnkaIndexV2`] (written).
///
/// `indexed_sources` is not persisted in v2 (the envelope deliberately avoids the
/// legacy field name); on a v2 read it is the sum of the coverage entries.
#[derive(Debug, Default)]
struct AnkaIndex {
    records: Vec<AnkaRecord>,
    /// Per-harness refresh coverage. Empty only on a legacy cache read, which
    /// reconstructs what is knowable from the records themselves.
    coverage: Vec<AnkaHarnessCoverage>,
    indexed_sources: usize,
    last_indexed_at: Option<String>,
}

/// The pre-envelope wire shape: exactly what an older install writes, and — the
/// point of the split — exactly what its reader *requires*. Kept as the reference
/// shape for the old-reader rejection test.
#[derive(Debug, Deserialize)]
struct AnkaIndexV1 {
    records: Vec<AnkaRecord>,
    indexed_sources: usize,
    last_indexed_at: Option<String>,
}

/// The v2 wire shape, read side. The `schema_version` marker's *presence* is
/// what routes the read here (see [`read_index`]) and its value is checked
/// before this deserialization runs, so the number itself is not re-stored.
/// The field names are deliberately disjoint from [`AnkaIndexV1`]'s — serde
/// *ignores* unknown fields but cannot invent missing required ones, so an
/// older binary reading this file fails with its own corrupt-index message
/// instead of silently accepting a cache it does not understand.
#[derive(Debug, Deserialize)]
struct AnkaIndexV2File {
    records_v2: Vec<AnkaRecord>,
    coverage: Vec<AnkaHarnessCoverage>,
}

/// The v2 wire shape, write side — borrows, so publishing does not clone the
/// whole record set.
#[derive(Debug, Serialize)]
struct AnkaIndexV2Wire<'a> {
    schema_version: u32,
    records_v2: &'a [AnkaRecord],
    coverage: &'a [AnkaHarnessCoverage],
}

/// The schema version this build reads and writes.
const CACHE_SCHEMA_VERSION: u32 = 2;

/// Marks a cache read that failed on the file's *format* — corrupt bytes or a
/// schema version this build does not know. This is the one failure [`status_in`]
/// reports as [`AnkaCacheState::Incompatible`] instead of propagating; recall
/// still surfaces it as an error, because showing a cache we cannot read would
/// be worse than showing nothing.
#[derive(Debug)]
struct AnkaCacheFormatError(String);

impl std::fmt::Display for AnkaCacheFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AnkaCacheFormatError {}

fn format_error(message: String) -> anyhow::Error {
    anyhow::Error::new(AnkaCacheFormatError(message))
}

const CORRUPT_INDEX_MESSAGE: &str = "ANKA index is corrupt; run `raios anka index` to rebuild it";

pub fn parse_harness(value: &str) -> Result<AnkaHarness> {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" => Ok(AnkaHarness::Claude),
        "codex" => Ok(AnkaHarness::Codex),
        "opencode" => Ok(AnkaHarness::Opencode),
        "agy" | "antigravity" => Ok(AnkaHarness::Antigravity),
        _ => {
            bail!("unsupported ANKA harness '{value}'; use claude, codex, opencode, or antigravity")
        }
    }
}

/// Rebuild the cache, or one harness's slice of it.
///
/// The whole read-modify-publish sequence — exclusions and tombstones read, sources
/// discovered, index published — runs while holding [`AnkaLock`]. Taking the lock
/// *before* reading the tombstones is what makes a concurrent `forget` impossible to
/// miss: an indexer that read the old tombstone set cannot publish over a tombstone
/// appended afterwards.
pub fn index(harness: Option<AnkaHarness>) -> Result<AnkaIndexStatus> {
    index_in(
        &AnkaRoots::live(),
        &default_cache_path(),
        &config_dir(),
        harness,
    )
}

fn index_in(
    roots: &AnkaRoots,
    cache_path: &Path,
    config: &Path,
    harness: Option<AnkaHarness>,
) -> Result<AnkaIndexStatus> {
    ensure_private_dir(cache_path)?;
    let _lock = AnkaLock::acquire(cache_path)?;
    // Never write over a cache this build cannot read. A *corrupt* cache stays
    // rebuildable (its own message tells the user to run `raios anka index`);
    // an unknown schema version does not — that file may hold data only a newer
    // build understands.
    check_cache_schema(cache_path)?;
    migrate_tombstones(roots, cache_path, config)?;
    let exclusions = read_lines(&config.join(EXCLUDE_FILE))?;
    let tombstones = read_tombstones(config)?;
    let full_refresh = harness.is_none();
    let target = harness;
    // A full refresh never reads the previous cache — that is what keeps a
    // corrupt cache rebuildable. A harness-scoped refresh does read it: it
    // carries the other harnesses' records that this run must preserve.
    let previous = if full_refresh {
        None
    } else {
        Some(read_index(cache_path)?)
    };
    let harnesses = target
        .clone()
        .map(|item| vec![item])
        .unwrap_or_else(|| AnkaHarness::ALL.to_vec());
    let refreshed_at = now();
    let mut records = Vec::new();
    let mut coverage = Vec::new();
    for item in harnesses {
        let (found, harness_report) = discover_harness(roots, &item)?;
        coverage.push(AnkaHarnessCoverage {
            harness: item,
            sources: harness_report.sources,
            records: found.len(),
            indexed_at: refreshed_at.clone(),
            full_refresh,
            oversized: harness_report.oversized,
        });
        records.extend(found);
    }
    if let Some(previous) = previous {
        // Preserve the other harnesses' coverage verbatim (including their
        // `full_refresh` and timing, so a scoped refresh stays visible as one)
        // and carry their records forward below.
        if let Some(target) = &target {
            coverage.extend(
                previous
                    .coverage
                    .into_iter()
                    .filter(|entry| entry.harness != *target),
            );
            records.extend(
                previous
                    .records
                    .into_iter()
                    .filter(|record| record.source.harness != *target),
            );
        }
    }
    // One filter over *every* record — carried-forward ones included — so a
    // scoped rebuild cannot resurrect a record hidden or excluded since the
    // previous refresh.
    records.retain(|record| {
        !tombstoned(record, &tombstones)
            && !exclusions
                .iter()
                .any(|pattern| contains(&record.source.project, pattern))
    });
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    // Stable envelope order regardless of which refresh wrote the entries.
    coverage.sort_by_key(|entry| {
        AnkaHarness::ALL
            .iter()
            .position(|harness| *harness == entry.harness)
            .unwrap_or(usize::MAX)
    });
    // Report each harness's coverage from the records actually being published —
    // the count is only trustworthy if it is the same set the index will contain.
    for entry in &mut coverage {
        entry.records = records
            .iter()
            .filter(|record| record.source.harness == entry.harness)
            .count();
    }
    let index = AnkaIndex {
        records,
        indexed_sources: coverage.iter().map(|entry| entry.sources).sum(),
        coverage,
        last_indexed_at: Some(refreshed_at),
    };
    write_index(cache_path, &index)?;
    Ok(status_from_index(cache_path.to_path_buf(), &index))
}

/// Refuse to build on top of a cache whose `schema_version` this build does not
/// know. Only `NotFound` passes as "nothing to lose": an existing file this build
/// cannot read (permissions, I/O) must stop the rebuild rather than be mistaken for
/// an absent one and then overwritten. Unparsable bytes pass too — a corrupt cache
/// is exactly what `raios anka index` exists to rebuild — but a `schema_version`
/// field that is present and not a number is refused like an unknown version: that
/// file was written by something this build does not understand, and destroying it
/// is the user's explicit move, never a side effect of asking for a rebuild.
fn check_cache_schema(cache_path: &Path) -> Result<()> {
    let path = cache_path.join(INDEX_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(());
    };
    match value.get("schema_version") {
        None => Ok(()),
        Some(version) => match version.as_u64() {
            Some(version) if version == u64::from(CACHE_SCHEMA_VERSION) => Ok(()),
            Some(version) => Err(format_error(unsupported_schema_message(version))),
            None => Err(format_error(invalid_schema_type_message())),
        },
    }
}

fn invalid_schema_type_message() -> String {
    format!(
        "ANKA cache schema_version is not a number (this build reads and writes version \
         {CACHE_SCHEMA_VERSION}) — refusing to overwrite it; use the build that wrote the \
         cache, or remove the file to start over"
    )
}

fn unsupported_schema_message(version: u64) -> String {
    format!(
        "ANKA cache schema version {version} is not supported by this build (it reads and \
         writes version {CACHE_SCHEMA_VERSION}) — refusing to overwrite it; use the build \
         that wrote the cache, or remove the file to start over"
    )
}

pub fn status() -> Result<AnkaIndexStatus> {
    status_in(&default_cache_path())
}

/// Status, with the cache's *readability* handled explicitly: a cache whose
/// format this build cannot follow is reported as [`AnkaCacheState::Incompatible`]
/// rather than as an error, because describing the cache is exactly what `status`
/// is for. Recall keeps propagating the same failure — it must fail safe, not
/// describe.
fn status_in(cache_path: &Path) -> Result<AnkaIndexStatus> {
    match read_index(cache_path) {
        Ok(index) => Ok(status_from_index(cache_path.to_path_buf(), &index)),
        Err(error) if error.downcast_ref::<AnkaCacheFormatError>().is_some() => {
            Ok(AnkaIndexStatus {
                state: AnkaCacheState::Incompatible,
                coverage: Vec::new(),
                cache_path: cache_path.to_path_buf(),
                indexed_sources: 0,
                indexed_records: 0,
                last_indexed_at: None,
            })
        }
        Err(error) => Err(error),
    }
}

/// The transport view of [`AnkaIndexStatus`] that the CLI and MCP surfaces
/// share, so neither hand-builds the status payload — which is how `state` came
/// to be guessed from `last_indexed_at` in the first place.
pub fn status_dto(status: AnkaIndexStatus) -> raios_contracts::anka::AnkaIndexStatusDto {
    raios_contracts::anka::AnkaIndexStatusDto {
        state: status.state.as_str().to_string(),
        cache_path: status.cache_path.display().to_string(),
        indexed_sources: status.indexed_sources,
        indexed_records: status.indexed_records,
        last_indexed_at: status.last_indexed_at,
        coverage: status
            .coverage
            .into_iter()
            .map(|entry| raios_contracts::anka::AnkaHarnessCoverageDto {
                harness: entry.harness.as_str().to_string(),
                sources: entry.sources,
                records: entry.records,
                indexed_at: entry.indexed_at,
                full_refresh: entry.full_refresh,
                oversized: entry.oversized,
            })
            .collect(),
    }
}

pub fn search(query: AnkaSearchQuery) -> Result<Vec<AnkaHit>> {
    search_in(&default_cache_path(), &config_dir(), query)
}

/// Recall, with the tombstone check that keeps `forget` honest between rebuilds.
///
/// Filtering tombstones only inside [`index_in`] is not enough: a `forget` that appends
/// its tombstone but fails to publish the rewritten index leaves the record in the
/// cache, and no rebuild may ever run again. The check has to live on the read path —
/// `search`, `blame`, and through them the MCP recall tool — or the privacy promise
/// survives only until the next `raios anka index`.
///
/// Read order is deliberate: the index snapshot first, the tombstone set second. A
/// `forget` landing between the two reads is then still honoured (the tombstones are
/// at least as fresh as the snapshot), whereas reading tombstones first would briefly
/// show a record the user has just removed. Neither read takes [`AnkaLock`]: recall is
/// read-only and is allowed to err only towards showing *less*.
fn search_in(cache_path: &Path, config: &Path, query: AnkaSearchQuery) -> Result<Vec<AnkaHit>> {
    let query_text = query.text.trim();
    if query_text.is_empty() {
        bail!("ANKA search query cannot be empty");
    }
    let terms = terms(query_text);
    let index = read_index(cache_path)?;
    let tombstones = read_tombstones(config)?;
    let mut hits = index
        .records
        .iter()
        .filter(|record| !tombstoned(record, &tombstones))
        .filter(|record| {
            query
                .project
                .as_deref()
                .map(|project| contains(&record.source.project, project))
                .unwrap_or(true)
        })
        .filter(|record| {
            query
                .harness
                .as_ref()
                .map(|harness| record.source.harness == *harness)
                .unwrap_or(true)
        })
        .filter_map(|record| hit_for(record, query_text, &terms))
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));
    hits.truncate(query.limit.clamp(1, 100));
    Ok(hits)
}

pub fn blame(path: &str, limit: usize) -> Result<Vec<AnkaHit>> {
    blame_in(&default_cache_path(), &config_dir(), path, limit)
}

/// [`blame`] through the same tombstone-aware path as [`search`], so a forgotten record
/// cannot be reached by routing around `search`.
fn blame_in(cache_path: &Path, config: &Path, path: &str, limit: usize) -> Result<Vec<AnkaHit>> {
    let path = path.trim();
    if path.is_empty() {
        bail!("ANKA blame path cannot be empty");
    }
    let query = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    search_in(
        cache_path,
        config,
        AnkaSearchQuery {
            text: query.to_string(),
            project: None,
            harness: None,
            limit,
        },
    )
}

/// Hides a record from ANKA and records a durable tombstone. Source transcripts are untouched.
///
/// Runs under the same [`AnkaLock`] as [`index`], covering read-index → append-tombstone
/// → write-index, so two concurrent `forget`s cannot lose one another's tombstone and a
/// rebuild cannot publish an index taken from a stale tombstone set. The tombstone is
/// written *before* the index is rewritten, so a crash between the two leaves the record
/// still hidden by the durable tombstone rather than resurrected.
///
/// A tombstone that has landed is never rolled back, not even when the index publish that
/// follows it fails: undoing it would put the record back in front of the user, which is
/// the exact outcome `forget` exists to prevent. The failure is reported with its
/// [`publish::PublishError`] phase, and [`search_in`] keeps hiding the record meanwhile.
pub fn forget(id: &str) -> Result<bool> {
    forget_in(&AnkaRoots::live(), &default_cache_path(), &config_dir(), id)
}

fn forget_in(roots: &AnkaRoots, cache_path: &Path, config: &Path, id: &str) -> Result<bool> {
    let id = id.trim();
    if id.is_empty() {
        bail!("ANKA record id cannot be empty");
    }
    ensure_private_dir(cache_path)?;
    let _lock = AnkaLock::acquire(cache_path)?;
    migrate_tombstones(roots, cache_path, config)?;
    let mut index = read_index(cache_path)?;
    let Some(record) = index.records.iter().find(|record| record.id == id) else {
        return Ok(false);
    };
    // The tombstone is always the `forget_key`, never the `record_id`: the key is
    // durable across append/mtime changes, the id is not. For a cache written
    // before `forget_key` existed the key is derived — and refused outright when
    // the derivation cannot be verified, because a wrong key would let the next
    // rebuild publish the record back while looking protected.
    let forget_key = derive_forget_key(roots, record)?.with_context(|| {
        format!("cannot derive a forget key for record {id}; nothing was written")
    })?;
    add_tombstone(config, &forget_key)?;
    index.records.retain(|record| record.id != id);
    write_index(cache_path, &index)?;
    Ok(true)
}

/// Add a `forget_key` to the tombstone set and publish the versioned file.
///
/// Callers hold [`AnkaLock`]. The publish error propagates — `PublishedUnsynced`
/// included: the entry is visible, but `forget_in` stops before it rewrites the
/// index, because a forget that survives only until the next crash was not what the
/// caller asked for. A retry re-reads the same set and re-publishes it, so the flow
/// stays idempotent while the fsync chain gets another chance to land.
fn add_tombstone(config: &Path, forget_key: &str) -> Result<()> {
    if !is_hash(forget_key) {
        bail!("ANKA refusing to tombstone a value that is not a hash");
    }
    let mut entries = read_tombstones(config)?;
    entries.insert(TombstoneEntry::forget_key(forget_key));
    write_tombstones(config, &entries)
}

/// Discover one harness's sources.
///
/// Phase 1: parsing and provenance correctness. The `ImportReport` counts outcomes
/// without carrying entry contents; Phase 2 folds it into `AnkaIndex.coverage`.
fn discover_harness(
    roots: &AnkaRoots,
    harness: &AnkaHarness,
) -> Result<(Vec<AnkaRecord>, importers::ImportReport)> {
    match harness {
        AnkaHarness::Claude => importers::discover_claude(roots),
        AnkaHarness::Codex => importers::discover_codex(roots),
        AnkaHarness::Opencode => importers::discover_opencode(roots),
        AnkaHarness::Antigravity => importers::discover_antigravity(roots),
    }
}

fn record(spec: RecordSpec) -> AnkaRecord {
    let content = redact_secrets(&spec.content);
    // `record_id` includes the discriminator and observed timestamp, so it is unique
    // per stored record but not a durable identity.
    let id = hash(&[
        spec.harness.as_str(),
        &spec.project,
        &spec.session_id,
        &spec.occurred_at,
        &spec.discriminator,
        &content,
    ]);
    // `forget_key` is derived only from `SourceIdentity`: harness, stable source
    // namespace, session, parent session, agent, and — for prompt-history records —
    // a fingerprint of the redacted prompt. Observed timestamps, discriminators, and
    // the resolved display project are excluded, so a provenance transition or an
    // append/mtime change cannot revive a forgotten record.
    let prompt_fingerprint = if spec.prompt_fingerprint {
        hash(&[&content])
    } else {
        String::new()
    };
    let forget_key = hash(&[
        spec.harness.as_str(),
        &spec.identity_project,
        &spec.session_id,
        spec.parent_session.as_deref().unwrap_or(""),
        spec.agent_id.as_deref().unwrap_or(""),
        &prompt_fingerprint,
    ]);
    let provenance = spec.provenance.clone();
    AnkaRecord {
        id,
        forget_key,
        source: AnkaSourceRef {
            harness: spec.harness,
            project: spec.project,
            session_id: spec.session_id,
            occurred_at: spec.occurred_at,
        },
        provenance,
        parent_session: spec.parent_session,
        agent_id: spec.agent_id,
        content,
    }
}

fn hit_for(record: &AnkaRecord, full_query: &str, terms: &[String]) -> Option<AnkaHit> {
    let haystack = record.content.to_ascii_lowercase();
    let exact = haystack.contains(&full_query.to_ascii_lowercase());
    let matched = terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count();
    if !exact && matched == 0 {
        return None;
    }
    let offset = terms
        .iter()
        .find_map(|term| haystack.find(term))
        .unwrap_or(0);
    let start = offset.saturating_sub(180);
    let end = (offset + 820).min(record.content.len());
    let mut snippet = record
        .content
        .get(start..end)
        .unwrap_or(&record.content)
        .to_string();
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < record.content.len() {
        snippet.push('…');
    }
    Some(AnkaHit {
        id: record.id.clone(),
        source: record.source.clone(),
        snippet,
        score: if exact {
            100.0 + matched as f64
        } else {
            matched as f64
        },
        confidence: if exact {
            AnkaConfidence::Exact
        } else {
            AnkaConfidence::Lexical
        },
    })
}

fn terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
                .to_ascii_lowercase()
        })
        .filter(|term| term.len() >= 2)
        .collect()
}

fn status_from_index(cache_path: PathBuf, index: &AnkaIndex) -> AnkaIndexStatus {
    // `Ready` requires an entry for *every* harness: only then does the cache
    // speak for all of them. Coverage with a gap — a harness-scoped refresh, or
    // a legacy cache whose coverage was reconstructed from its records — is
    // `Partial`; nothing at all is `Empty`. Never `last_indexed_at`: a scoped
    // refresh updates that too, and used to read as "ready" over a cache that
    // had just been narrowed to one harness.
    let state = if AnkaHarness::ALL
        .iter()
        .all(|harness| index.coverage.iter().any(|entry| entry.harness == *harness))
    {
        AnkaCacheState::Ready
    } else if index.coverage.is_empty() && index.records.is_empty() {
        AnkaCacheState::Empty
    } else {
        AnkaCacheState::Partial
    };
    AnkaIndexStatus {
        state,
        coverage: index.coverage.clone(),
        cache_path,
        indexed_sources: index.indexed_sources,
        indexed_records: index.records.len(),
        last_indexed_at: index.last_indexed_at.clone(),
    }
}

fn read_index(cache_path: &Path) -> Result<AnkaIndex> {
    let path = cache_path.join(INDEX_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AnkaIndex::default())
        }
        Err(error) => return Err(error.into()),
    };
    // Branch on the envelope marker itself, not on which fields happen to parse:
    // presence of `schema_version` means a versioned envelope, absence means the
    // legacy shape, and a version this build does not know is a refusal — never
    // a fallback to the legacy reader, which would silently misread it.
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|_| format_error(CORRUPT_INDEX_MESSAGE.to_string()))?;
    let corrupt = || format_error(CORRUPT_INDEX_MESSAGE.to_string());
    match value.get("schema_version") {
        None => {
            let legacy: AnkaIndexV1 = serde_json::from_value(value).map_err(|_| corrupt())?;
            let coverage = legacy_coverage(&legacy.records, legacy.last_indexed_at.as_deref());
            Ok(AnkaIndex {
                records: legacy.records,
                coverage,
                indexed_sources: legacy.indexed_sources,
                last_indexed_at: legacy.last_indexed_at,
            })
        }
        Some(version) => {
            let Some(number) = version.as_u64() else {
                // Present but not a number: same refusal as an unknown version —
                // a version field this build cannot interpret may be anything,
                // and "corrupt, rebuild it" would invite overwriting it.
                return Err(format_error(invalid_schema_type_message()));
            };
            if number != u64::from(CACHE_SCHEMA_VERSION) {
                return Err(format_error(unsupported_schema_message(number)));
            }
            let envelope: AnkaIndexV2File = serde_json::from_value(value).map_err(|_| corrupt())?;
            // The combined refresh stamp is derived, not stored: one source of
            // truth (the per-harness entries) instead of a field that could
            // drift from them.
            let last_indexed_at = envelope
                .coverage
                .iter()
                .map(|entry| entry.indexed_at.as_str())
                .filter(|stamp| !stamp.is_empty())
                .max()
                .map(str::to_string);
            let indexed_sources = envelope.coverage.iter().map(|entry| entry.sources).sum();
            Ok(AnkaIndex {
                records: envelope.records_v2,
                coverage: envelope.coverage,
                indexed_sources,
                last_indexed_at,
            })
        }
    }
}

/// What is knowable about a legacy cache's coverage, reconstructed from the
/// records themselves: which harnesses it speaks for, and when it was written.
/// Sources, oversized counts, and refresh scope were never persisted, so they
/// report the honest zeros/false rather than a guess — and a harness with no
/// records gets no entry, which is why a legacy cache reads as `Partial` until
/// the first `raios anka index` writes real coverage.
fn legacy_coverage(
    records: &[AnkaRecord],
    last_indexed_at: Option<&str>,
) -> Vec<AnkaHarnessCoverage> {
    AnkaHarness::ALL
        .iter()
        .filter_map(|harness| {
            let records = records
                .iter()
                .filter(|record| record.source.harness == *harness)
                .count();
            (records > 0).then(|| AnkaHarnessCoverage {
                harness: harness.clone(),
                sources: 0,
                records,
                indexed_at: last_indexed_at.unwrap_or_default().to_string(),
                full_refresh: false,
                oversized: 0,
            })
        })
        .collect()
}

fn write_index(cache_path: &Path, index: &AnkaIndex) -> Result<()> {
    // No directory creation or hardening here: every caller (`index_in`, `forget_in`)
    // has already created the cache owner-only before taking the lock, and a hidden
    // `chmod` inside the critical section would both race with peers and mask a real
    // permission failure as if it were a recoverable one.
    //
    // What lands on disk is the v2 envelope: `schema_version` plus field names
    // the legacy shape never used, so this build's output is the one an older
    // reader fails on rather than one it would silently half-accept.
    let wire = AnkaIndexV2Wire {
        schema_version: CACHE_SCHEMA_VERSION,
        records_v2: &index.records,
        coverage: &index.coverage,
    };
    Ok(atomic_write(
        &cache_path.join(INDEX_FILE),
        &serde_json::to_vec(&wire)?,
    )?)
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("raios")
}

fn read_lines(path: &Path) -> Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(error.into()),
    }
}

/// Schema version written into — and required by — the versioned tombstone file.
const TOMBSTONE_SCHEMA_VERSION: u32 = 2;

/// Which identity a tombstone entry carries.
///
/// Declared in the file itself so a reader never guesses: a `forget_key` entry may
/// only match a record's `forget_key`, a `record_id` entry only its id. Legacy
/// entries therefore keep hiding exactly the cache they were written against
/// without ever matching a rebuilt record, whose id is computed differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TombstoneKind {
    ForgetKey,
    RecordId,
}

/// One entry of the versioned tombstone file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct TombstoneEntry {
    #[serde(rename = "type")]
    kind: TombstoneKind,
    /// A 64-hex-digit hash: a `forget_key` or a legacy `record_id`.
    value: String,
}

impl TombstoneEntry {
    fn forget_key(value: &str) -> Self {
        Self {
            kind: TombstoneKind::ForgetKey,
            value: value.to_string(),
        }
    }

    fn record_id(value: &str) -> Self {
        Self {
            kind: TombstoneKind::RecordId,
            value: value.to_string(),
        }
    }
}

/// On-disk shape a tombstone file was found in. Deciding this is deliberate: a file
/// is versioned **only** when it carries the explicit `schema_version` header — a
/// headerless file is recognized as legacy by its lines being hashes and is never
/// treated as versioned, whatever its content looks like otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TombstoneFormat {
    Legacy,
    Versioned,
}

/// A hash as this module writes them: exactly 64 hex digits. Used to validate every
/// entry on read, so a corrupt value fails the read instead of silently matching
/// nothing.
fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Serialized form of the versioned tombstone file.
#[derive(Debug, Serialize, Deserialize)]
struct TombstoneFile {
    schema_version: u32,
    entries: Vec<TombstoneEntry>,
}

/// Read the tombstone file in either on-disk format.
///
/// - **Versioned** (starts with `{`): `schema_version` must be exactly
///   [`TOMBSTONE_SCHEMA_VERSION`]; an unknown version or an invalid entry is an
///   error, never an empty set — recall must fail safe, not forget safe.
/// - **Legacy** (headerless): one `record_id` hash per line, `#` comments allowed.
///   A line that is neither is corruption.
///
/// A missing or empty file is an empty set: nothing has been forgotten yet.
fn read_tombstones_raw(config: &Path) -> Result<(TombstoneFormat, BTreeSet<TombstoneEntry>)> {
    let path = config.join(TOMBSTONE_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((TombstoneFormat::Legacy, BTreeSet::new()));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok((TombstoneFormat::Legacy, BTreeSet::new()));
    }
    let corrupt = || {
        anyhow::anyhow!(
            "ANKA tombstones are corrupt ({}); fix or remove the file to continue — \
             removing it drops the forget protections it holds",
            path.display()
        )
    };
    if trimmed.starts_with('{') {
        let file: TombstoneFile = serde_json::from_str(trimmed).map_err(|_| corrupt())?;
        if file.schema_version != TOMBSTONE_SCHEMA_VERSION {
            bail!(
                "unsupported ANKA tombstone schema version {}; this build understands {}",
                file.schema_version,
                TOMBSTONE_SCHEMA_VERSION
            );
        }
        let mut entries = BTreeSet::new();
        for entry in file.entries {
            if !is_hash(&entry.value) {
                return Err(corrupt());
            }
            entries.insert(entry);
        }
        return Ok((TombstoneFormat::Versioned, entries));
    }
    let mut entries = BTreeSet::new();
    for line in trimmed.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !is_hash(line) {
            return Err(corrupt());
        }
        entries.insert(TombstoneEntry::record_id(line));
    }
    Ok((TombstoneFormat::Legacy, entries))
}

/// The tombstone set, in whichever format the file is written.
fn read_tombstones(config: &Path) -> Result<BTreeSet<TombstoneEntry>> {
    Ok(read_tombstones_raw(config)?.1)
}

/// Publish the versioned tombstone file. The config directory is created owner-only
/// on first use — `atomic_write` never creates directories itself, so a permission
/// failure there stays visible instead of being papered over.
///
/// The publish error propagates as it landed, `PublishedUnsynced` included. That
/// error is not success on this side: the entries are visible, but the callers that
/// stop here (`index_in`, `forget_in`) are about to publish the replacement index,
/// and records are hidden by the tombstone's *durability*, not by its visibility —
/// a crash that reverts an unconfirmed tombstone resurrects whatever the index
/// publish would have stripped. So the error stops the flow before the index moves,
/// and the retry re-publishes the same set — [`migrate_tombstones`] never skips that
/// step just because the file already reads as versioned — re-running the
/// file+directory fsync chain until it lands.
fn write_tombstones(config: &Path, entries: &BTreeSet<TombstoneEntry>) -> Result<()> {
    write_tombstones_with(config, entries, atomic_write)
}

/// [`write_tombstones`] with the publish supplied by the caller: the seam that makes
/// an unconfirmed publish observable in a test without needing a real directory fsync
/// to fail (everything up to and including the rename stays the real code).
fn write_tombstones_with<F>(
    config: &Path,
    entries: &BTreeSet<TombstoneEntry>,
    publish_write: F,
) -> Result<()>
where
    F: FnOnce(&Path, &[u8]) -> Result<(), publish::PublishError>,
{
    if !config.as_os_str().is_empty() && !config.exists() {
        ensure_private_dir(config)?;
    }
    let file = TombstoneFile {
        schema_version: TOMBSTONE_SCHEMA_VERSION,
        entries: entries.iter().cloned().collect(),
    };
    publish_write(&config.join(TOMBSTONE_FILE), &serde_json::to_vec(&file)?)?;
    Ok(())
}

/// Whether a record is covered by the tombstone set, matched **by entry type**: a
/// `forget_key` entry against the record's key, a `record_id` entry against its id.
///
/// This is the recall-side and rebuild-side check in one place. Type-strict matching
/// is what makes the retained legacy entries safe in both directions: they still hide
/// the old cache they came from (whose records predate `forget_key`), and they can
/// never match a rebuilt record — old and new ids are computed from different inputs.
fn tombstoned(record: &AnkaRecord, tombstones: &BTreeSet<TombstoneEntry>) -> bool {
    tombstones.iter().any(|entry| match entry.kind {
        TombstoneKind::ForgetKey => entry.value == record.forget_key,
        TombstoneKind::RecordId => entry.value == record.id,
    })
}

/// One-time translation of legacy tombstones into versioned entries.
///
/// Runs under [`AnkaLock`] and before anything is published:
///
/// 1. Every legacy line is a `record_id`; each must find **its** record in the
///    current cache, and from that record a `forget_key` is derived exactly the way
///    the importer derives it today ([`derive_forget_key`]). One unresolvable
///    tombstone stops the migration *before any write* — no new tombstone file, no
///    index publish — and the tombstone is kept, never dropped: losing it would
///    resurrect the record, which is the outcome this exists to prevent. An id
///    whose record an older `forget` already deleted from the cache lands here too;
///    that state is expected and is resolved explicitly (an approved source-wide
///    exclusion), not by silently discarding the protection.
/// 2. Only when every mapping resolves are the translated entries published — and
///    they are published before the replacement index, which `index_in` writes
///    afterwards. The legacy `record_id` entries are **retained** beside the new
///    `forget_key` entries for exactly that window: a crash between the two writes
///    leaves the old cache on disk with records that predate `forget_key`, and only
///    the retained id entries still match them. Against a rebuilt index they are
///    inert; against the old cache they are the protection.
/// 3. A versioned file is never re-translated. A retry after a crash, and every
///    later `index` run, therefore reads the same set — and *re-publishes* it,
///    byte-identical. The re-publish is not wasted work: an earlier attempt may
///    have stopped the caller with `PublishedUnsynced` (the file visible, its
///    crash-durability unconfirmed), and this retry must re-run the fsync chain
///    rather than treat "already versioned" as "already durable".
fn migrate_tombstones(roots: &AnkaRoots, cache_path: &Path, config: &Path) -> Result<()> {
    let (format, entries) = read_tombstones_raw(config)?;
    match format {
        TombstoneFormat::Versioned => return write_tombstones(config, &entries),
        TombstoneFormat::Legacy if entries.is_empty() => return Ok(()),
        TombstoneFormat::Legacy => {}
    }
    let index = read_index(cache_path).context(
        "ANKA tombstone migration blocked: the previous cache could not be read; \
         nothing was published",
    )?;
    let mut translated = entries.clone();
    for entry in &entries {
        let Some(record) = index.records.iter().find(|record| record.id == entry.value) else {
            bail!(
                "ANKA tombstone migration blocked: no cache record matches tombstone {}; \
                 it may have been removed by an older `forget`. Nothing was published — \
                 the tombstone is kept and must be resolved explicitly rather than dropped",
                entry.value
            );
        };
        let Some(forget_key) = derive_forget_key(roots, record)? else {
            bail!(
                "ANKA tombstone migration blocked: the forget key for tombstone {} cannot \
                 be derived from the cache and its source. Nothing was published",
                entry.value
            );
        };
        translated.insert(TombstoneEntry::forget_key(&forget_key));
    }
    write_tombstones(config, &translated)
}

/// The durable `forget_key` for a record: its own when the cache already carries
/// one, otherwise re-derived the way the current importer derives it.
///
/// Returns `Ok(None)` when the mapping cannot be **verified** — that is an ambiguity
/// for the caller to stop on, never a best-effort guess: a key derived wrongly would
/// silently stop hiding the record after the next rebuild.
fn derive_forget_key(roots: &AnkaRoots, record: &AnkaRecord) -> Result<Option<String>> {
    if !record.forget_key.is_empty() {
        return Ok(Some(record.forget_key.clone()));
    }
    let parent = record.parent_session.as_deref().unwrap_or("");
    let agent = record.agent_id.as_deref().unwrap_or("");
    match &record.source.harness {
        // Pre-identity caches held one record per main transcript
        // `<slug>/<session>.jsonl` — exactly the layout identity is derived from
        // today: identity project = slug, session = file stem, no parent/agent, no
        // prompt fingerprint. Appended turns never enter the key, so a transcript
        // that has grown since cannot change it; the file only has to be the same
        // transcript, which the path check establishes.
        AnkaHarness::Claude => {
            let transcript = roots
                .claude_projects()
                .join(&record.source.project)
                .join(format!("{}.jsonl", record.source.session_id));
            if !transcript.is_file() {
                return Ok(None);
            }
            Ok(Some(hash(&[
                record.source.harness.as_str(),
                &record.source.project,
                &record.source.session_id,
                parent,
                agent,
                "",
            ])))
        }
        // A history record's native `session_id` never reached the old cache — it
        // stored `"<file-stem>:<line>"` — while the current importer does use it.
        // The source line is the only place that mapping can be verified.
        harness => {
            let Some(identity) = importers::recover_history_identity(
                roots,
                harness,
                &record.source.session_id,
                &record.content,
                &record.source.occurred_at,
            )?
            else {
                return Ok(None);
            };
            let prompt_fingerprint = hash(&[&record.content]);
            Ok(Some(hash(&[
                harness.as_str(),
                &identity.identity_project,
                &identity.session_id,
                parent,
                agent,
                &prompt_fingerprint,
            ])))
        }
    }
}

fn contains(value: &str, needle: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
fn modified_at(path: &Path) -> String {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|time| time.as_secs().to_string())
        .unwrap_or_else(now)
}
fn hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}
fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    fn spec(content: &str) -> RecordSpec {
        RecordSpec {
            harness: AnkaHarness::Codex,
            project: "p".into(),
            identity_project: "codex-history".into(),
            session_id: "s".into(),
            occurred_at: "1".into(),
            content: content.to_string(),
            provenance: Provenance::unknown(),
            parent_session: None,
            agent_id: None,
            discriminator: "1".into(),
            prompt_fingerprint: true,
        }
    }

    #[test]
    fn imported_content_is_redacted_before_becoming_a_record() {
        let record = record(spec("token=abcdefghijklmno"));
        assert!(!record.content.contains("abcdefghijklmno"));
        assert!(record.content.contains("REDACTED"));
    }

    #[test]
    fn exact_match_has_stronger_confidence() {
        let record = record(spec("jwt refresh rotation fixed"));
        let hit = hit_for(&record, "jwt refresh", &terms("jwt refresh")).unwrap();
        assert_eq!(hit.confidence, AnkaConfidence::Exact);
    }

    #[test]
    fn record_id_includes_the_discriminator_so_distinct_events_never_collide() {
        let mut first = spec("identical prompt");
        let mut second = spec("identical prompt");
        first.discriminator = "1".into();
        second.discriminator = "2".into();
        assert_ne!(record(first).id, record(second).id);
    }

    #[test]
    fn provenance_survives_serde_with_a_default_for_pre_phase_caches() {
        let json = r#"{"id":"x","source":{"harness":"Claude","project":"p","session_id":"s","occurred_at":"1"},"content":"c"}"#;
        let parsed: AnkaRecord = serde_json::from_str(json).expect("old cache must stay readable");
        assert_eq!(parsed.provenance, Provenance::unknown());
        assert_eq!(parsed.parent_session, None);
        assert_eq!(parsed.agent_id, None);
    }

    /// Synthetic history, cache, and config: these tests may not touch the real home,
    /// so every path is derived from a temp directory rather than from the environment.
    struct Fixture {
        roots: AnkaRoots,
        cache: PathBuf,
        config: PathBuf,
        _temp: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let cache = temp.path().join("cache").join("anka");
        let config = temp.path().join("config");
        let history = home
            .join(".local")
            .join("state")
            .join("opencode")
            .join("prompt-history.jsonl");
        fs::create_dir_all(history.parent().expect("history parent")).expect("history dir");
        fs::write(&history, "{\"text\":\"remember this prompt\"}\n").expect("history");
        Fixture {
            roots: AnkaRoots {
                home: home.clone(),
                config: config.clone(),
                cache: cache.clone(),
            },
            cache,
            config,
            _temp: temp,
        }
    }

    fn only_record_id(fixture: &Fixture) -> String {
        read_index(&fixture.cache)
            .expect("read index")
            .records
            .first()
            .expect("the fixture yields one record")
            .id
            .clone()
    }

    /// A pre-`forget_key` install's history record as it was actually written:
    /// `id` computed over `(harness, project, session, occurred_at, content)`, a
    /// `"<file-stem>:<line>"` session, no `forget_key`, no `provenance`.
    fn legacy_history_id() -> String {
        hash(&[
            AnkaHarness::Opencode.as_str(),
            "opencode-history",
            "prompt-history:1",
            "1700000000",
            "remember this prompt",
        ])
    }

    /// The key the importer derives for the fixture's history record — what the
    /// migrated tombstone has to contain to keep filtering it after a rebuild.
    fn expected_history_forget_key(session_id: &str) -> String {
        hash(&[
            AnkaHarness::Opencode.as_str(),
            "prompt-history",
            session_id,
            "",
            "",
            &hash(&["remember this prompt"]),
        ])
    }

    /// The same id shape for a Codex history record: `~/.codex/history.jsonl` stems
    /// to `history`, and the old importer hashed exactly these five inputs.
    fn legacy_codex_id() -> String {
        hash(&[
            AnkaHarness::Codex.as_str(),
            "codex-history",
            "history:1",
            "1700000000",
            "remember this prompt",
        ])
    }

    /// The key the importer derives for the Codex fixture record — identity
    /// project is the file stem (`history`), the rest matches
    /// [`expected_history_forget_key`]'s shape.
    fn expected_codex_forget_key(session_id: &str) -> String {
        hash(&[
            AnkaHarness::Codex.as_str(),
            "history",
            session_id,
            "",
            "",
            &hash(&["remember this prompt"]),
        ])
    }

    /// Write the exact on-disk state a pre-versioned install leaves behind: an index
    /// whose records carry only `id`/`source`/`content`, plus the config directory
    /// for the caller to drop a line-based tombstone file into.
    fn write_legacy_cache(fixture: &Fixture, legacy_id: &str) {
        write_legacy_cache_with_source(
            fixture,
            legacy_id,
            serde_json::json!({
                "harness": "Opencode",
                "project": "opencode-history",
                "session_id": "prompt-history:1",
                "occurred_at": "1700000000"
            }),
        );
    }

    /// [`write_legacy_cache`] with the record's `source` supplied by the caller —
    /// the legacy cache spoke for whichever harness wrote it.
    fn write_legacy_cache_with_source(
        fixture: &Fixture,
        legacy_id: &str,
        source: serde_json::Value,
    ) {
        ensure_private_dir(&fixture.cache).expect("cache dir");
        ensure_private_dir(&fixture.config).expect("config dir");
        let index = serde_json::json!({
            "records": [{
                "id": legacy_id,
                "source": source,
                "content": "remember this prompt"
            }],
            "indexed_sources": 1,
            "last_indexed_at": "2026-01-01T00:00:00Z"
        });
        fs::write(
            fixture.cache.join(INDEX_FILE),
            serde_json::to_vec(&index).expect("serialize the legacy index"),
        )
        .expect("write the legacy cache");
    }

    fn write_v1_tombstones(fixture: &Fixture, ids: &[&str]) {
        let mut body = String::new();
        for id in ids {
            body.push_str(id);
            body.push('\n');
        }
        fs::write(fixture.config.join(TOMBSTONE_FILE), body).expect("v1 tombstones");
    }

    fn coverage_of(status: &AnkaIndexStatus, harness: AnkaHarness) -> &AnkaHarnessCoverage {
        status
            .coverage
            .iter()
            .find(|entry| entry.harness == harness)
            .expect("the harness has a coverage entry")
    }

    /// The rebuild must not get past `ensure_private_dir` until the lock is free, and
    /// it must read the tombstones *after* taking it. The tombstone is written while
    /// the rebuild waits — exactly the state a `forget` completing first would leave —
    /// so a rebuild that had read tombstones up front would publish the record back.
    #[test]
    fn an_index_run_takes_the_lock_before_it_reads_tombstones() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("first build");
        let record = read_index(&fixture.cache)
            .expect("read index")
            .records
            .first()
            .expect("one record")
            .clone();
        let forgotten = record.id.clone();
        let forget_key = record.forget_key.clone();
        // Start the rebuild from a cache with no published index, so "not published
        // yet" is directly observable instead of inferred from a timestamp.
        fs::remove_file(fixture.cache.join(INDEX_FILE)).expect("drop the index");

        let held = AnkaLock::acquire(&fixture.cache).expect("hold the lock");
        let rebuild = std::thread::spawn({
            let roots = fixture.roots.clone();
            let cache = fixture.cache.clone();
            let config = fixture.config.clone();
            move || index_in(&roots, &cache, &config, None).expect("rebuild")
        });

        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !fixture.cache.join(INDEX_FILE).exists(),
            "a rebuild must not publish while another writer holds the lock"
        );

        add_tombstone(&fixture.config, &forget_key).expect("tombstone");
        drop(held);
        rebuild.join().expect("rebuild finished");

        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            !published.iter().any(|record| record.id == forgotten),
            "a tombstone written while the rebuild was waiting must still be honoured"
        );
    }

    /// `forget` is the other half of the same lock: nothing of its read-modify-write
    /// may happen while a rebuild is inside its own critical section.
    #[test]
    fn forget_waits_while_an_index_run_holds_the_lock() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let forgotten = only_record_id(&fixture);

        let held = AnkaLock::acquire(&fixture.cache).expect("hold the lock");
        let forgetter = std::thread::spawn({
            let roots = fixture.roots.clone();
            let cache = fixture.cache.clone();
            let config = fixture.config.clone();
            let id = forgotten.clone();
            move || forget_in(&roots, &cache, &config, &id).expect("forget")
        });

        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !fixture.config.join(TOMBSTONE_FILE).exists(),
            "a waiting forget must not touch the tombstone file while the lock is held"
        );
        drop(held);

        assert!(
            forgetter.join().expect("forget finished"),
            "the record exists, so the forget must succeed"
        );
        assert!(
            fixture.config.join(TOMBSTONE_FILE).exists(),
            "the tombstone lands once the lock is free"
        );
        assert!(!read_index(&fixture.cache)
            .expect("read back")
            .records
            .iter()
            .any(|record| record.id == forgotten));
    }

    /// End to end, whichever order the two writers win: a record the user forgot must
    /// not be sitting in the published index when both are done. Without the shared
    /// lock this is the resurrection bug the plan calls out.
    #[test]
    fn a_rebuild_and_a_forget_racing_each_other_never_republish_a_forgotten_record() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let forgotten = only_record_id(&fixture);

        let rebuild = {
            let roots = fixture.roots.clone();
            let cache = fixture.cache.clone();
            let config = fixture.config.clone();
            std::thread::spawn(move || index_in(&roots, &cache, &config, None).expect("rebuild"))
        };
        let forgetter = {
            let roots = fixture.roots.clone();
            let cache = fixture.cache.clone();
            let config = fixture.config.clone();
            let id = forgotten.clone();
            std::thread::spawn(move || forget_in(&roots, &cache, &config, &id).expect("forget"))
        };

        assert!(forgetter.join().expect("forget finished"));
        rebuild.join().expect("rebuild finished");

        assert!(
            fixture.config.join(TOMBSTONE_FILE).exists(),
            "the tombstone is durable"
        );
        assert!(
            !read_index(&fixture.cache)
                .expect("read back")
                .records
                .iter()
                .any(|record| record.id == forgotten),
            "the forgotten record must not be published by a rebuild that raced it"
        );
    }

    /// `forget_key` is derived only from `SourceIdentity`, so an append or mtime change
    /// cannot revive a forgotten record. The `record_id` changes (it includes the
    /// observed timestamp and discriminator), but the `forget_key` does not.
    #[test]
    fn forget_key_survives_observed_timestamp_and_content_changes() {
        let mut first = spec("same prompt");
        first.occurred_at = "1".into();
        first.discriminator = "1".into();
        let mut second = spec("same prompt");
        second.occurred_at = "2".into();
        second.discriminator = "2".into();

        let first = record(first);
        let second = record(second);

        assert_ne!(
            first.id, second.id,
            "distinct events never share a record id"
        );
        assert_eq!(
            first.forget_key, second.forget_key,
            "the same prompt in the same source namespace shares a forget group"
        );
    }

    /// A transcript-based record (Claude) identifies by its source alone — no prompt
    /// fingerprint — so appending to the transcript does not change the forget key.
    #[test]
    fn transcript_records_identify_by_source_not_content() {
        let mut first = spec("first turn");
        first.harness = AnkaHarness::Claude;
        first.prompt_fingerprint = false;
        let mut second = spec("appended turn");
        second.harness = AnkaHarness::Claude;
        second.prompt_fingerprint = false;

        let first = record(first);
        let second = record(second);

        assert_eq!(
            first.forget_key, second.forget_key,
            "appending to a transcript must not change the forget group"
        );
    }

    /// `forget` writes the `forget_key` to the tombstone, not the `record_id`. The
    /// tombstone is durable across rebuilds, so it must reference the stable identity.
    #[test]
    fn forget_writes_the_forget_key_to_the_tombstone() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let record = read_index(&fixture.cache)
            .expect("read index")
            .records
            .first()
            .expect("one record")
            .clone();
        let record_id = record.id.clone();
        let forget_key = record.forget_key.clone();
        assert_ne!(record_id, forget_key, "the two handles must be distinct");

        forget_in(&fixture.roots, &fixture.cache, &fixture.config, &record_id).expect("forget");

        let tombstones = read_tombstones(&fixture.config).expect("tombstones");
        assert!(
            tombstones.contains(&TombstoneEntry::forget_key(&forget_key)),
            "the tombstone must reference the durable forget key"
        );
        assert!(
            !tombstones.iter().any(|entry| entry.value == record_id),
            "the tombstone must not reference the volatile record id"
        );
    }

    /// A forgotten record stays hidden even after a rebuild that would have republished
    /// it — the tombstone is checked on recall, not only during the rebuild.
    #[test]
    fn a_forgotten_record_stays_hidden_after_a_rebuild() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let forgotten = only_record_id(&fixture);

        forget_in(&fixture.roots, &fixture.cache, &fixture.config, &forgotten).expect("forget");
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("rebuild");

        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "a tombstone must survive a rebuild"
        );
    }

    /// The rebuild itself must not republish a forgotten record — not just hide it from
    /// recall. The published index is the source of truth for `status` and for any
    /// future reader that trusts it, so the filter belongs in `index_in` too.
    #[test]
    fn a_rebuild_does_not_republish_a_forgotten_record() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let forgotten = only_record_id(&fixture);

        forget_in(&fixture.roots, &fixture.cache, &fixture.config, &forgotten).expect("forget");
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("rebuild");

        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            !published.iter().any(|record| record.id == forgotten),
            "the published index must not contain the forgotten record"
        );
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "recall must also return nothing"
        );
    }

    /// A provenance transition (Unknown → SessionCwd) must not change the forget key.
    /// The history importer resolves the display project from provenance, but the
    /// durable identity uses the stable source namespace (history file stem).
    #[test]
    fn provenance_transition_does_not_change_forget_key() {
        let mut unknown = spec("same prompt");
        unknown.project = "codex-history".into();
        unknown.provenance = Provenance::unknown();
        let mut resolved = spec("same prompt");
        resolved.project = "/home/alaz/dev/ai/svp".into();
        resolved.provenance = Provenance::direct(
            "/home/alaz/dev/ai/svp",
            &AnkaRoots {
                home: PathBuf::from("/home/alaz"),
                config: PathBuf::from("/home/alaz/.config/raios"),
                cache: PathBuf::from("/home/alaz/.cache/raios/anka"),
            },
        );

        let unknown = record(unknown);
        let resolved = record(resolved);

        assert_ne!(
            unknown.source.project, resolved.source.project,
            "the display project differs — that is the point"
        );
        assert_eq!(
            unknown.forget_key, resolved.forget_key,
            "a provenance transition must not change the forget group"
        );
    }

    /// `add_tombstone` must only treat `NotFound` as an empty start. Any other read
    /// error (permissions, I/O) must leave the file untouched rather than being
    /// silently treated as empty and then overwritten.
    #[test]
    fn add_tombstone_propagates_read_errors_other_than_not_found() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("config");
        ensure_private_dir(&config).expect("config dir");
        let path = config.join(TOMBSTONE_FILE);
        let seed = br#"{"schema_version":2,"entries":[]}"#;
        fs::write(&path, seed).expect("seed");

        // Make the file unreadable by removing read permission.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("remove read permission");

        let key = hash(&["key"]);
        let result = add_tombstone(&config, &key);
        assert!(
            result.is_err(),
            "a read error must propagate, not be treated as empty"
        );

        // Restore permissions before reading back.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restore permissions");
        assert_eq!(
            fs::read(&path).expect("read back"),
            seed,
            "the file must be untouched after a failed read"
        );
    }

    fn query(text: &str) -> AnkaSearchQuery {
        AnkaSearchQuery {
            text: text.to_string(),
            project: None,
            harness: None,
            limit: 10,
        }
    }

    /// Step 3's acceptance criterion: the tombstone is durable, the index publish that
    /// should have followed it failed, and the stale index still holds the record.
    /// Recall must not show it — filtering tombstones only inside the rebuild would
    /// leave exactly this state searchable until some future `raios anka index`, which
    /// may never run.
    ///
    /// The sequence is `forget_in`'s own, replayed step by step under the same lock so
    /// the index publish can be made to fail deterministically: the cache directory is
    /// made unwritable between the tombstone and the publish. No threads, no timing.
    #[test]
    fn a_durable_tombstone_hides_a_record_even_when_the_index_publish_fails() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let record = read_index(&fixture.cache)
            .expect("read index")
            .records
            .first()
            .expect("one record")
            .clone();
        let forgotten = record.id.clone();
        let forget_key = record.forget_key.clone();

        assert_eq!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .len(),
            1,
            "the record is recallable before the forget"
        );

        let lock = AnkaLock::acquire(&fixture.cache).expect("the same lock forget_in takes");
        let mut index = read_index(&fixture.cache).expect("read index");
        add_tombstone(&fixture.config, &forget_key).expect("the tombstone is durable");
        index.records.retain(|record| record.id != forgotten);
        fs::set_permissions(&fixture.cache, fs::Permissions::from_mode(0o500))
            .expect("make the cache unwritable");
        let failure = write_index(&fixture.cache, &index).expect_err("the publish must fail");
        let phase = failure
            .downcast_ref::<publish::PublishError>()
            .expect("the publish failure must keep its phase");
        assert!(
            !phase.is_published(),
            "the failure happened before the rename, so the old index is intact: {phase}"
        );
        fs::set_permissions(&fixture.cache, fs::Permissions::from_mode(0o700))
            .expect("make the cache writable again");
        drop(lock);

        // The on-disk state a failed publish leaves behind: durable tombstone, stale
        // index still holding the record.
        assert!(
            read_tombstones(&fixture.config)
                .expect("tombstones")
                .contains(&TombstoneEntry::forget_key(&forget_key)),
            "the tombstone survived the failed publish"
        );
        assert!(
            read_index(&fixture.cache)
                .expect("read back")
                .records
                .iter()
                .any(|record| record.id == forgotten),
            "the index is stale, which is the point: recall must not trust it"
        );

        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "a durable tombstone must hide the record even when the index was never rewritten"
        );
        assert!(
            blame_in(&fixture.cache, &fixture.config, "prompt-history.jsonl", 10)
                .expect("blame")
                .is_empty(),
            "blame routes through the same tombstone-aware path"
        );
    }

    /// A tombstone publish that fails before the rename leaves the previous tombstone
    /// file byte for byte — the "rollback" is the atomic publish itself: nothing reaches
    /// the destination until the replacement is complete, so there is no partial state
    /// to clean up and no old data to restore by hand.
    #[test]
    fn a_failed_tombstone_add_leaves_the_previous_tombstones_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("config");
        ensure_private_dir(&config).expect("config dir");
        let path = config.join(TOMBSTONE_FILE);
        let seed = br#"{"schema_version":2,"entries":[]}"#;
        fs::write(&path, seed).expect("seed the tombstone file");

        // 0500 keeps the file readable (so the read half still succeeds) but the
        // directory unwritable, which is where the temp file would be created.
        fs::set_permissions(&config, fs::Permissions::from_mode(0o500))
            .expect("make the config dir unwritable");
        let key = hash(&["another-record"]);
        let failure = add_tombstone(&config, &key).expect_err("the publish must fail");
        let phase = failure
            .downcast_ref::<publish::PublishError>()
            .expect("the publish failure must keep its phase");
        assert!(
            !phase.is_published(),
            "the failure happened before the rename: {phase}"
        );
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700))
            .expect("make the config dir writable again");

        assert_eq!(
            fs::read(&path).expect("read back"),
            seed,
            "a failed publish must not touch the previous tombstones"
        );
    }

    /// Migration from real pre-versioned bytes: an index a 3.x install wrote
    /// (records without `forget_key`) and a line-based tombstone file of record ids.
    /// The tombstone's record is still in the cache, so it resolves; the key is
    /// re-derived the way the importer derives it; the rebuilt index is filtered by
    /// the translated key.
    #[test]
    fn a_legacy_tombstone_migrates_through_the_old_cache_record() {
        let fixture = fixture();
        let legacy_id = legacy_history_id();
        write_legacy_cache(&fixture, &legacy_id);
        write_v1_tombstones(&fixture, &[&legacy_id]);

        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("migration and rebuild");

        let expected_key = expected_history_forget_key("prompt-history");
        let entries = read_tombstones(&fixture.config).expect("tombstones");
        assert!(
            entries.contains(&TombstoneEntry::forget_key(&expected_key)),
            "the legacy id must translate into the key the importer derives: {entries:?}"
        );
        assert!(
            entries.contains(&TombstoneEntry::record_id(&legacy_id)),
            "the legacy entry is retained so the not-yet-replaced cache stays hidden"
        );
        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            !published
                .iter()
                .any(|record| record.forget_key == expected_key),
            "the translated key must filter the record out of the rebuilt index"
        );
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "the record must not be recallable after the migration"
        );
    }

    /// A tombstone whose record an older `forget` already deleted from the cache has
    /// no match — expected, and precisely why the migration must stop instead of
    /// "resolving" it by dropping the tombstone, which would resurrect the record on
    /// the next rebuild. Nothing is published: tombstone file byte-identical, index
    /// untouched, and `forget` held to the same rule while migration is blocked.
    #[test]
    fn an_unresolvable_legacy_tombstone_blocks_the_migration_and_publishes_nothing() {
        let fixture = fixture();
        let present = legacy_history_id();
        let missing = hash(&["a record an older forget removed from the cache"]);
        write_legacy_cache(&fixture, &present);
        write_v1_tombstones(&fixture, &[&missing, &present]);

        let tombstone_path = fixture.config.join(TOMBSTONE_FILE);
        let index_path = fixture.cache.join(INDEX_FILE);
        let tombstones_before = fs::read(&tombstone_path).expect("tombstone bytes");
        let index_before = fs::read(&index_path).expect("index bytes");

        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("one unresolvable id must stop the whole migration");
        assert!(error.to_string().contains("migration blocked"), "{error}");
        assert_eq!(
            fs::read(&tombstone_path).expect("tombstone bytes"),
            tombstones_before,
            "no new tombstone file may be published"
        );
        assert_eq!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "no index may be published"
        );

        // `forget` runs the same migration first and must publish nothing either.
        let forget_error = forget_in(&fixture.roots, &fixture.cache, &fixture.config, &present)
            .expect_err("forget must not write past a blocked migration");
        assert!(
            forget_error.to_string().contains("migration blocked"),
            "{forget_error}"
        );
        assert_eq!(
            fs::read(&tombstone_path).expect("tombstone bytes"),
            tombstones_before,
            "a refused forget must not touch the tombstones"
        );
        assert_eq!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "a refused forget must not touch the index"
        );

        // Recall keeps honouring the untouched legacy pair meanwhile.
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("recall stays available")
            .is_empty(),
            "the record whose id is tombstoned stays hidden"
        );
    }

    /// Recall reads tombstones too: a corrupt file must surface as an error on both
    /// paths, never as "nothing is forgotten" — ignoring it would show records the
    /// user hid. Hash appearance is not a format: a versioned file that fails
    /// validation, an unknown schema version, and a headerless non-hash line are all
    /// corruption.
    #[test]
    fn a_corrupt_tombstone_file_is_reported_instead_of_ignored() {
        let fixture = fixture();
        ensure_private_dir(&fixture.config).expect("config dir");
        let path = fixture.config.join(TOMBSTONE_FILE);

        fs::write(
            &path,
            br#"{"schema_version":2,"entries":[{"type":"forget_key","value":"not-a-hash"}]}"#,
        )
        .expect("invalid entry");
        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("a corrupt tombstone file must stop the rebuild");
        assert!(error.to_string().contains("corrupt"), "{error}");
        let recall = search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("recall must fail safe, not ignore corruption");
        assert!(recall.to_string().contains("corrupt"), "{recall}");

        fs::write(&path, br#"{"schema_version":3,"entries":[]}"#).expect("unknown version");
        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("an unknown schema version must stop the rebuild");
        assert!(error.to_string().contains("schema version"), "{error}");

        fs::write(&path, b"not-a-hash\n").expect("invalid legacy line");
        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("a headerless non-hash line is corruption");
        assert!(error.to_string().contains("corrupt"), "{error}");
        search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("recall must fail safe here too");
    }

    /// The migration publishes translated tombstones first and the replacement index
    /// after; a crash in between leaves exactly that state. It must be safe: the old
    /// cache and the new tombstones together keep the record hidden, and retrying
    /// converges on byte-identical tombstones — a versioned file is never
    /// re-translated, only re-published, so every later run re-runs the fsync chain
    /// instead of assuming a previous sync landed.
    #[test]
    fn a_crash_between_the_tombstone_and_index_publish_is_safe_and_retries_idempotently() {
        let fixture = fixture();
        let legacy_id = legacy_history_id();
        let expected_key = expected_history_forget_key("prompt-history");
        write_legacy_cache(&fixture, &legacy_id);
        write_v1_tombstones(&fixture, &[&legacy_id]);

        let tombstone_path = fixture.config.join(TOMBSTONE_FILE);
        let index_path = fixture.cache.join(INDEX_FILE);
        let index_before = fs::read(&index_path).expect("index bytes");

        // Crash right after the tombstone publish: `migrate_tombstones` on its own is
        // the first half of `index_in`.
        migrate_tombstones(&fixture.roots, &fixture.cache, &fixture.config)
            .expect("translate the tombstones");
        assert_eq!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "tombstones are published first; the replacement index is not"
        );
        let translated = fs::read(&tombstone_path).expect("translated bytes");

        // Old cache + new tombstones must not expose the record.
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("recall")
            .is_empty(),
            "the retained record_id entry must keep the old cache's record hidden"
        );

        // Retry: idempotent — the versioned file is re-published, never
        // re-translated, so the entries stay byte-identical.
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("the retry must succeed");
        assert_eq!(
            fs::read(&tombstone_path).expect("translated bytes"),
            translated,
            "a retry must republish byte-identical tombstones"
        );
        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            !published
                .iter()
                .any(|record| record.forget_key == expected_key),
            "the rebuilt index must exclude the record"
        );
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "the record must stay hidden after the retry"
        );

        // A second full run changes nothing either.
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("the second run");
        assert_eq!(
            fs::read(&tombstone_path).expect("translated bytes"),
            translated,
            "re-running the migration must leave the tombstones byte-identical"
        );
    }

    /// `PublishedUnsynced` is not success on the tombstone side: the entries are
    /// visible, but visibility is what the caller is *stopping* over — the
    /// replacement index may only ride on a tombstone whose crash-durability was
    /// confirmed. Converting the error to `Ok(())` here would let exactly that
    /// happen, silently.
    #[test]
    fn a_tombstone_publish_that_lands_without_confirmation_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("config");
        ensure_private_dir(&config).expect("config dir");
        let key = hash(&["unconfirmed key"]);
        let entries = BTreeSet::from([TombstoneEntry::forget_key(&key)]);

        let error = write_tombstones_with(&config, &entries, |path, bytes| {
            publish::atomic_write_with(path, bytes, |_| {
                anyhow::bail!("injected: directory sync failed")
            })
        })
        .expect_err("an unconfirmed tombstone publish must not read as success");
        let publish_error = error
            .downcast_ref::<publish::PublishError>()
            .expect("the typed publish error must reach the caller");
        assert!(
            matches!(publish_error, publish::PublishError::PublishedUnsynced(_)),
            "{error}"
        );
        assert!(
            publish_error.is_published(),
            "the entries are visible — only durability is missing"
        );
        assert_eq!(
            read_tombstones(&config).expect("the visible half"),
            entries,
            "the destination already holds the new set"
        );
    }

    /// A tombstone publish can land without its directory fsync — the file is
    /// visible, its crash-durability unconfirmed. The index publish must not follow
    /// it (records are hidden by durability, not visibility), and the retry must
    /// re-publish the versioned file — re-running the fsync chain — instead of
    /// treating "already versioned" as "already durable".
    #[test]
    fn an_unconfirmed_tombstone_publish_stops_the_index_and_a_retry_confirms_durability() {
        use std::os::unix::fs::MetadataExt;

        let fixture = fixture();
        let legacy_id = legacy_history_id();
        write_legacy_cache(&fixture, &legacy_id);
        write_v1_tombstones(&fixture, &[&legacy_id]);
        let tombstone_path = fixture.config.join(TOMBSTONE_FILE);
        let index_path = fixture.cache.join(INDEX_FILE);
        let index_before = fs::read(&index_path).expect("index bytes");

        // The owner may create and rename inside the directory but not open it for
        // reading — precisely where `sync_directory` runs, after the rename.
        fs::set_permissions(&fixture.config, fs::Permissions::from_mode(0o300))
            .expect("restrict the config dir");

        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("the index must not publish on an unconfirmed tombstone");
        let publish_error = error
            .downcast_ref::<publish::PublishError>()
            .expect("the typed publish error must reach the caller");
        assert!(
            matches!(publish_error, publish::PublishError::PublishedUnsynced(_)),
            "{error}"
        );
        assert_eq!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "the index publish must not be reached"
        );
        // The visible half did land: the old cache plus these tombstones keep the
        // record hidden across the window.
        let visible = fs::read_to_string(&tombstone_path).expect("tombstone bytes");
        assert!(visible.contains("\"schema_version\":2"), "{visible}");
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("recall")
            .is_empty(),
            "the record stays hidden across the window"
        );

        // Retry with a usable directory: the versioned file must be re-published —
        // new inode, fsync chain re-run — not skipped as already durable.
        let inode_before = fs::metadata(&tombstone_path)
            .expect("tombstone metadata")
            .ino();
        fs::set_permissions(&fixture.config, fs::Permissions::from_mode(0o700))
            .expect("restore the config dir");
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("the retry must succeed");
        let inode_after = fs::metadata(&tombstone_path)
            .expect("tombstone metadata")
            .ino();
        assert_ne!(
            inode_before, inode_after,
            "the retry must re-publish the versioned file so its durability is confirmed"
        );
        assert_eq!(
            fs::read_to_string(&tombstone_path).expect("tombstone bytes"),
            visible,
            "re-publishing changes nothing about the entries"
        );
        assert_ne!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "after the retry the replacement index lands"
        );
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "the record stays hidden after the rebuild"
        );
    }

    /// The legacy cache stored `"<file-stem>:<line>"` as the session and dropped the
    /// entry's native `session_id`, which the current importer does use. Migration
    /// must recover it from the source line: deriving the stem instead would produce
    /// a key that misses the rebuilt record — and the prompt would come back.
    #[test]
    fn a_legacy_history_tombstone_recovers_the_native_session_id() {
        let fixture = fixture();
        fs::write(
            fixture.roots.opencode_history(),
            "{\"session_id\":\"SID-9\",\"text\":\"remember this prompt\"}\n",
        )
        .expect("history");
        let legacy_id = legacy_history_id();
        write_legacy_cache(&fixture, &legacy_id);
        write_v1_tombstones(&fixture, &[&legacy_id]);

        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("migration and rebuild");

        let expected_key = expected_history_forget_key("SID-9");
        assert!(
            read_tombstones(&fixture.config)
                .expect("tombstones")
                .contains(&TombstoneEntry::forget_key(&expected_key)),
            "the recovered native session must enter the derived key"
        );
        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            !published
                .iter()
                .any(|record| record.source.session_id == "SID-9"),
            "the rebuilt record must be filtered by the recovered key"
        );
        assert!(
            search_in(
                &fixture.cache,
                &fixture.config,
                query("remember this prompt")
            )
            .expect("search")
            .is_empty(),
            "the record must not reappear after migration"
        );
    }

    /// A rotation can leave *another session's event* at the line the old cache
    /// recorded — same prompt text, different timestamp. Text equality cannot tell
    /// those apart, and deriving a key from the wrong event would silently stop
    /// hiding the record after the next rebuild, so for Codex the re-read line must
    /// carry the exact event time the cache stored; an indistinguishable match
    /// stops the migration with nothing published until the source is resolved.
    #[test]
    fn a_legacy_migration_stops_when_the_same_prompt_sits_under_another_event() {
        let fixture = fixture();
        let codex_history = fixture.roots.codex_history();
        fs::create_dir_all(codex_history.parent().expect("codex dir")).expect("codex dir");
        // The line now holds a different event: rotated in, same prompt, another
        // session, another timestamp than the cache recorded.
        fs::write(
            &codex_history,
            "{\"ts\":1700000099,\"session_id\":\"SID-ROTATED\",\"text\":\"remember this prompt\"}\n",
        )
        .expect("rotated codex history");
        let legacy_id = legacy_codex_id();
        write_legacy_cache_with_source(
            &fixture,
            &legacy_id,
            serde_json::json!({
                "harness": "Codex",
                "project": "codex-history",
                "session_id": "history:1",
                "occurred_at": "1700000000"
            }),
        );
        write_v1_tombstones(&fixture, &[&legacy_id]);
        let tombstone_path = fixture.config.join(TOMBSTONE_FILE);
        let index_path = fixture.cache.join(INDEX_FILE);
        let tombstone_before = fs::read(&tombstone_path).expect("tombstone bytes");
        let index_before = fs::read(&index_path).expect("index bytes");

        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("an indistinguishable match must stop the migration");
        assert!(error.to_string().contains("migration blocked"), "{error}");
        assert_eq!(
            fs::read(&tombstone_path).expect("tombstone bytes"),
            tombstone_before,
            "nothing was published while the mapping is unverifiable"
        );
        assert_eq!(
            fs::read(&index_path).expect("index bytes"),
            index_before,
            "the cache is untouched while the migration is blocked"
        );

        // Resolve explicitly: the line *is* the cached event — same prompt, the
        // event timestamp the cache recorded — and the key is derived from it.
        fs::write(
            &codex_history,
            "{\"ts\":1700000000,\"session_id\":\"SID-CX\",\"text\":\"remember this prompt\"}\n",
        )
        .expect("resolved codex history");
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("the resolved migration proceeds");
        assert!(
            read_tombstones(&fixture.config)
                .expect("tombstones")
                .contains(&TombstoneEntry::forget_key(&expected_codex_forget_key(
                    "SID-CX"
                ))),
            "the key is derived from the verified event's session"
        );
        assert!(
            !read_index(&fixture.cache)
                .expect("read back")
                .records
                .iter()
                .any(|record| record.source.session_id == "SID-CX"),
            "the rebuilt index excludes the record the verified key covers"
        );
    }

    /// The envelope a pre-step-4 install cannot read: no top-level `records` or
    /// `indexed_sources` — the exact fields its reader *requires*. Serde ignores
    /// what it does not recognize but cannot invent what is missing, so the old
    /// reader fails on this file with its own corrupt-index message instead of
    /// accepting a cache it only half understands.
    #[test]
    fn an_old_cache_reader_cannot_accept_the_versioned_envelope() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

        let raw = fs::read_to_string(fixture.cache.join(INDEX_FILE)).expect("cache bytes");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
        assert_eq!(
            value
                .get("schema_version")
                .and_then(|version| version.as_u64()),
            Some(2),
            "the envelope must carry its version"
        );
        assert!(
            value.get("records").is_none() && value.get("indexed_sources").is_none(),
            "no legacy top-level field name may survive into the envelope: {raw}"
        );
        serde_json::from_value::<AnkaIndexV1>(value).expect_err(
            "the old reader requires `records` and `indexed_sources` — this file must fail it",
        );
    }

    /// A cache stamped with a schema this build does not know is refused,
    /// described, and left alone: recall surfaces the refusal rather than
    /// falling back to the legacy reader, `status` reports it as `incompatible`,
    /// and even a rebuild declines to overwrite it — that file may hold data
    /// only a newer build understands, so destroying it is the user's explicit
    /// move, never a side effect of asking what time it is.
    #[test]
    fn an_unknown_cache_schema_version_is_refused_described_and_never_overwritten() {
        let fixture = fixture();
        ensure_private_dir(&fixture.cache).expect("cache dir");
        let path = fixture.cache.join(INDEX_FILE);
        let future = br#"{"schema_version":3,"records_v2":[],"coverage":[]}"#;
        fs::write(&path, future).expect("future cache");

        let recall = search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("recall must fail safe against a cache it cannot read");
        assert!(recall.to_string().contains("schema version 3"), "{recall}");
        assert_eq!(
            fs::read(&path).expect("cache bytes"),
            future,
            "recall must not migrate or rebuild as a side effect"
        );

        let status = status_in(&fixture.cache).expect("status describes the cache");
        assert_eq!(status.state, AnkaCacheState::Incompatible);
        assert_eq!(status.indexed_records, 0);
        assert_eq!(status_dto(status).state, "incompatible");

        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("a rebuild must not overwrite a newer cache");
        assert!(error.to_string().contains("schema version 3"), "{error}");
        assert_eq!(
            fs::read(&path).expect("cache bytes"),
            future,
            "the newer cache must be untouched"
        );
    }

    /// Only `NotFound` reads as "nothing to lose". An existing cache this build
    /// cannot read must stop the rebuild — otherwise a permission problem would be
    /// mistaken for an absent cache and the file would be silently replaced, losing
    /// whatever it held.
    #[test]
    fn an_unreadable_cache_blocks_the_rebuild_instead_of_being_replaced() {
        let fixture = fixture();
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
        let path = fixture.cache.join(INDEX_FILE);
        let before = fs::read(&path).expect("cache bytes");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("make the cache unreadable");
        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("an unreadable cache must not be mistaken for an absent one");
        assert!(error.to_string().contains("could not read"), "{error}");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restore permissions");
        assert_eq!(
            fs::read(&path).expect("cache bytes"),
            before,
            "the unreadable cache must be untouched"
        );
    }

    /// A `schema_version` that is present but not a number is refused with its own
    /// message, everywhere: recall fails safe, `status` reports the cache as
    /// `incompatible`, and a rebuild declines rather than overwrite a file this
    /// build cannot even interpret — unlike corrupt bytes, an identifiable version
    /// field means *something else wrote this*.
    #[test]
    fn an_invalid_schema_version_type_is_refused_described_and_never_overwritten() {
        let fixture = fixture();
        ensure_private_dir(&fixture.cache).expect("cache dir");
        let path = fixture.cache.join(INDEX_FILE);
        let invalid = br#"{"schema_version":"two","records":[],"indexed_sources":0}"#;
        fs::write(&path, invalid).expect("invalid cache");

        let recall = search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("recall must fail safe against a version it cannot interpret");
        assert!(
            recall
                .to_string()
                .contains("schema_version is not a number"),
            "{recall}"
        );
        assert_eq!(
            fs::read(&path).expect("cache bytes"),
            invalid,
            "recall must not migrate or rebuild as a side effect"
        );

        let status = status_in(&fixture.cache).expect("status describes the cache");
        assert_eq!(status.state, AnkaCacheState::Incompatible);

        let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("a rebuild must not overwrite a version it cannot interpret");
        assert!(
            error.to_string().contains("schema_version is not a number"),
            "{error}"
        );
        assert_eq!(
            fs::read(&path).expect("cache bytes"),
            invalid,
            "the refused cache must be untouched"
        );
    }

    /// The unreadable-file and invalid-version refusals must not swallow the
    /// behavior that motivates `raios anka index` in the first place: bytes that
    /// parse as nothing at all are corrupt, and a full rebuild fixes them.
    #[test]
    fn a_corrupt_index_stays_rebuildable() {
        let fixture = fixture();
        ensure_private_dir(&fixture.cache).expect("cache dir");
        fs::write(fixture.cache.join(INDEX_FILE), b"{definitely not json").expect("corrupt cache");

        let status = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect("a corrupt cache is exactly what a rebuild is for");
        assert_eq!(status.state, AnkaCacheState::Ready);
        assert!(
            status.indexed_records > 0,
            "the rebuilt cache holds the fixture's records"
        );
    }

    /// A harness-scoped refresh replaces only that harness's slice. The other
    /// harnesses' records carry forward — still re-filtered through the current
    /// tombstones and exclusions — and their coverage entries stay verbatim, so
    /// their `full_refresh` flags survive and the scoped run stays visible as
    /// scoped instead of the whole cache pretending to have been refreshed.
    #[test]
    fn a_harness_scoped_refresh_preserves_the_other_harnesses_and_their_coverage() {
        let fixture = fixture();
        // A second harness, with two prompts of its own.
        let codex_history = fixture.roots.codex_history();
        fs::create_dir_all(codex_history.parent().expect("codex dir")).expect("codex dir");
        fs::write(
            &codex_history,
            "{\"text\":\"first codex prompt\"}\n{\"text\":\"second codex prompt\"}\n",
        )
        .expect("codex history");

        let full =
            index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("full build");
        assert_eq!(
            full.state,
            AnkaCacheState::Ready,
            "a full run covers every harness"
        );
        assert_eq!(
            full.coverage.len(),
            AnkaHarness::ALL.len(),
            "every harness gets an entry, discovered or not"
        );
        assert!(
            full.coverage.iter().all(|entry| entry.full_refresh),
            "a full run refreshes every harness"
        );
        assert_eq!(coverage_of(&full, AnkaHarness::Codex).records, 2);
        assert_eq!(coverage_of(&full, AnkaHarness::Opencode).records, 1);

        // Scoped: the codex history turns over — one line leaves, one arrives.
        fs::write(
            &codex_history,
            "{\"text\":\"second codex prompt\"}\n{\"text\":\"third codex prompt\"}\n",
        )
        .expect("codex history updated");
        let scoped = index_in(
            &fixture.roots,
            &fixture.cache,
            &fixture.config,
            Some(AnkaHarness::Codex),
        )
        .expect("scoped refresh");

        assert_eq!(
            scoped.state,
            AnkaCacheState::Ready,
            "the cache still speaks for every harness"
        );
        assert_eq!(
            coverage_of(&scoped, AnkaHarness::Codex).records,
            2,
            "the scoped slice was rebuilt"
        );
        assert!(
            !coverage_of(&scoped, AnkaHarness::Codex).full_refresh,
            "a scoped run must not claim to be a full refresh"
        );
        assert_eq!(
            coverage_of(&scoped, AnkaHarness::Opencode).records,
            1,
            "the other harness's coverage carries forward"
        );
        assert!(
            coverage_of(&scoped, AnkaHarness::Opencode).full_refresh,
            "carried-forward coverage keeps the flag it was written with"
        );

        let published = read_index(&fixture.cache).expect("read back").records;
        assert!(
            published
                .iter()
                .any(|record| record.content == "remember this prompt"),
            "the other harness's record must survive a scoped refresh"
        );
        assert!(
            published
                .iter()
                .any(|record| record.content == "third codex prompt"),
            "the refreshed slice must carry the new line"
        );
        assert!(
            !published
                .iter()
                .any(|record| record.content == "first codex prompt"),
            "the line that left the source must leave the cache"
        );
    }

    /// `oversized` is its own exclusion reason in the coverage, not folded into
    /// malformed: the line was never parsed — its content never materialized —
    /// so counting it as malformed would describe an event that did not happen.
    #[test]
    fn oversized_lines_are_reported_as_their_own_coverage_reason() {
        let fixture = fixture();
        let oversized_line = "a".repeat(provenance::HISTORY_LINE_MAX_BYTES + 1);
        fs::write(
            fixture.roots.opencode_history(),
            format!("{{\"text\":\"remember this prompt\"}}\n{oversized_line}\n"),
        )
        .expect("history with an oversized line");

        let status =
            index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

        let opencode = coverage_of(&status, AnkaHarness::Opencode);
        assert_eq!(opencode.oversized, 1, "the oversized line counts as itself");
        assert_eq!(
            opencode.records, 1,
            "the healthy line still yields its record"
        );
        assert_eq!(opencode.sources, 1, "the file was still discovered");
        assert_eq!(
            coverage_of(&status, AnkaHarness::Codex).oversized,
            0,
            "the reason is scoped to the harness it happened in"
        );
    }

    /// Before anything has been indexed there is no cache, and `status` says so
    /// as `empty` — derived from coverage, which has none, and not from a
    /// timestamp that does not exist.
    #[test]
    fn status_is_empty_before_the_first_index() {
        let temp = tempfile::tempdir().expect("tempdir");
        let status = status_in(&temp.path().join("cache")).expect("status");
        assert_eq!(status.state, AnkaCacheState::Empty);
        assert!(status.coverage.is_empty());
        assert_eq!(
            status_dto(status).state,
            "empty",
            "the surfaces receive the same state over the wire"
        );
    }

    /// A pre-envelope cache carries no coverage, so it is reconstructed from
    /// what the records prove: which harnesses it speaks for, and when it was
    /// written. Everything else reports the honest zero/false — and because
    /// state comes from coverage rather than `last_indexed_at`, a legacy cache
    /// whose records came from one harness reads as `partial`, not as the
    /// `ready` the old timestamp-based guess would have claimed.
    #[test]
    fn a_legacy_cache_reports_partial_status_with_reconstructed_coverage() {
        let fixture = fixture();
        write_legacy_cache(&fixture, &legacy_history_id());

        let status = status_in(&fixture.cache).expect("status");
        assert_eq!(status.state, AnkaCacheState::Partial);
        assert_eq!(status.indexed_records, 1);
        assert_eq!(
            status.indexed_sources, 1,
            "the legacy total is preserved on read"
        );
        assert_eq!(
            status.last_indexed_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            status.coverage.len(),
            1,
            "only the harness with records speaks"
        );
        let entry = &status.coverage[0];
        assert_eq!(entry.harness, AnkaHarness::Opencode);
        assert_eq!(entry.records, 1);
        assert_eq!(
            entry.sources, 0,
            "a legacy cache never recorded per-harness sources"
        );
        assert_eq!(
            entry.indexed_at, "2026-01-01T00:00:00Z",
            "the only timing it has"
        );
        assert!(!entry.full_refresh, "the refresh scope was never persisted");
        assert_eq!(entry.oversized, 0, "nor were the old exclusion counts");
    }
}
