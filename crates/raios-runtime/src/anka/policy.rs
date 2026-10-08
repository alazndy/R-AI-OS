//! ANKA privacy policy: explicit consent, typed HOME rule, fail-closed load.
//!
//! Two files make up the policy surface and they are deliberately separate:
//!
//! - `anka-policy` — the consent record written by `raios anka policy-init`.
//!   It carries a versioned header, the mandatory HOME retention choice, and
//!   the home path the choice was made for. Recording the path means the exact
//!   HOME comparison and the encoded Claude fallback slug derive from the
//!   consent itself, not from whatever environment reads the policy later.
//! - `anka-exclude` — case-insensitive literal substring rules. Preserved
//!   verbatim across `init`, evaluated alongside the typed rules. Plain
//!   substrings: never globs, never regular expressions.
//!
//! [`AnkaPolicy::load`] fails closed: a missing, malformed, or unreadable
//! policy is an error, never an empty allow. Rebuild (`index`), recall
//! (`search`/`blame` and through them the MCP recall tool), and `forget` all
//! load it before doing anything. An explicitly initialized policy with no
//! exclusion rules is a distinct, valid choice — it admits everything the
//! typed rules admit.
//!
//! Rule order only decides the *reported* exclusion reason; any match
//! excludes: substring → HOME (including slug/path conflicts) → unknown
//! provenance.
//!
//! Policy is evaluated on recall as well as at index time, so a rule change
//! takes effect on the next query — before any rebuild.

use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use raios_core::anka::{AnkaHarness, AnkaPolicySummary};
use serde::Serialize;

use super::provenance::{normalize_path, AnkaRoots, ProjectScope};
use super::publish::{create_owner_only, ensure_private_dir, sync_directory};
use super::{
    contains, read_index, read_lines, read_tombstones, tombstoned, AnkaIndex, AnkaRecord,
    TombstoneEntry, EXCLUDE_FILE,
};

/// The consent file. Its *absence* is what makes everything else fail closed.
pub const POLICY_FILE: &str = "anka-policy";
/// First line of every policy this build writes; anything else is malformed.
const POLICY_HEADER: &str = "# anka-policy v1";

/// The mandatory HOME retention choice carried by `policy-init --home`.
///
/// There is no default and no inference: creating a policy without deciding
/// this explicitly is a usage error, because "silently keep" and "silently
/// drop" are both privacy positions the operator has to take on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeChoice {
    /// Retain records whose provenance resolves to HOME. They stay searchable
    /// unfiltered but never satisfy a project filter — HOME is not a project.
    Keep,
    /// Drop records whose provenance resolves to HOME: the exact home path or
    /// the exact encoded home slug. Child projects are never touched.
    Exclude,
}

impl HomeChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Exclude => "exclude",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim() {
            "keep" => Ok(Self::Keep),
            "exclude" => Ok(Self::Exclude),
            other => bail!("ANKA home choice must be `keep` or `exclude`, got `{other}`"),
        }
    }
}

/// Why a record is not eligible. Any match excludes; the first match wins the
/// label (substring → HOME → unknown), so the breakdown in `policy-show`
/// counts every record under exactly one reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    /// A literal substring rule in `anka-exclude` matched one of the record's
    /// project claims: the resolved path, the raw harness slug, or the display
    /// label — a harness may store any of the three as its project.
    Substring,
    /// The typed HOME rule matched — or the provenance conflicted with itself
    /// (slug says HOME, resolved path says otherwise) and was rejected.
    Home,
    /// No provable project at all: excluded by default, every harness,
    /// independent of the HOME choice.
    UnknownProvenance,
}

impl Exclusion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Substring => "substring",
            Self::Home => "home",
            Self::UnknownProvenance => "unknown_provenance",
        }
    }
}

/// An initialized policy. Only [`AnkaPolicy::init`] creates one; every other
/// path arrives here through [`AnkaPolicy::load`], which fails closed.
#[derive(Debug, Clone)]
pub(super) struct AnkaPolicy {
    home: HomeChoice,
    /// The normalized home path recorded at `init` — the consent's subject.
    home_path: String,
    /// [`encoded_home_slug`] of `home_path`, precomputed for exact matching.
    home_slug: String,
    /// Literal substring rules from `anka-exclude` (comments and blanks
    /// already dropped, matching is case-insensitive).
    exclude_patterns: HashSet<String>,
}

