use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
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
#[cfg(unix)]
struct Fixture {
    roots: AnkaRoots,
    cache: PathBuf,
    config: PathBuf,
    _temp: tempfile::TempDir,
}

#[cfg(unix)]
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
    // Real provenance on the fixture line: the default policy admits known
    // provenance only, so a metadata-less line would be dropped from the
    // index by default and every "the record exists" assertion would then
    // pass vacuously.
    fs::write(
        &history,
        "{\"project\":\"/srv/anka-fixture\",\"text\":\"remember this prompt\"}\n",
    )
    .expect("history");
    let roots = AnkaRoots {
        home: home.clone(),
        config: config.clone(),
        cache: cache.clone(),
    };
    // Explicit consent for these temp roots, written exactly the way
    // `policy-init` writes it: nothing may recall, rebuild, or forget
    // without one.
    policy::AnkaPolicy::init(&config, HomeChoice::Keep, &roots).expect("fixture policy");
    Fixture {
        roots,
        cache,
        config,
        _temp: temp,
    }
}

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
fn write_legacy_cache_with_source(fixture: &Fixture, legacy_id: &str, source: serde_json::Value) {
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

#[cfg(unix)]
fn write_v1_tombstones(fixture: &Fixture, ids: &[&str]) {
    let mut body = String::new();
    for id in ids {
        body.push_str(id);
        body.push('\n');
    }
    fs::write(fixture.config.join(TOMBSTONE_FILE), body).expect("v1 tombstones");
}

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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

/// The coverage numbers must count the records the index actually holds:
/// a forget that shrinks the record set shrinks its harness coverage too,
/// instead of leaving the tombstoned record numerically present until the
/// next full refresh.
#[cfg(unix)]
#[test]
fn forget_recounts_the_harness_coverage_of_the_record_set_it_shrinks() {
    let fixture = fixture();
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    let forgotten = only_record_id(&fixture);

    forget_in(&fixture.roots, &fixture.cache, &fixture.config, &forgotten).expect("forget");

    let index = read_index(&fixture.cache).expect("read index");
    assert!(
        index.records.is_empty(),
        "the fixture has a single record and it was forgotten"
    );
    for entry in &index.coverage {
        let counted = index
            .records
            .iter()
            .filter(|record| record.source.harness == entry.harness)
            .count();
        assert_eq!(
            entry.records, counted,
            "coverage for {:?} must count the records the index holds",
            entry.harness
        );
    }
}

/// A byte offset landing mid-character (routine for non-ASCII content) must
/// clamp to the enclosing boundary — never fall back to handing out the
/// whole record as the snippet, which would defeat the ~1KB output bound on
/// the CLI and MCP surfaces.
#[test]
fn a_snippet_boundary_miss_clamps_instead_of_returning_the_whole_record() {
    // Bytes 178..184 are two 3-byte `€`; the raw start (359 - 180 = 179)
    // lands inside the first one, so the old `.get(start..end)` returned None.
    let content = format!(
        "{}€€{}target{}",
        "a".repeat(178),
        "a".repeat(175),
        "a".repeat(2000)
    );
    let hit = hit_for(&record(spec(&content)), "target", &["target".to_string()])
        .expect("the content contains the term");

    assert!(
        hit.snippet.len() < content.len(),
        "a boundary miss must never produce the whole record"
    );
    assert!(
        hit.snippet.len() <= 1010,
        "the snippet stays within its ~1KB window, got {} bytes",
        hit.snippet.len()
    );
    assert!(
        hit.snippet.starts_with('…'),
        "the offset window keeps its prefix marker"
    );
    assert!(
        hit.snippet.contains("target"),
        "the matched term stays visible"
    );
}

/// A forgotten record stays hidden even after a rebuild that would have republished
/// it — the tombstone is checked on recall, not only during the rebuild.
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
#[test]
fn add_tombstone_propagates_read_errors_other_than_not_found() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config");
    ensure_private_dir(&config).expect("config dir");
    let path = config.join(TOMBSTONE_FILE);
    let seed = br#"{"schema_version":2,"entries":[]}"#;
    fs::write(&path, seed).expect("seed");

    // Make the file unreadable by removing read permission.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("remove read permission");

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

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
///
/// This is also the acceptance fixture for an mtime-fallback line: the default
/// history entry carries no timestamp *and* no native `session_id`, so the
/// derived key is the fixed file-stem namespace plus the prompt group —
/// identical for every line in the file — and nothing event-specific is
/// claimed. The record migrates instead of being blocked on an event the old
/// importer never recorded.
#[cfg(unix)]
#[test]
fn a_legacy_tombstone_migrates_through_the_old_cache_record() {
    let fixture = fixture();
    let legacy_id = legacy_history_id();
    write_legacy_cache(&fixture, &legacy_id);
    write_v1_tombstones(&fixture, &[&legacy_id]);

    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("migration and rebuild");

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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
/// a key that misses the rebuilt record — and the prompt would come back. The
/// recovery runs through a *verified* event: the line carries the exact
/// timestamp the cache recorded, so the session — and with it the forget key —
/// is proven to belong to this record's event.
#[cfg(unix)]
#[test]
fn a_legacy_history_tombstone_recovers_the_native_session_id() {
    let fixture = fixture();
    // Known provenance on the line so the record survives the default
    // policy — the assertion below must prove the *tombstone* filtered it,
    // not the provenance rule.
    fs::write(
        fixture.roots.opencode_history(),
        "{\"project\":\"/srv/anka-fixture\",\"session_id\":\"SID-9\",\"timestamp\":1700000000,\"text\":\"remember this prompt\"}\n",
    )
    .expect("history");
    let legacy_id = legacy_history_id();
    write_legacy_cache(&fixture, &legacy_id);
    write_v1_tombstones(&fixture, &[&legacy_id]);

    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("migration and rebuild");

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

/// A line without a timestamp but *with* a native `session_id` claims an
/// event-specific identity that nothing verifies: after a rotation this line
/// can hold **another session's** identical prompt, so the recovered session
/// — and the forget key derived from it — may name the wrong event and quietly
/// resurrect the forgotten record after the next rebuild. Unlike the
/// session-less case (where the key is the fixed file-stem namespace plus the
/// prompt group), a wrong guess here is detectable only by refusing: the
/// migration stops with nothing published, and mtime is never consulted as
/// substitute event evidence.
#[cfg(unix)]
#[test]
fn a_legacy_migration_stops_when_a_native_session_cannot_be_tied_to_an_event() {
    let fixture = fixture();
    fs::write(
        fixture.roots.opencode_history(),
        "{\"session_id\":\"SID-UNVERIFIED\",\"text\":\"remember this prompt\"}\n",
    )
    .expect("history");
    let legacy_id = legacy_history_id();
    write_legacy_cache(&fixture, &legacy_id);
    write_v1_tombstones(&fixture, &[&legacy_id]);
    let tombstone_path = fixture.config.join(TOMBSTONE_FILE);
    let index_path = fixture.cache.join(INDEX_FILE);
    let tombstone_before = fs::read(&tombstone_path).expect("tombstone bytes");
    let index_before = fs::read(&index_path).expect("index bytes");

    let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
        .expect_err("a native session without an event must stop the migration");
    assert!(error.to_string().contains("migration blocked"), "{error}");
    assert_eq!(
        fs::read(&tombstone_path).expect("tombstone bytes"),
        tombstone_before,
        "nothing was published while the session is unverifiable"
    );
    assert_eq!(
        fs::read(&index_path).expect("index bytes"),
        index_before,
        "the cache is untouched while the migration is blocked"
    );
}

/// A rotation can leave *another session's event* at the line the old cache
/// recorded — same prompt text, different timestamp. Text equality cannot tell
/// those apart, and deriving a key from the wrong event would silently stop
/// hiding the record after the next rebuild, so for Codex the re-read line must
/// carry the exact event time the cache stored; an indistinguishable match
/// stops the migration with nothing published until the source is resolved.
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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

    let status = status_in(&fixture.cache, &fixture.config).expect("status describes the cache");
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
#[cfg(unix)]
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
#[cfg(unix)]
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

    let status = status_in(&fixture.cache, &fixture.config).expect("status describes the cache");
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
#[cfg(unix)]
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
#[cfg(unix)]
#[test]
fn a_harness_scoped_refresh_preserves_the_other_harnesses_and_their_coverage() {
    let fixture = fixture();
    // A second harness, with two prompts of its own. Antigravity rather
    // than Codex: its records carry inline `workspace` provenance, while a
    // Codex history line without session metadata has unknown provenance
    // and the default policy keeps it out of the index entirely — which
    // would make "records carry forward" impossible to observe here.
    let antigravity_history = fixture.roots.antigravity_history();
    fs::create_dir_all(antigravity_history.parent().expect("antigravity dir"))
        .expect("antigravity dir");
    fs::write(
        &antigravity_history,
        "{\"workspace\":\"/srv/anka-fixture\",\"display\":\"first antigravity prompt\"}\n\
         {\"workspace\":\"/srv/anka-fixture\",\"display\":\"second antigravity prompt\"}\n",
    )
    .expect("antigravity history");

    let full = index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("full build");
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
    assert_eq!(coverage_of(&full, AnkaHarness::Antigravity).records, 2);
    assert_eq!(coverage_of(&full, AnkaHarness::Opencode).records, 1);

    // Scoped: the antigravity history turns over — one line leaves, one arrives.
    fs::write(
        &antigravity_history,
        "{\"workspace\":\"/srv/anka-fixture\",\"display\":\"second antigravity prompt\"}\n\
         {\"workspace\":\"/srv/anka-fixture\",\"display\":\"third antigravity prompt\"}\n",
    )
    .expect("antigravity history updated");
    let scoped = index_in(
        &fixture.roots,
        &fixture.cache,
        &fixture.config,
        Some(AnkaHarness::Antigravity),
    )
    .expect("scoped refresh");

    assert_eq!(
        scoped.state,
        AnkaCacheState::Ready,
        "the cache still speaks for every harness"
    );
    assert_eq!(
        coverage_of(&scoped, AnkaHarness::Antigravity).records,
        2,
        "the scoped slice was rebuilt"
    );
    assert!(
        !coverage_of(&scoped, AnkaHarness::Antigravity).full_refresh,
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
            .any(|record| record.content == "third antigravity prompt"),
        "the refreshed slice must carry the new line"
    );
    assert!(
        !published
            .iter()
            .any(|record| record.content == "first antigravity prompt"),
        "the line that left the source must leave the cache"
    );
}

/// `oversized` is its own exclusion reason in the coverage, not folded into
/// malformed: the line was never parsed — its content never materialized —
/// so counting it as malformed would describe an event that did not happen.
#[cfg(unix)]
#[test]
fn oversized_lines_are_reported_as_their_own_coverage_reason() {
    let fixture = fixture();
    let oversized_line = "a".repeat(provenance::HISTORY_LINE_MAX_BYTES + 1);
    fs::write(
        fixture.roots.opencode_history(),
        format!(
            "{{\"project\":\"/srv/anka-fixture\",\"text\":\"remember this prompt\"}}\n{oversized_line}\n"
        ),
    )
    .expect("history with an oversized line");

    let status = index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

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
    let status =
        status_in(&temp.path().join("cache"), &temp.path().join("config")).expect("status");
    assert_eq!(status.state, AnkaCacheState::Empty);
    assert!(status.coverage.is_empty());
    // No policy either — and that is reported, not propagated: `status`
    // describes policy state instead of failing the way recall does.
    assert!(!status.policy.initialized);
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
#[cfg(unix)]
#[test]
fn a_legacy_cache_reports_partial_status_with_reconstructed_coverage() {
    let fixture = fixture();
    write_legacy_cache(&fixture, &legacy_history_id());

    let status = status_in(&fixture.cache, &fixture.config).expect("status");
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

// ─── Phase 3: privacy policy — init/show, recall exclusions, breakdown ───

/// Drop the fixture's consent and write a fresh one with the requested
/// choice — `init` refuses an existing policy, by design.
#[cfg(unix)]
fn reinit_policy(fixture: &Fixture, home: HomeChoice) {
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture policy");
    policy::AnkaPolicy::init(&fixture.config, home, &fixture.roots).expect("policy-init");
}

#[cfg(unix)]
#[test]
fn policy_init_writes_a_versioned_consent_and_refuses_to_overwrite_it() {
    let fixture = fixture();
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture policy");

    let view = policy::AnkaPolicy::init(&fixture.config, HomeChoice::Exclude, &fixture.roots)
        .expect("the first init succeeds");
    assert!(view.created);
    assert_eq!(view.home, "exclude");
    assert_eq!(
        view.home_path,
        fixture.roots.home.display().to_string(),
        "the consent records which home the choice was made for"
    );

    let written =
        fs::read_to_string(fixture.config.join(policy::POLICY_FILE)).expect("policy bytes");
    assert!(
        written.starts_with("# anka-policy v1"),
        "the versioned header identifies what this build wrote: {written:?}"
    );
    assert!(written.contains("home = exclude"), "{written:?}");
    assert!(written.contains("home_path = "), "{written:?}");

    let error = policy::AnkaPolicy::init(&fixture.config, HomeChoice::Keep, &fixture.roots)
        .expect_err("a second init must refuse");
    assert!(error.to_string().contains("already exists"), "{error}");
    let after = fs::read_to_string(fixture.config.join(policy::POLICY_FILE)).expect("bytes");
    assert!(
        after.contains("home = exclude"),
        "the refusal must leave the original consent untouched"
    );
}

#[cfg(unix)]
#[test]
fn policy_init_creates_nothing_but_the_policy_file() {
    let fixture = fixture();
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture policy");
    fs::write(fixture.config.join(EXCLUDE_FILE), "Personal\n").expect("rule file");
    write_v1_tombstones(&fixture, &["legacy-id"]);
    let exclude_before = fs::read(fixture.config.join(EXCLUDE_FILE)).expect("exclude bytes");
    let tombstone_before = fs::read(fixture.config.join(TOMBSTONE_FILE)).expect("tombstone bytes");

    policy::AnkaPolicy::init(&fixture.config, HomeChoice::Keep, &fixture.roots)
        .expect("init is allowed to create its own file");

    assert_eq!(
        fs::read(fixture.config.join(EXCLUDE_FILE)).expect("exclude bytes"),
        exclude_before,
        "init never rewrites the operator's rule file"
    );
    assert_eq!(
        fs::read(fixture.config.join(TOMBSTONE_FILE)).expect("tombstone bytes"),
        tombstone_before,
        "init never rewrites tombstones"
    );
    assert!(
        !fixture.cache.exists(),
        "init never touches the cache — no indexing, no migration, no timer"
    );
    let mut entries: Vec<String> = fs::read_dir(&fixture.config)
        .expect("config dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    let mut expected = vec![
        policy::POLICY_FILE.to_string(),
        TOMBSTONE_FILE.to_string(),
        EXCLUDE_FILE.to_string(),
    ];
    expected.sort();
    assert_eq!(
        entries, expected,
        "exactly the policy, the rules, and the tombstones — nothing else appears"
    );
}

#[cfg(unix)]
#[test]
fn recall_index_and_forget_refuse_to_run_without_an_initialized_policy() {
    let fixture = fixture();
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture policy");

    for error in [
        index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
            .expect_err("index refuses")
            .to_string(),
        search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("search refuses")
        .to_string(),
        blame_in(
            &fixture.cache,
            &fixture.config,
            "crates/raios-core/src/anka.rs",
            5,
        )
        .expect_err("blame refuses")
        .to_string(),
        forget_in(&fixture.roots, &fixture.cache, &fixture.config, "some-id")
            .expect_err("forget refuses")
            .to_string(),
    ] {
        assert!(
            error.contains("not initialized"),
            "every path fails closed on the same reason: {error}"
        );
    }
    assert!(
        !fixture.cache.exists(),
        "a refused index run writes nothing at all"
    );
    assert!(
        !fixture.config.join(TOMBSTONE_FILE).exists(),
        "a refused forget writes no tombstone"
    );

    // `status` keeps describing the state instead of failing like recall.
    let status = status_in(&fixture.cache, &fixture.config).expect("status reports");
    assert!(!status.policy.initialized);
    assert!(status_dto(status).policy.home.is_none());
}

#[cfg(unix)]
#[test]
fn a_malformed_policy_is_refused_rather_than_fallback() {
    let fixture = fixture();
    let path = fixture.config.join(policy::POLICY_FILE);

    fs::write(&path, "home = keep\n").expect("headerless policy");
    let error = search_in(
        &fixture.cache,
        &fixture.config,
        query("remember this prompt"),
    )
    .expect_err("recall refuses a policy it cannot parse");
    assert!(error.to_string().contains("malformed"), "{error}");
    // Status still works — reporting `initialized: false`, not guessing.
    let status = status_in(&fixture.cache, &fixture.config).expect("status reports");
    assert!(!status.policy.initialized);

    fs::write(&path, "# anka-policy v1\nhome = keep\nsurprise = 1\n").expect("unknown key");
    let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
        .expect_err("an unknown key is not silently ignored");
    assert!(error.to_string().contains("unknown key"), "{error}");
}

#[cfg(unix)]
#[test]
fn exclusion_rules_take_effect_at_recall_before_any_rebuild() {
    let fixture = fixture();
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    assert_eq!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt")
        )
        .expect("search")
        .len(),
        1,
        "the record is recallable before the rule exists"
    );

    // The rule lands after the index — no rebuild, no migration, no write.
    fs::write(fixture.config.join(EXCLUDE_FILE), "srv\n").expect("rule");

    assert!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt")
        )
        .expect("search")
        .is_empty(),
        "the rule applies on the very next query"
    );
    assert!(
        blame_in(&fixture.cache, &fixture.config, "whatever/path.rs", 5)
            .expect("blame")
            .is_empty(),
        "blame reads through the same predicate"
    );
    assert_eq!(
        read_index(&fixture.cache).expect("index").records.len(),
        1,
        "the cache itself is untouched — recall moved without a rebuild"
    );

    // The next rebuild applies the rule at index time too.
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("rebuild");
    assert!(
        read_index(&fixture.cache)
            .expect("index")
            .records
            .is_empty(),
        "the rebuilt index applies the rule as well"
    );

    // And `status` carries the policy summary alongside the cache state.
    let status = status_in(&fixture.cache, &fixture.config).expect("status");
    assert!(status.policy.initialized);
    assert_eq!(status.policy.home.as_deref(), Some("keep"));
    assert_eq!(status.policy.exclude_rules, 1);
}

#[cfg(unix)]
#[test]
fn unknown_provenance_is_excluded_by_default_at_index_and_recall() {
    let fixture = fixture();
    fs::write(
        fixture.roots.opencode_history(),
        "{\"text\":\"no provenance prompt\"}\n",
    )
    .expect("history");

    // (a) Index path: the metadata-less line never reaches the cache.
    let status = index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    assert_eq!(
        coverage_of(&status, AnkaHarness::Opencode).records,
        0,
        "unknown provenance is not published"
    );
    assert!(search_in(
        &fixture.cache,
        &fixture.config,
        query("no provenance prompt")
    )
    .expect("search")
    .is_empty());

    // (b) Recall path: a cache that already holds an unknown record — one
    // written before this policy existed, say — is filtered on read,
    // without waiting for a rebuild.
    ensure_private_dir(&fixture.cache).expect("cache dir");
    let mut legacy = spec("legacy unknown prompt");
    legacy.provenance = Provenance::unknown();
    write_index(
        &fixture.cache,
        &AnkaIndex {
            records: vec![record(legacy)],
            indexed_sources: 1,
            coverage: Vec::new(),
            last_indexed_at: Some(now()),
        },
    )
    .expect("hand-built cache");
    assert_eq!(
        read_index(&fixture.cache).expect("index").records.len(),
        1,
        "the record really is in the cache…"
    );
    assert!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("legacy unknown prompt")
        )
        .expect("search")
        .is_empty(),
        "…and recall still refuses it"
    );
}

