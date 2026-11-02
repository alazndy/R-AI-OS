//! ANKA tombstones: the durable, versioned record of what `forget` has hidden.

use super::*;

/// Add a `forget_key` to the tombstone set and publish the versioned file.
///
/// Callers hold [`AnkaLock`]. The publish error propagates — `PublishedUnsynced`
/// included: the entry is visible, but `forget_in` stops before it rewrites the
/// index, because a forget that survives only until the next crash was not what the
/// caller asked for. A retry re-reads the same set and re-publishes it, so the flow
/// stays idempotent while the fsync chain gets another chance to land.
pub(super) fn add_tombstone(config: &Path, forget_key: &str) -> Result<()> {
    if !is_hash(forget_key) {
        bail!("ANKA refusing to tombstone a value that is not a hash");
    }
    let mut entries = read_tombstones(config)?;
    entries.insert(TombstoneEntry::forget_key(forget_key));
    write_tombstones(config, &entries)
}

/// Schema version written into — and required by — the versioned tombstone file.
pub(super) const TOMBSTONE_SCHEMA_VERSION: u32 = 2;

/// Which identity a tombstone entry carries.
///
/// Declared in the file itself so a reader never guesses: a `forget_key` entry may
/// only match a record's `forget_key`, a `record_id` entry only its id. Legacy
/// entries therefore keep hiding exactly the cache they were written against
/// without ever matching a rebuilt record, whose id is computed differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TombstoneKind {
    ForgetKey,
    RecordId,
}

/// One entry of the versioned tombstone file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) struct TombstoneEntry {
    #[serde(rename = "type")]
    pub(super) kind: TombstoneKind,
    /// A 64-hex-digit hash: a `forget_key` or a legacy `record_id`.
    pub(super) value: String,
}

impl TombstoneEntry {
    pub(super) fn forget_key(value: &str) -> Self {
        Self {
            kind: TombstoneKind::ForgetKey,
            value: value.to_string(),
        }
    }

    pub(super) fn record_id(value: &str) -> Self {
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
pub(super) enum TombstoneFormat {
    Legacy,
    Versioned,
}

/// A hash as this module writes them: exactly 64 hex digits. Used to validate every
/// entry on read, so a corrupt value fails the read instead of silently matching
/// nothing.
pub(super) fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Serialized form of the versioned tombstone file.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TombstoneFile {
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
pub(super) fn read_tombstones_raw(
    config: &Path,
) -> Result<(TombstoneFormat, BTreeSet<TombstoneEntry>)> {
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
pub(super) fn read_tombstones(config: &Path) -> Result<BTreeSet<TombstoneEntry>> {
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
pub(super) fn write_tombstones(config: &Path, entries: &BTreeSet<TombstoneEntry>) -> Result<()> {
    write_tombstones_with(config, entries, atomic_write)
}

/// [`write_tombstones`] with the publish supplied by the caller: the seam that makes
/// an unconfirmed publish observable in a test without needing a real directory fsync
/// to fail (everything up to and including the rename stays the real code).
pub(super) fn write_tombstones_with<F>(
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
pub(super) fn tombstoned(record: &AnkaRecord, tombstones: &BTreeSet<TombstoneEntry>) -> bool {
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
pub(super) fn migrate_tombstones(
    roots: &AnkaRoots,
    cache_path: &Path,
    config: &Path,
) -> Result<()> {
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
pub(super) fn derive_forget_key(roots: &AnkaRoots, record: &AnkaRecord) -> Result<Option<String>> {
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