impl AnkaPolicy {
    /// Load the policy, failing closed on anything that is not a readable,
    /// parseable, initialized consent file.
    pub(super) fn load(config: &Path) -> Result<Self> {
        Self::try_load(config)?.ok_or_else(|| anyhow!(not_initialized_message()))
    }

    /// [`AnkaPolicy::load`] with "not initialized" distinguished from every
    /// other failure: `Ok(None)` means the file does not exist, `Err` means it
    /// exists but cannot be honored (malformed, unreadable) — both block
    /// recall, but only the latter is worth an error on a read-only status
    /// surface that has to keep describing the cache anyway.
    pub(super) fn try_load(config: &Path) -> Result<Option<Self>> {
        let path = config.join(POLICY_FILE);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()))
            }
        };
        let (home, home_path) = parse_policy(&content, &path)?;
        let exclude_patterns = read_lines(&config.join(EXCLUDE_FILE))?;
        let home_slug = encoded_home_slug(&home_path);
        Ok(Some(Self {
            home,
            home_path,
            home_slug,
            exclude_patterns,
        }))
    }

    /// Create the consent file. Refuses to overwrite an existing one, writes
    /// owner-only, and has no other side effect: no indexing, no cache, no
    /// timer, and `anka-exclude`/`anka-tombstones` are never rewritten.
    pub(super) fn init(
        config: &Path,
        home: HomeChoice,
        roots: &AnkaRoots,
    ) -> Result<PolicyInitView> {
        let home_path = normalize_path(&roots.home.to_string_lossy()).ok_or_else(|| {
            anyhow!(
                "ANKA cannot normalize the home path {}",
                roots.home.display()
            )
        })?;
        ensure_private_dir(config)?;
        let path = config.join(POLICY_FILE);
        let body = format!(
            "{POLICY_HEADER}\nhome = {}\nhome_path = {home_path}\n",
            home.as_str()
        );
        // `create_new` is the refusal: the file's existence *is* the consent,
        // so the no-overwrite rule holds atomically — no check-then-write
        // window in which a second init could slip past.
        let mut file = match create_owner_only(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "ANKA policy already exists at {} — refusing to overwrite it; remove \
                     the file explicitly to re-initialize",
                    path.display()
                );
            }
            Err(error) => {
                return Err(error).with_context(|| format!("could not create {}", path.display()))
            }
        };
        if let Err(error) = file
            .write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
        {
            // We created this file in this call, so removing our own partial
            // write is safe and keeps the next `policy-init` retryable instead
            // of tripped up by a header a crash cut in half.
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error).with_context(|| format!("could not write {}", path.display()));
        }
        drop(file);
        sync_directory(config)?;
        Ok(PolicyInitView {
            created: true,
            policy_path: path.display().to_string(),
            home: home.as_str().to_string(),
            home_path,
        })
    }

    /// The read-only view behind `policy-show`: resolved rules, effective HOME
    /// behaviour, the tombstone count, the kept/excluded breakdown of the
    /// *current* cache — and the pre-publication count: the same policy
    /// evaluated against freshly discovered sources, so a pending change's loss
    /// is countable before any rebuild publishes it. Never writes, never
    /// migrates; discovery here only reads the sources.
    pub(super) fn show_view(
        config: &Path,
        cache_path: &Path,
        roots: &AnkaRoots,
    ) -> Result<PolicyShowView> {
        let mut exclude_rules: Vec<String> = read_lines(&config.join(EXCLUDE_FILE))?
            .into_iter()
            .collect();
        exclude_rules.sort();
        let tombstones = read_tombstones(config)?;
        let policy_path = config.join(POLICY_FILE).display().to_string();
        let Some(policy) = Self::try_load(config)? else {
            // Not initialized is a *state*, not a failure: `show` is the
            // diagnostic surface, and reporting the gap is more useful than
            // echoing the same error recall already gives.
            return Ok(PolicyShowView {
                initialized: false,
                policy_path,
                home: None,
                home_path: None,
                exclude_rules,
                tombstones: tombstones.len(),
                cache: None,
                discovery: None,
            });
        };
        let cache = cache_breakdown(&policy, cache_path, &tombstones);
        let discovery = discovery_breakdown(&policy, roots, &tombstones);
        Ok(PolicyShowView {
            initialized: true,
            policy_path,
            home: Some(policy.home.as_str().to_string()),
            home_path: Some(policy.home_path.clone()),
            exclude_rules,
            tombstones: tombstones.len(),
            cache: Some(cache),
            discovery: Some(discovery),
        })
    }

    /// One predicate over a record: admitted, or the reason it is not. Shared
    /// by `index`, `search`, and `blame` so a rule change moves recall without
    /// waiting for a rebuild and a rebuild cannot publish what recall hides.
    pub(super) fn admit(&self, record: &AnkaRecord) -> Option<Exclusion> {
        // User-authored rules first: an explicit substring is the most specific
        // intent on the page. They match every project claim the record carries —
        // the resolved provenance path, the raw harness slug, and the display
        // label — because a harness may store any of the three: Claude records
        // the directory slug in `source.project` while the trustworthy path
        // exists only in the provenance.
        if self.exclude_patterns.iter().any(|pattern| {
            contains(&record.source.project, pattern)
                || record
                    .provenance
                    .path
                    .as_deref()
                    .is_some_and(|path| contains(path, pattern))
                || record
                    .provenance
                    .slug
                    .as_deref()
                    .is_some_and(|slug| contains(slug, pattern))
                // A cache written before `Provenance::path` existed still carries
                // the resolved path inside the scope itself.
                || matches!(
                    &record.provenance.scope,
                    ProjectScope::Scoped(path) if contains(path, pattern)
                )
        }) {
            return Some(Exclusion::Substring);
        }
        // Conflicting provenance: the slug still says HOME while a resolved
        // path says otherwise (a session that started under HOME and moved, or
        // a harness whose directory name contradicts its reported cwd).
        // Ambiguous evidence is rejected regardless of the HOME choice — it is
        // never guessed at in either direction.

        if record.provenance.slug.as_deref() == Some(self.home_slug.as_str())
            && !matches!(
                record.provenance.scope,
                ProjectScope::HomeUnscoped | ProjectScope::Slug(_)
            )
        {
            return Some(Exclusion::Home);
        }
        if self.is_home_label(record) {
            match self.home {
                HomeChoice::Exclude => return Some(Exclusion::Home),
                // Retained by explicit consent; the project-filter
                // suppression lives in `search_in` (see
                // `suppressed_from_project_filter`).
                HomeChoice::Keep => {}
            }
        }
        if record.provenance.scope == ProjectScope::Unknown {
            return Some(Exclusion::UnknownProvenance);
        }
        None
    }

    /// True when a record speaks for the *consented* HOME itself rather than a
    /// project: its resolved path equals the `home_path` recorded at `init`, or
    /// its harness slug is that path's exact encoded form. Both forms are
    /// matched; children and other slugs never are (exact equality, never
    /// substring — that is the whole point of the typed rule).
    ///
    /// The consent's recorded path decides — not the import-time
    /// [`ProjectScope::HomeUnscoped`] label, which was computed against whatever
    /// `$HOME` the indexing process saw. When the process home moves, the home
    /// named in the consent must keep being the one this rule speaks for.
    pub(super) fn is_home_label(&self, record: &AnkaRecord) -> bool {
        let provenance = &record.provenance;
        let resolved_path = match provenance.path.as_deref() {
            Some(path) => Some(path),
            // Records written before `Provenance::path` existed: the import-time
            // scope is all the evidence there is. `HomeUnscoped` carries no path
            // at all, so its label is honored conservatively (a home claim we
            // cannot re-check is still a home claim); a legacy `Scoped` path is
            // compared to the consent exactly like a fresh one.
            None => match &provenance.scope {
                ProjectScope::HomeUnscoped => return true,
                ProjectScope::Scoped(path) => Some(path.as_str()),
                ProjectScope::Slug(_) | ProjectScope::Unknown => None,
            },
        };
        resolved_path == Some(self.home_path.as_str())
            || provenance.slug.as_deref() == Some(self.home_slug.as_str())
    }

    /// True when a record must never satisfy a project filter: it speaks for a
    /// home directory itself — consent-bound or import-time unscoped — because
    /// an unscoped home label is not a project, whichever home the consent
    /// names. Independent of the HOME choice: this governs filters, not
    /// admission.
    pub(super) fn suppressed_from_project_filter(&self, record: &AnkaRecord) -> bool {
        self.is_home_label(record) || record.provenance.scope == ProjectScope::HomeUnscoped
    }

    /// The summary `status` carries so a fail-closed recall surface is
    /// explainable without a second call. Never fails: an absent policy
    /// reports `initialized: false`, a malformed one likewise (the detail
    /// belongs to `policy-show`, which is the diagnostic surface).
    pub(super) fn summary(config: &Path) -> AnkaPolicySummary {
        match Self::try_load(config) {
            Ok(Some(policy)) => AnkaPolicySummary {
                initialized: true,
                home: Some(policy.home.as_str().to_string()),
                exclude_rules: policy.exclude_patterns.len(),
            },
            Ok(None) | Err(_) => AnkaPolicySummary {
                initialized: false,
                home: None,
                exclude_rules: 0,
            },
        }
    }

    /// This loaded policy's own summary — used by `index_in`, which already
    /// holds the loaded policy and must not load it twice.
    pub(super) fn own_summary(&self) -> AnkaPolicySummary {
        AnkaPolicySummary {
            initialized: true,
            home: Some(self.home.as_str().to_string()),
            exclude_rules: self.exclude_patterns.len(),
        }
    }
}