#[cfg(unix)]
#[test]
fn home_exclude_drops_the_exact_home_record_but_keeps_child_projects() {
    let fixture = fixture();
    reinit_policy(&fixture, HomeChoice::Exclude);
    let home = fixture.roots.home.display().to_string();
    fs::write(
        fixture.roots.opencode_history(),
        format!(
            "{{\"project\":\"{home}\",\"text\":\"zebra quantum\"}}\n\
             {{\"project\":\"{home}/child-project\",\"text\":\"mango orchard\"}}\n"
        ),
    )
    .expect("history");

    let status = index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    assert_eq!(
        coverage_of(&status, AnkaHarness::Opencode).records,
        1,
        "only the child project is published"
    );
    assert!(
        search_in(&fixture.cache, &fixture.config, query("zebra quantum"))
            .expect("search")
            .is_empty(),
        "the HOME record is gone"
    );
    assert_eq!(
        search_in(&fixture.cache, &fixture.config, query("mango orchard"))
            .expect("search")
            .len(),
        1,
        "child projects are never touched by the typed rule"
    );
}

#[cfg(unix)]
#[test]
fn home_exclude_also_drops_the_exact_encoded_home_slug() {
    let fixture = fixture();
    reinit_policy(&fixture, HomeChoice::Exclude);
    let slug = policy::encoded_home_slug(&fixture.roots.home.display().to_string());

    let mut home_slug = spec("onyx lighthouse");
    home_slug.provenance = Provenance::from_slug(&slug);
    home_slug.project = slug.clone();
    let child_slug_value = format!("{slug}-x");
    let mut child_slug = spec("saffron meadow");
    child_slug.provenance = Provenance::from_slug(&child_slug_value);
    child_slug.project = child_slug_value.clone();

    ensure_private_dir(&fixture.cache).expect("cache dir");
    write_index(
        &fixture.cache,
        &AnkaIndex {
            records: vec![record(home_slug), record(child_slug)],
            indexed_sources: 2,
            coverage: Vec::new(),
            last_indexed_at: Some(now()),
        },
    )
    .expect("hand-built cache");

    assert!(
        search_in(&fixture.cache, &fixture.config, query("onyx lighthouse"))
            .expect("search")
            .is_empty(),
        "the exact encoded home slug matches HOME"
    );
    assert_eq!(
        search_in(&fixture.cache, &fixture.config, query("saffron meadow"))
            .expect("search")
            .len(),
        1,
        "a longer slug that merely starts with the home slug is a different directory"
    );
}

