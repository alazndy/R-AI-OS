//! ANKA read-only transcript recall with a rebuildable sidecar cache.
//!
//! Platform: the path model is POSIX-only by design. `provenance::normalize_path`
//! accepts only paths rooted at `/`, and directory `fsync` follows unix semantics,
//! so on Windows `AnkaPolicy::init` fails closed with "ANKA cannot normalize the
//! home path …" and a publish would report its durability as unconfirmed. Tests
//! that assert those semantics are `#[cfg(unix)]`; a Windows-aware path mode is a
//! deliberate follow-up, not silent partial support.

pub mod provenance;

mod importers;
pub mod lock;
pub mod policy;
mod publish;

use anyhow::{bail, Context, Result};
use lock::AnkaLock;
use provenance::{AnkaRoots, Provenance};
// Re-exported so the CLI and MCP surfaces read provenance through one path instead of
// reaching into the module layout; the shared output types are part of the contract.
pub use policy::{HomeChoice, PolicyInitView, PolicyShowView};
pub use provenance::{ProjectScope, ProvenanceQuality, TimeSource};
use publish::{atomic_write, ensure_private_dir};
use raios_core::anka::{
    default_cache_path, AnkaCacheState, AnkaConfidence, AnkaHarness, AnkaHarnessCoverage, AnkaHit,
    AnkaIndexStatus, AnkaPolicySummary, AnkaSearchQuery, AnkaSourceRef,
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
    // Consent first: nothing — not even tombstone migration — runs against a
    // machine whose policy is missing, malformed, or unreadable. Fail closed
    // before the lock and before any write.
    let policy = policy::AnkaPolicy::load(config)?;
    ensure_private_dir(cache_path)?;
    let _lock = AnkaLock::acquire(cache_path)?;
    // Never write over a cache this build cannot read. A *corrupt* cache stays
    // rebuildable (its own message tells the user to run `raios anka index`);
    // an unknown schema version does not — that file may hold data only a newer
    // build understands.
    check_cache_schema(cache_path)?;
    migrate_tombstones(roots, cache_path, config)?;
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
    // previous refresh. Tombstones and the policy share this gate, so what the
    // index publishes is exactly the set recall would admit.
    records.retain(|record| !tombstoned(record, &tombstones) && policy.admit(record).is_none());
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
    let mut index = AnkaIndex {
        records,
        indexed_sources: coverage.iter().map(|entry| entry.sources).sum(),
        coverage,
        last_indexed_at: Some(refreshed_at),
    };
    recount_coverage(&mut index);
    write_index(cache_path, &index)?;
    Ok(status_from_index(
        cache_path.to_path_buf(),
        &index,
        policy.own_summary(),
    ))
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
    status_in(&default_cache_path(), &config_dir())
}