/// `policy-init` result. Nothing else happened, and the payload says so.
#[derive(Debug, Clone, Serialize)]
pub struct PolicyInitView {
    pub created: bool,
    pub policy_path: String,
    pub home: String,
    pub home_path: String,
}

/// `policy-show` result.
#[derive(Debug, Clone, Serialize)]
pub struct PolicyShowView {
    /// False when `anka-policy` does not exist — a state to fix, not an error.
    pub initialized: bool,
    pub policy_path: String,
    /// `keep` | `exclude`; `None` until the policy is initialized.
    pub home: Option<String>,
    /// The home path the consent was recorded for; `None` until initialized.
    pub home_path: Option<String>,
    /// Resolved `anka-exclude` rules, sorted for stable output. The operator's
    /// own file, so displaying the patterns themselves is in bounds; record
    /// contents and personal project names never are.
    pub exclude_rules: Vec<String>,
    /// Entries in `anka-tombstones` (forget-key and retained record-id entries).
    pub tombstones: usize,
    /// Kept/excluded breakdown of the current cache — `None` while the policy
    /// is not initialized, because there is no admission decision to report.
    pub cache: Option<CacheBreakdown>,
    /// Pre-publication count: discovery output evaluated against the same
    /// tombstones and policy the next rebuild applies — `None` while the policy
    /// is not initialized.
    pub discovery: Option<DiscoveryBreakdown>,
}