#[cfg(unix)]
#[test]
fn home_keep_retains_unscoped_records_but_never_matches_a_project_filter() {
    let fixture = fixture(); // the fixture inits `keep`
    let home = fixture.roots.home.display().to_string();
    fs::write(
        fixture.roots.opencode_history(),
        format!("{{\"project\":\"{home}\",\"text\":\"home prompt content\"}}\n"),
    )
    .expect("history");

    let status = index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    assert_eq!(
        coverage_of(&status, AnkaHarness::Opencode).records,
        1,
        "kept by explicit consent"
    );
    assert_eq!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("home prompt content")
        )
        .expect("search")
        .len(),
        1,
        "a kept HOME record stays recallable without a project filter"
    );

    let filtered = AnkaSearchQuery {
        text: "home prompt content".to_string(),
        project: Some("$HOME".to_string()),
        harness: None,
        limit: 10,
    };
    assert!(
        search_in(&fixture.cache, &fixture.config, filtered)
            .expect("search")
            .is_empty(),
        "an unscoped label never satisfies a project filter — HOME is not a project"
    );

    // Conflicting provenance under `keep`: the slug says HOME while the
    // resolved path says otherwise. Ambiguous evidence is rejected, not
    // guessed at in either direction.
    let mut conflict = spec("nebula cartographer");
    conflict.provenance = Provenance::direct(&format!("{home}/elsewhere"), &fixture.roots)
        .with_slug(&policy::encoded_home_slug(&home));
    conflict.project = format!("{home}/elsewhere");
    let mut index = read_index(&fixture.cache).expect("index");
    index.records.push(record(conflict));
    write_index(&fixture.cache, &index).expect("extend cache");
    assert!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("nebula cartographer")
        )
        .expect("search")
        .is_empty(),
        "conflicting provenance is rejected even where HOME is kept"
    );
}