/// Status, with the cache's *readability* handled explicitly: a cache whose
/// format this build cannot follow is reported as [`AnkaCacheState::Incompatible`]
/// rather than as an error, because describing the cache is exactly what `status`
/// is for. Recall keeps propagating the same failure — it must fail safe, not
/// describe. The policy summary follows the same rule in reverse: it never
/// fails the status, because "no policy" is a state to report, not a reason to
/// stop describing the cache.
fn status_in(cache_path: &Path, config: &Path) -> Result<AnkaIndexStatus> {
    let policy = policy::AnkaPolicy::summary(config);
    match read_index(cache_path) {
        Ok(index) => Ok(status_from_index(cache_path.to_path_buf(), &index, policy)),
        Err(error) if error.downcast_ref::<AnkaCacheFormatError>().is_some() => {
            Ok(AnkaIndexStatus {
                state: AnkaCacheState::Incompatible,
                coverage: Vec::new(),
                cache_path: cache_path.to_path_buf(),
                indexed_sources: 0,
                indexed_records: 0,
                last_indexed_at: None,
                policy,
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
        policy: raios_contracts::anka::AnkaPolicySummaryDto {
            initialized: status.policy.initialized,
            home: status.policy.home,
            exclude_rules: status.policy.exclude_rules,
        },
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

/// Create the privacy policy — explicit consent only. `home` has no default
/// and no inference, an existing policy is never overwritten, and creating one
/// has no side effect beyond the file itself: no indexing, no cache, no timer,
/// and `anka-exclude`/`anka-tombstones` are never rewritten.
pub fn policy_init(home: HomeChoice) -> Result<PolicyInitView> {
    policy::AnkaPolicy::init(&config_dir(), home, &AnkaRoots::live())
}

/// Read-only policy view: resolved rules, effective HOME behaviour, the
/// tombstone count, the kept/excluded breakdown of the current cache — and the
/// pre-publication count of what freshly discovered sources would publish under
/// this policy, so a rule change's loss is countable before any rebuild makes
/// it real. Never writes, never migrates; discovery only reads the sources.
pub fn policy_show() -> Result<PolicyShowView> {
    policy::AnkaPolicy::show_view(&config_dir(), &default_cache_path(), &AnkaRoots::live())
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
    // Consent gate first, before any cache read: a machine without a policy
    // gets the same refusal from recall as from rebuild — never an empty
    // allow, never an unrestricted fallback.
    let policy = policy::AnkaPolicy::load(config)?;
    let terms = terms(query_text);
    let index = read_index(cache_path)?;
    let tombstones = read_tombstones(config)?;
    let mut hits = index
        .records
        .iter()
        .filter(|record| !tombstoned(record, &tombstones))
        // Policy on the read path, not only at index time: a rule change moves
        // recall immediately — "excluded" must not survive until the next
        // rebuild, in either direction.
        .filter(|record| policy.admit(record).is_none())
        .filter(|record| {
            query
                .project
                .as_deref()
                .map(|project| {
                    // A record speaking for HOME itself — consent-bound or
                    // import-time unscoped — carries a home label, not a
                    // project: it must never satisfy a project filter.
                    !policy.suppressed_from_project_filter(record)
                        && contains(&record.source.project, project)
                })
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
    // Same consent gate as recall and rebuild, and equally fail-closed: `forget`
    // rewrites the index, so it never runs against an absent or malformed
    // policy. The rewrite itself only removes the tombstoned record — it does
    // not re-admit others; recall filters those and the next rebuild applies
    // the policy properly.
    let _policy = policy::AnkaPolicy::load(config)?;
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
    recount_coverage(&mut index);
    write_index(cache_path, &index)?;
    Ok(true)
}

/// Recount `coverage[].records` from the records the index actually holds.
///
/// Every operation that changes the record set (`index_in`'s publish and
/// `forget_in`'s retain) routes through this before writing: the coverage
/// numbers only mean something when they count exactly the records the status
/// output will sum over — a forget that left a tombstoned record in the
/// coverage would keep reporting it as present until the next full refresh.
fn recount_coverage(index: &mut AnkaIndex) {
    for entry in &mut index.coverage {
        entry.records = index
            .records
            .iter()
            .filter(|record| record.source.harness == entry.harness)
            .count();
    }
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

/// Largest byte index `<= index` that starts a UTF-8 character.
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Smallest byte index `>= index` that starts a UTF-8 character (or the end).
fn ceil_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
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
    // Byte offsets routinely land mid-character on non-ASCII content; clamp to
    // the enclosing boundaries instead of falling back to the whole record —
    // a boundary miss must never turn a ~1KB snippet into an unbounded one.
    let start = floor_char_boundary(&record.content, offset.saturating_sub(180));
    let end = ceil_char_boundary(&record.content, offset + 820);
    let mut snippet = record.content[start..end].to_string();
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

fn status_from_index(
    cache_path: PathBuf,
    index: &AnkaIndex,
    policy: AnkaPolicySummary,
) -> AnkaIndexStatus {
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
        policy,
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
mod tests;