/// What the current cache holds under the current policy. Computed read-only:
/// records a *pending* rule will drop show up here as excluded before any
/// rebuild publishes that loss.
#[derive(Debug, Clone, Serialize)]
pub struct CacheBreakdown {
    /// "absent" (no cache yet) | "ok" | "unreadable" (corrupt, unknown schema,
    /// or could not be inspected).
    pub state: String,
    /// Present only when `state` is "unreadable": the reader's own message.
    pub detail: Option<String>,
    pub kept: usize,
    pub excluded: ReasonCounts,
    /// Per-harness kept/excluded, all four harnesses in stable order.
    pub per_harness: Vec<HarnessCounts>,
}

/// What the *next* rebuild would publish: discovery output — every source read
/// fresh, nothing written — evaluated against the same tombstones and policy
/// that rebuild applies. [`CacheBreakdown`] reports what is published today;
/// this reports what publication would become, so a rule change's loss is
/// countable without running the rebuild.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryBreakdown {
    /// "ok" | "error" (the sources could not be read).
    pub state: String,
    /// Present only when `state` is "error": the reader's own message.
    pub detail: Option<String>,
    /// Records discovery produced, before tombstone or policy filtering.
    pub discovered: usize,
    pub kept: usize,
    pub excluded: ReasonCounts,
    /// Per-harness kept/excluded, all four harnesses in stable order.
    pub per_harness: Vec<HarnessCounts>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReasonCounts {
    pub substring: usize,
    pub home: usize,
    pub unknown_provenance: usize,
    pub tombstone: usize,
}