#[cfg(unix)]
#[test]
fn substring_rules_are_case_insensitive_and_literal() {
    let fixture = fixture();
    fs::write(
        fixture.roots.opencode_history(),
        "{\"project\":\"/srv/anka-fixture\",\"text\":\"remember this prompt\"}\n\
         {\"project\":\"/data/keeper\",\"text\":\"saffron meadowlark\"}\n",
    )
    .expect("history");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    // Case-insensitive: the rule is written in a different case than the
    // project it matches.
    fs::write(fixture.config.join(EXCLUDE_FILE), "KEEPER\n").expect("rule");
    assert!(
        search_in(&fixture.cache, &fixture.config, query("saffron meadowlark"))
            .expect("search")
            .is_empty(),
        "matching folds case on both sides"
    );
    assert_eq!(
        search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt")
        )
        .expect("search")
        .len(),
        1,
        "the unrelated project is untouched"
    );

    // Literal: `*` is a character in the rule, not a wildcard.
    fs::write(fixture.config.join(EXCLUDE_FILE), "keeper*\n").expect("rule");
    assert_eq!(
        search_in(&fixture.cache, &fixture.config, query("saffron meadowlark"))
            .expect("search")
            .len(),
        1,
        "`keeper*` is a literal substring — `/data/keeper` does not contain an asterisk"
    );
}

#[cfg(unix)]
#[test]
fn policy_show_reports_rules_tombstones_and_the_kept_excluded_breakdown() {
    let fixture = fixture();
    let home = fixture.roots.home.display().to_string();
    fs::write(
        fixture.roots.opencode_history(),
        format!(
            "{{\"project\":\"/srv/anka-fixture\",\"text\":\"remember this prompt\"}}\n\
             {{\"project\":\"/data/keeper\",\"text\":\"keeper prompt content\"}}\n\
             {{\"project\":\"/data/other\",\"text\":\"other prompt content\"}}\n\
             {{\"project\":\"{home}\",\"text\":\"home prompt content\"}}\n"
        ),
    )
    .expect("history");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    // Forget one record, then land a rule that would drop two more —
    // without rebuilding, so the breakdown documents the pending loss.
    let forgotten = read_index(&fixture.cache)
        .expect("index")
        .records
        .iter()
        .find(|record| record.content == "remember this prompt")
        .expect("the fixture record")
        .id
        .clone();
    assert!(
        forget_in(&fixture.roots, &fixture.cache, &fixture.config, &forgotten).expect("forget")
    );
    fs::write(fixture.config.join(EXCLUDE_FILE), "data\n").expect("rule");

    let view = policy::AnkaPolicy::show_view(&fixture.config, &fixture.cache, &fixture.roots)
        .expect("show is read-only and never fails on a healthy policy");
    assert!(view.initialized);
    assert_eq!(view.home.as_deref(), Some("keep"));
    assert_eq!(view.exclude_rules, vec!["data".to_string()]);
    assert_eq!(view.tombstones, 1);

    // The pre-publication count runs the same policy over freshly discovered
    // sources: the forgotten record is hidden here too, and the pending rule
    // has not dropped anything yet because no rebuild has run.
    let discovery = view
        .discovery
        .expect("an initialized policy reports discovery");
    assert_eq!(discovery.state, "ok");
    assert_eq!(discovery.discovered, 4, "all four source lines, read fresh");
    assert_eq!(
        discovery.kept, 1,
        "the kept HOME record, as in the cache view"
    );
    assert_eq!(discovery.excluded.substring, 2, "both /data projects");
    assert_eq!(
        discovery.excluded.tombstone, 1,
        "the forgotten record is counted before publication"
    );

    let cache = view.cache.expect("the policy is initialized");
    assert_eq!(cache.state, "ok");
    assert_eq!(
        cache.kept, 1,
        "the kept HOME record is the only one still admitted"
    );
    assert_eq!(cache.excluded.substring, 2, "both /data projects");
    assert_eq!(cache.excluded.tombstone, 0, "forget already removed it");
    assert_eq!(cache.excluded.total(), 2);
    let opencode = cache
        .per_harness
        .iter()
        .find(|entry| entry.harness == "opencode")
        .expect("opencode slot");
    assert_eq!((opencode.kept, opencode.excluded), (1, 2));
    assert!(
        cache
            .per_harness
            .iter()
            .filter(|entry| entry.harness != "opencode")
            .all(|entry| entry.kept == 0 && entry.excluded == 0),
        "the other harnesses saw no records"
    );
}