impl ReasonCounts {
    fn bump(&mut self, reason: Exclusion) {
        match reason {
            Exclusion::Substring => self.substring += 1,
            Exclusion::Home => self.home += 1,
            Exclusion::UnknownProvenance => self.unknown_provenance += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.substring + self.home + self.unknown_provenance + self.tombstone
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessCounts {
    pub harness: String,
    pub kept: usize,
    pub excluded: usize,
}

/// Evaluate the current cache against `policy` without writing anything.
fn cache_breakdown(
    policy: &AnkaPolicy,
    cache_path: &Path,
    tombstones: &std::collections::BTreeSet<TombstoneEntry>,
) -> CacheBreakdown {
    let index_path = cache_path.join(super::INDEX_FILE);
    // An inspection failure (permissions, I/O) is *not* an absent cache:
    // reporting `absent` with zero records would claim a total loss when the
    // file simply could not be seen. Only a definitive "does not exist" is
    // `absent`.
    match index_path.try_exists() {
        Ok(false) => {
            return CacheBreakdown {
                state: "absent".to_string(),
                detail: None,
                kept: 0,
                excluded: ReasonCounts::default(),
                per_harness: Vec::new(),
            };
        }
        Err(error) => {
            return CacheBreakdown {
                state: "unreadable".to_string(),
                detail: Some(format!(
                    "could not inspect {}: {error}",
                    index_path.display()
                )),
                kept: 0,
                excluded: ReasonCounts::default(),
                per_harness: Vec::new(),
            };
        }
        Ok(true) => {}
    }
    let index: AnkaIndex = match read_index(cache_path) {
        Ok(index) => index,
        Err(error) => {
            return CacheBreakdown {
                state: "unreadable".to_string(),
                detail: Some(error.to_string()),
                kept: 0,
                excluded: ReasonCounts::default(),
                per_harness: Vec::new(),
            }
        }
    };
    let (kept, excluded, per_harness) = count_records(policy, &index.records, tombstones);
    CacheBreakdown {
        state: "ok".to_string(),
        detail: None,
        kept,
        excluded,
        per_harness,
    }
}

/// Evaluate freshly discovered sources against `policy` without writing
/// anything: the pre-publication half of `policy-show`. Runs the same discovery
/// `index_in` would run, then applies the same tombstones and admission
/// predicate — so the count is what a rebuild *would* publish and drop.
fn discovery_breakdown(
    policy: &AnkaPolicy,
    roots: &AnkaRoots,
    tombstones: &std::collections::BTreeSet<TombstoneEntry>,
) -> DiscoveryBreakdown {
    let mut records = Vec::new();
    for harness in AnkaHarness::ALL.iter() {
        match super::discover_harness(roots, harness) {
            Ok((found, _report)) => records.extend(found),
            Err(error) => {
                return DiscoveryBreakdown {
                    state: "error".to_string(),
                    detail: Some(error.to_string()),
                    discovered: 0,
                    kept: 0,
                    excluded: ReasonCounts::default(),
                    per_harness: Vec::new(),
                };
            }
        }
    }
    let discovered = records.len();
    let (kept, excluded, per_harness) = count_records(policy, &records, tombstones);
    DiscoveryBreakdown {
        state: "ok".to_string(),
        detail: None,
        discovered,
        kept,
        excluded,
        per_harness,
    }
}

/// Count records under `policy`, tombstones first. Shared by the cache view and
/// the pre-publication discovery view so both report the identical admission
/// decision for every record — a difference between the two is then a real
/// difference in inputs, never in rules.
fn count_records(
    policy: &AnkaPolicy,
    records: &[AnkaRecord],
    tombstones: &std::collections::BTreeSet<TombstoneEntry>,
) -> (usize, ReasonCounts, Vec<HarnessCounts>) {
    let mut kept = 0usize;
    let mut excluded = ReasonCounts::default();
    let mut per_harness: Vec<HarnessCounts> = AnkaHarness::ALL
        .iter()
        .map(|harness| HarnessCounts {
            harness: harness.as_str().to_string(),
            kept: 0,
            excluded: 0,
        })
        .collect();
    for record in records {
        let slot = per_harness
            .iter_mut()
            .find(|entry| entry.harness == record.source.harness.as_str())
            .expect("every harness has a slot");
        let reason = if tombstoned(record, tombstones) {
            excluded.tombstone += 1;
            slot.excluded += 1;
            continue;
        } else {
            policy.admit(record)
        };
        match reason {
            None => {
                kept += 1;
                slot.kept += 1;
            }
            Some(reason) => {
                excluded.bump(reason);
                slot.excluded += 1;
            }
        }
    }
    (kept, excluded, per_harness)
}

/// Parse an initialized policy file: exact versioned header, then
/// `key = value` lines. Unknown keys, duplicate keys, a missing key, or a
/// header this build does not recognize are all malformed — fail closed with
/// the file's own message, never a partial interpretation.
fn parse_policy(content: &str, path: &Path) -> Result<(HomeChoice, String)> {
    let mut lines = content.lines().enumerate();
    let first = lines.next().map(|(_, line)| line.trim()).unwrap_or("");
    if first != POLICY_HEADER {
        bail!(
            "ANKA policy {} is malformed: it must start with the `{POLICY_HEADER}` header",
            path.display()
        );
    }
    let mut home: Option<HomeChoice> = None;
    let mut home_path: Option<String> = None;
    for (index, raw) in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!(
                "ANKA policy {} line {} is malformed: expected `key = value`",
                path.display(),
                index + 1
            );
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "home" => {
                if home.is_some() {
                    bail!(
                        "ANKA policy {} is malformed: duplicate `home` key",
                        path.display()
                    );
                }
                home = Some(HomeChoice::parse(value).map_err(|error| {
                    anyhow!("ANKA policy {} is malformed: {error}", path.display())
                })?);
            }
            "home_path" => {
                if home_path.is_some() {
                    bail!(
                        "ANKA policy {} is malformed: duplicate `home_path` key",
                        path.display()
                    );
                }
                if value.is_empty() {
                    bail!(
                        "ANKA policy {} is malformed: `home_path` is empty",
                        path.display()
                    );
                }
                // The consent's subject feeds exact path comparisons, so it must
                // arrive in the canonical form `init` writes: absolute and
                // already normalized. A relative path, a trailing `/`, or an
                // interior `//` would let formatting — not the operator's
                // decision — decide whether a record is HOME.
                match normalize_path(value) {
                    Some(normalized) if normalized == value => {
                        home_path = Some(value.to_string());
                    }
                    _ => bail!(
                        "ANKA policy {} line {} is malformed: `home_path` must be an \
                         absolute, already-normalized path (got `{value}`)",
                        path.display(),
                        index + 1
                    ),
                }
            }
            other => {
                bail!(
                    "ANKA policy {} is malformed: unknown key `{other}`",
                    path.display()
                )
            }
        }
    }
    match (home, home_path) {
        (Some(home), Some(home_path)) => Ok((home, home_path)),
        _ => bail!(
            "ANKA policy {} is malformed: `home` and `home_path` are both required",
            path.display()
        ),
    }
}

/// The exact directory slug a harness derives for a path: every character
/// outside `[A-Za-z0-9]` becomes one `-`, so `/home/alaz` encodes to
/// `-home-alaz` — the shape Claude Code stores as a project directory name.
/// Matching stays exact after encoding; this never broadens to a prefix.
pub(super) fn encoded_home_slug(home_path: &str) -> String {
    home_path
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn not_initialized_message() -> String {
    "ANKA policy is not initialized — run `raios anka policy-init --home keep` or \
     `--home exclude` first; recall, indexing, and forget stay blocked until then"
        .to_string()
}