#[cfg(unix)]
#[test]
fn policy_show_reports_the_missing_state_without_failing() {
    let fixture = fixture();
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture policy");
    fs::write(fixture.config.join(EXCLUDE_FILE), "personal\n").expect("rule file");

    let view = policy::AnkaPolicy::show_view(&fixture.config, &fixture.cache, &fixture.roots)
        .expect("`show` diagnoses the gap instead of erroring like recall");
    assert!(!view.initialized);
    assert!(view.home.is_none());
    assert!(
        view.cache.is_none(),
        "no admission decision to report without a policy"
    );
    assert!(
        view.discovery.is_none(),
        "nor a pre-publication count — there is no policy to count against"
    );
    assert_eq!(
        view.exclude_rules,
        vec!["personal".to_string()],
        "the operator's own rules are still readable"
    );
}

// ─── Phase 3 corrections: consent-bound HOME, fail-closed diagnostics ───

/// Claude stores its project as a directory slug: `source.project` is the
/// slug while the trustworthy directory exists only in the provenance. An
/// exclusion naming that directory must still exclude the record — the
/// display label alone is not the whole project claim.
#[cfg(unix)]
#[test]
fn substring_rules_match_the_resolved_path_and_the_raw_slug_not_only_the_display() {
    let fixture = fixture();
    let claude = fixture
        .roots
        .claude_projects()
        .join("-srv-private")
        .join("sess-p.jsonl");
    fs::create_dir_all(claude.parent().expect("claude project dir")).expect("claude dir");
    fs::write(
        &claude,
        format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant",
                "cwd": "/srv/private",
                "message": {"content": "velvet semaphore"}
            })
        ),
    )
    .expect("claude transcript");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");
    assert_eq!(
        search_in(&fixture.cache, &fixture.config, query("velvet semaphore"))
            .expect("search")
            .len(),
        1,
        "baseline: the record recalls while no rule names it"
    );

    // The raw slug is a match candidate in its own right.
    fs::write(fixture.config.join(EXCLUDE_FILE), "-srv-private\n").expect("rule");
    assert!(
        search_in(&fixture.cache, &fixture.config, query("velvet semaphore"))
            .expect("search")
            .is_empty(),
        "a rule matching the raw slug must exclude the record"
    );

    // The exclusion names the resolved directory; the display is only the
    // slug, so pre-fix this rule matched nothing.
    fs::write(fixture.config.join(EXCLUDE_FILE), "/srv/private\n").expect("rule");
    assert!(
        search_in(&fixture.cache, &fixture.config, query("velvet semaphore"))
            .expect("search")
            .is_empty(),
        "the provenance path must be a match candidate"
    );
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("rebuild");
    assert_eq!(
        read_index(&fixture.cache).expect("index").records.len(),
        1,
        "the rebuild applies the same predicate: only the fixture record survives"
    );
}

/// The consent names *which* home the decision was made for. When the
/// process later runs with a different `$HOME`, the record at the consented
/// home must still be excluded — and the new process home is simply not the
/// consented subject.
#[cfg(unix)]
#[test]
fn the_home_decision_follows_the_consent_path_not_the_process_home() {
    let fixture = fixture();
    let old_home = fixture
        .roots
        .home
        .parent()
        .expect("temp root")
        .join("old-home");
    fs::create_dir_all(&old_home).expect("old home dir");
    fs::remove_file(fixture.config.join(policy::POLICY_FILE)).expect("drop fixture consent");
    let consent_roots = AnkaRoots {
        home: old_home.clone(),
        config: fixture.roots.config.clone(),
        cache: fixture.roots.cache.clone(),
    };
    policy::AnkaPolicy::init(&fixture.config, HomeChoice::Exclude, &consent_roots)
        .expect("consent recorded for the old home");

    let old_path = old_home.to_string_lossy().into_owned();
    let process_home = fixture.roots.home.to_string_lossy().into_owned();
    fs::write(
        fixture.roots.opencode_history(),
        format!(
            "{{\"project\":\"{old_path}\",\"text\":\"ember archipelago\"}}\n\
             {{\"project\":\"{process_home}\",\"text\":\"copper tidepool\"}}\n"
        ),
    )
    .expect("history");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    let index = read_index(&fixture.cache).expect("index");
    assert_eq!(
        index.records.len(),
        1,
        "the consented home is excluded wherever the process home now lives"
    );
    assert_eq!(
        index.records[0].content, "copper tidepool",
        "only the process-home record — which the consent never names — survives"
    );
    assert!(
        search_in(&fixture.cache, &fixture.config, query("ember archipelago"))
            .expect("search")
            .is_empty(),
        "recall must agree with the rebuild"
    );

    // The surviving record's unscoped home label is still not a project.
    let mut project_query = query("copper tidepool");
    project_query.project = Some("$HOME".to_string());
    assert!(
        search_in(&fixture.cache, &fixture.config, project_query)
            .expect("search")
            .is_empty(),
        "an unscoped home label never satisfies a project filter"
    );
}

/// `/tmp/x//home` names `/tmp/x/home` — POSIX resolves an interior `//`
/// exactly like a single `/`. Such a spelling must not slip past the exact
/// home comparison as an ordinary scoped project.
#[cfg(unix)]
#[test]
fn an_asserted_home_with_interior_double_slashes_is_normalized_before_the_home_comparison() {
    let fixture = fixture();
    reinit_policy(&fixture, HomeChoice::Exclude);
    let base = fixture.roots.home.parent().expect("temp root");
    let double = format!(
        "{}//{}",
        base.display(),
        fixture
            .roots
            .home
            .file_name()
            .expect("home name")
            .to_string_lossy()
    );
    let home = fixture.roots.home.to_string_lossy().into_owned();
    assert_ne!(double, home, "the spelling must carry an interior `//`");
    assert_eq!(
        provenance::normalize_path(&double).as_deref(),
        Some(home.as_str()),
        "normalization collapses the run to the canonical form"
    );
    fs::write(
        fixture.roots.opencode_history(),
        format!("{{\"project\":\"{double}\",\"text\":\"linen monsoon\"}}\n"),
    )
    .expect("history");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    assert_eq!(
        read_index(&fixture.cache).expect("index").records.len(),
        0,
        "the double-slashed home is the consented home, not a scoped project"
    );
    assert!(
        search_in(&fixture.cache, &fixture.config, query("linen monsoon"))
            .expect("search")
            .is_empty(),
        "recall must agree"
    );
}

/// The consent's subject feeds exact comparisons, so it must arrive in the
/// canonical form `init` writes: a relative path or a non-normalized
/// spelling would let formatting decide what HOME means.
#[cfg(unix)]
#[test]
fn a_policy_home_path_that_is_not_already_normalized_is_malformed() {
    let fixture = fixture();
    let path = fixture.config.join(policy::POLICY_FILE);

    for bad in ["relative/path", "/srv/home/", "/srv//home"] {
        fs::write(
            &path,
            format!("# anka-policy v1\nhome = keep\nhome_path = {bad}\n"),
        )
        .expect("policy");
        let error = search_in(
            &fixture.cache,
            &fixture.config,
            query("remember this prompt"),
        )
        .expect_err("recall refuses a policy whose subject is not canonical");
        assert!(error.to_string().contains("malformed"), "{error}");
        assert!(
            error.to_string().contains("already-normalized"),
            "the message says what is wrong: {error}"
        );
    }

    // Rebuild refuses the same file — no surface honors it.
    fs::write(
        &path,
        "# anka-policy v1\nhome = keep\nhome_path = relative/path\n",
    )
    .expect("policy");
    let error = index_in(&fixture.roots, &fixture.cache, &fixture.config, None)
        .expect_err("rebuild refuses a non-canonical consent subject too");
    assert!(error.to_string().contains("malformed"), "{error}");
}

/// An unreadable cache is not an empty one: reporting `absent` with zero
/// records would claim a total loss that never happened.
#[cfg(unix)]
#[test]
fn policy_show_reports_an_unreadable_cache_as_unreadable_not_as_absent() {
    let fixture = fixture();
    let view = policy::AnkaPolicy::show_view(&fixture.config, &fixture.cache, &fixture.roots)
        .expect("show");
    assert_eq!(
        view.cache.expect("cache").state,
        "absent",
        "before the first build there genuinely is no cache"
    );
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    let mut permissions = fs::metadata(&fixture.cache)
        .expect("cache dir")
        .permissions();
    permissions.set_mode(0o000);
    fs::set_permissions(&fixture.cache, permissions.clone()).expect("lock the cache dir");
    let view = policy::AnkaPolicy::show_view(&fixture.config, &fixture.cache, &fixture.roots)
        .expect("show stays diagnostic");
    // Restore before asserting: the tempdir cleanup needs the directory back,
    // and a failing assertion must not leave permissions changed behind it.
    permissions.set_mode(0o700);
    fs::set_permissions(&fixture.cache, permissions).expect("unlock the cache dir");

    let cache = view.cache.expect("cache");
    assert_eq!(
        cache.state, "unreadable",
        "an inspection failure is not an absence"
    );
    assert!(
        cache.detail.is_some(),
        "the detail carries the I/O failure: {cache:?}"
    );
    assert_eq!(cache.kept, 0);
}

/// The cache breakdown can only measure what the cache *holds* — a record
/// the policy filtered out at its first index left no trace there. The
/// discovery breakdown applies the same policy to the sources themselves,
/// so what a rebuild would drop is countable before publication.
#[cfg(unix)]
#[test]
fn policy_show_counts_discovery_against_the_policy_before_publication() {
    let fixture = fixture();
    fs::write(fixture.config.join(EXCLUDE_FILE), "keeper\n").expect("rule");
    fs::write(
        fixture.roots.opencode_history(),
        "{\"project\":\"/srv/anka-fixture\",\"text\":\"remember this prompt\"}\n\
         {\"project\":\"/data/keeper\",\"text\":\"saffron meadowlark\"}\n",
    )
    .expect("history");
    index_in(&fixture.roots, &fixture.cache, &fixture.config, None).expect("build");

    let view = policy::AnkaPolicy::show_view(&fixture.config, &fixture.cache, &fixture.roots)
        .expect("show");

    let cache = view.cache.expect("cache");
    assert_eq!(cache.state, "ok");
    assert_eq!(
        cache.kept, 1,
        "the cache never held the keeper: the policy filtered it at index time"
    );
    assert_eq!(
        cache.excluded.substring, 0,
        "the cache breakdown cannot see a record the policy never let in"
    );

    let discovery = view.discovery.expect("pre-publication count");
    assert_eq!(discovery.state, "ok");
    assert_eq!(discovery.discovered, 2, "both source lines, read fresh");
    assert_eq!(
        discovery.kept, 1,
        "the same admission a rebuild would apply"
    );
    assert_eq!(
        discovery.excluded.substring, 1,
        "the keeper is counted against the policy before any publication"
    );
    assert_eq!(discovery.excluded.total(), 1);
    let opencode = discovery
        .per_harness
        .iter()
        .find(|entry| entry.harness == "opencode")
        .expect("opencode slot");
    assert_eq!((opencode.kept, opencode.excluded), (1, 1));
}
