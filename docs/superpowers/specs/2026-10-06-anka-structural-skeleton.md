# ANKA Structural Skeleton (Phase 0 contract review)

Date: 2026-10-06
Owner: OpenCode Kaira
Status: Structural design for approval. No implementation yet.
Companion plan: `docs/superpowers/plans/2026-10-06-anka-reliability-and-privacy.md`

## 1. Isolation record

| Item | Value |
| --- | --- |
| Live tree branch | `feature/rate-limits-and-tray-hardening` @ `6becca6` |
| Isolated checkout | `.worktrees/anka-reliability-privacy`, detached `6becca6` |
| Live tree dirty files | 23, stash empty, **untouched by this work** |
| Installed CLI | `~/.local/bin/raios` 3.9.0, built 2026-10-04 20:15 |
| Concurrent sessions | 3 `opencode_kaira`, 2 `codex_kaira`, 1 `claude_kaira` reported `running` |

`.worktrees/` is gitignored, so the checkout is invisible to other agents' `git status`.
`.cargo/` contains only `audit.toml` (no vendored-source replacement), so the isolated
checkout builds without the untracked `vendor/` directory.

## 2. Mandatory scans

`raios deps` — lockfile present, no known CVEs, all dependencies up to date.

`raios security` — 40/100 (D): 0 critical, 2 high, 2 medium, 2 low. Reviewed individually
against the ANKA change boundary rather than trusted at face value:

| Severity | Location | Verdict |
| --- | --- | --- |
| HIGH | `tools/raios-workspace/src-tauri/src/tests.rs:53` | False positive — asserts traversal is rejected |
| HIGH | `tools/raios-workspace/src-tauri/src/memory.rs:100` | **Likely real**, outside ANKA |
| MED ×2 | `.../gen/schemas/*.json` `$schema` `http://` | False positive — identifier, not a fetch target |
| LOW | `.../auth.rs:78` header `.parse().unwrap()` | False positive — static string |
| LOW | `surface-cli/tests/handoff_delivery.rs:41` | Test asserting `0o700`; untracked, other agent's work |

**Zero findings fall inside `crates/*/src/anka*`.** The 40/D score is driven entirely by
`tools/raios-workspace/`, which is untracked in-flight work from another stream.

Action required outside this plan: `memory.rs:100` uses `format!("SELECT count(*) {from}")`.
If `from` reaches the query from a caller, that is SQL injection. Report to the owner of
`tools/raios-workspace`; do not edit it from this stream.

## 3. Contract defects found in the current boundary

Evidence lines are against `6becca6`.

### C1 — No interprocess lock exists
Plan Phase 0 assumes "the lock protecting index/forget operations" exists to be reviewed.
It does not: no `lock`/`flock`/`try_lock` reference in `crates/raios-runtime/src/anka.rs`,
none workspace-wide tied to ANKA.

Consequence: `forget` performs `read_index` → `append_line` → `write_index`, and
`append_line` itself is read-modify-write of the whole file. Two concurrent `forget`s can
lose a tombstone; `forget` racing `index` can publish an index built from a stale tombstone
set. This is a **new contract to add**, not an existing one to inspect.

### C2 — `atomic_write` has both defects the plan predicts
```rust
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");   // fixed name: index.json -> index.tmp
    fs::write(&temporary, content)?;              // created with umask perms (0644)
    set_owner_only(&temporary)?;                  // chmod AFTER write
    fs::rename(temporary, path)?;
    set_owner_only(path)
}
```
- Permissive window: the temp file is world-readable between `fs::write` and `set_owner_only`.
- Non-unique name: concurrent writers share `index.tmp`, so bytes interleave and a publish
  can be lost or corrupt.
- `harden_file_perms` sets `0o600`, and `ensure_private_dir` sets `0o700`, so the *final*
  modes are already correct. The gap is the creation path, not the target mode.

### C3 — Policy is evaluated at index time only
`EXCLUDE_FILE`/`TOMBSTONE_FILE` are read only inside `index` and on `forget`'s append.
`search` and `blame` never read either. Therefore:
- Editing `anka-exclude` has no effect until the next rebuild (plan decision #4).
- If `forget` appends the tombstone but its `write_index` fails, the record stays in the
  cache **and remains searchable**, because recall does not consult tombstones.
- MCP `tool_anka_recall` calls `runtime::anka::search`, so it inherits the same gap.

### C4 — The advertised contract is decorative; three surfaces disagree
- `AnkaRecallStore` and `AnkaImporter` are never implemented anywhere.
- `AnkaSearchRequestDto`, `AnkaHitDto`, `AnkaIndexStatusDto` are only re-exported from
  `contracts/src/lib.rs`; CLI and MCP never use them.
- The real boundary is the free functions in `raios_runtime::anka`, and the CLI hand-builds
  `serde_json::json!` per action.
- Shape divergence: `AnkaImporter::index(&self, harnesses: &[AnkaHarness])` versus the free
  `index(harness: Option<AnkaHarness>)`.

Plan decision #2 requires status additions "reviewed at the CLI/DTO boundary", but there is
currently no DTO boundary to review.

### C5 — `state` is derived in the CLI and misreports partial rebuilds
`state` is `last_indexed_at.is_some()`. Measured live: after `index --harness claude` the
CLI reported `58/58, state ready`; after `index --harness opencode`, `1 source, 0 records,
state ready`. `AnkaIndexStatus` carries no per-harness coverage, so a refresh of one harness
is presented as a full refresh. This is plan line 136-138.

### C6 — No schema version field
`AnkaIndex` is `{records, indexed_sources, last_indexed_at}` with no `schema_version`, and
`read_index` parses directly. Serde ignores unknown fields by default, so an older binary
reading a newer cache would silently accept it instead of failing safely.

### C7 — Stable identity confirmed as fragile
`record()` hashes `[harness, project, session_id, occurred_at, content]`, so any timestamp or
content change produces a new id. `discover_jsonl` substitutes file mtime when the record has
no timestamp:
```rust
occurred_at.unwrap_or_else(|| modified_at(path)),
```
OpenCode history carries neither `ts` nor `timestamp`, so **all OpenCode ids change whenever
one more prompt is appended**, defeating every existing tombstone for that harness. Codex
history does carry `ts`, so it is stable per prompt.

### C8 — No injectable roots
`default_cache_path()` uses `dirs::cache_dir()`, `config_file()` uses `dirs::config_dir()`,
`discover_*` uses `dirs::home_dir()`. Phase 4 requires synthetic source/config/cache roots,
so root injection is a prerequisite, not a refactor detail.

### C9 — Claude project attribution uses the immediate parent
`path.parent().file_name()` yields `subagents` for nested agent files (plan line 29).

### C10 — `contains` is case-insensitive substring
Confirms both plan warnings: literal rules are not glob/regex, and a `-home-alaz` rule would
match all 10 Claude project directories, so exact-match is required if HOME is excluded.

### C11 — Harness alias is inconsistent across surfaces
`parse_harness` accepts `agy`, but `AnkaHarness::as_str()` emits `antigravity`, and the MCP
`inputSchema` enum documents only `antigravity`. Runtime accepts what the schema omits.

### C12 — `blame` is `search` on the basename
No path scoping: it matches the filename anywhere in the corpus. Phase 3 acceptance requires
exclusions to block `blame`, which is achievable, but blame's own correctness is unaddressed.

## 4. Proposed type skeleton

Shapes only; bodies land in Phases 1-3.

```rust
/// Single source of every filesystem root. Only `AnkaRoots::live()` may consult
/// the environment; tests construct this directly.
pub struct AnkaRoots {
    pub home: PathBuf,     // harness transcript roots
    pub config: PathBuf,   // anka-exclude, anka-tombstones
    pub cache: PathBuf,    // index + cache-side tombstones
}

pub enum ProjectScope {
    /// Trustworthy project metadata, or a cwd reported by the harness itself.
    /// The value is a real filesystem path the harness asserted.
    Scoped(String),
    /// Raw directory slug exactly as stored by the harness (e.g. `-home-alaz-dev-ai-svp`).
    /// Lossy and opaque. This carries slug evidence only; it never claims to be the
    /// resolved filesystem path, and no path is reconstructed from it.
    Slug(String),
    /// Resolved exactly to `$HOME`.
    HomeUnscoped,
    /// No provable project.
    Unknown,
}

pub enum ProvenanceQuality {
    Direct,        // cwd / workspace field from harness metadata
    SessionCwd,    // history record joined to session_meta payload id
    SlugEncoded,   // evidence is the raw slug string only; no path was reconstructed
    Unknown,
}

pub struct Provenance {
    pub scope: ProjectScope,
    pub quality: ProvenanceQuality,
    pub observed_via: &'static str, // field name that produced it; never prompt text
}

/// Durable **forget** identity. Never derived from mtime, observed timestamp, or
/// line number. This is the basis of `forget_key` only — it is not `record_id`.
pub enum SourceIdentity {
    /// Claude: harness + project dir (resolved against `.claude/projects`) + session + agent.
    Transcript { project_dir: String, session_id: String, agent_id: Option<String> },
    /// History with a native immutable id, namespaced by harness and source.
    Native { source: String, native_id: String },
    /// History without native ids: versioned fingerprint of the redacted prompt.
    Fingerprinted { source: String, version: u8, prompt_hash: String },
}

pub enum Decision { Keep, Exclude(Reason) }

pub enum Reason {
    LiteralMatch, HomeUnscoped, ProjectUnknown,
    ConflictingProvenance, Tombstoned,
}

pub enum PolicyError {
    Missing { path: PathBuf },        // not initialized -> safe error, never silent empty
    Malformed { path: PathBuf, line: usize },
    Unreadable { path: PathBuf, source: std::io::Error },
}

pub struct Policy { /* literal rules + scope rule */ }
impl Policy {
    pub fn load(config: &Path) -> Result<Self, PolicyError>;
    pub fn evaluate(&self, provenance: &Provenance) -> Decision;
}

/// **NEW contract, added in Phase 2 step 2.** No interprocess lock existed (defect C1),
/// so this is a contract to add, not a review of existing structure. Advisory lock on
/// `<cache>/anka.lock`, shared by `index` and `forget`, held across
/// read-tombstones → build → publish. Dropping the value releases it — the lock lives on
/// the descriptor, so a crash cannot strand it.
pub struct AnkaLock { /* held `File` on `<cache>/anka.lock` */ }
impl AnkaLock {
    pub fn acquire(cache: &Path) -> Result<Self>;        // waits
    pub fn try_acquire(cache: &Path) -> Result<Option<Self>>; // `None` = busy
}

pub struct HarnessCoverage {
    pub sources: usize,
    pub records: usize,
    pub indexed_at: String,
    pub full_refresh: bool,
}

pub struct AnkaIndex {
    pub schema_version: u32,                       // NEW
    pub records: Vec<AnkaRecord>,
    pub coverage: Vec<(AnkaHarness, HarnessCoverage)>, // NEW
    pub last_indexed_at: Option<String>,
}

/// `state` becomes derivable instead of CLI-guessed:
/// ready | partial | empty | incompatible
pub struct AnkaIndexStatusDto { /* additive: state, coverage, policy summary */ }

/// Publish path (`anka/publish.rs`): created `0600` at open time (no chmod-after
/// window), unique temporary name under `O_EXCL`, `fsync` of file, `rename`, then
/// `fsync` of the directory — all under the lock.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()>;
```

### Identity split — `record_id` vs `forget_key`

These are two different identifiers and must not be collapsed.

```rust
/// Unique per stored record. Stable enough to identify this record inside one
/// index build; safe as the dedup key, because two distinct events never share it.
pub struct RecordId(String);

/// Durable forget handle, derived from `SourceIdentity`. A forget is recorded
/// against this, so it survives append/mtime/timestamp changes. Never used as a
/// dedup key.
pub struct ForgetKey(String);
```

Rules:
1. `record_id` may change when an observed timestamp or file mtime changes; that is
   acceptable because it is never the forget handle.
2. `forget_key` is derived only from `SourceIdentity`, so an OpenCode append (which
   rewrites mtime) does not revive a forgotten prompt.
3. `dedup_by` operates on `record_id` only. **Removing distinct events from the index
   because they share a `forget_key` is not approved.** Identical redacted prompts
   remain separate records; they simply belong to one forget group, so forgetting one
   hides them all.
4. Codex `session_id` is a session identifier, not a prompt identifier. It may be part
   of a source namespace but can never stand alone as a prompt identity.
5. `forget` accepts either handle: a `record_id` resolves to its `forget_key`, and the
   durable tombstone written is always the `forget_key`.

### Cache format rejection (replaces plain `schema_version`)

Adding `schema_version` alone is insufficient: the current reader is
`serde_json::from_str::<AnkaIndex>`, and serde ignores unknown fields by default, so an
older binary would silently accept a newer cache.

The v1 reader requires `records`, `indexed_sources`, and `last_indexed_at`. The v2
envelope therefore **omits the legacy field names entirely**:

```rust
// v1 (current): {"records":[…],"indexed_sources":N,"last_indexed_at":…}
// v2:           {"schema_version":2,"records_v2":{…},"coverage":{…},…}
```

- Old binary → missing required field → `Err` → existing message
  `"ANKA index is corrupt; run \`raios anka index\` to rebuild it"`: an explicit refusal,
  not a silent accept and not a silent empty.
- New binary → reads `schema_version`, then branches. Unknown future value → safe error,
  no migration and no rebuild as a recall side effect (plan Phase 2, line 139-140).
- The v1 file is replaced in place during migration under `AnkaLock`, so no unfiltered
  broad cache is left behind (plan Phase 3, line 163-165).

Trade-off to keep visible: an old binary shown the corrupt message may be *run* by the
operator, and old `index` would rebuild the unfiltered cache. That path is blocked by
policy (Phase 5 rollback forbids reverting to a binary without the protections), not by
the format.

### Policy command contract — `anka policy init|show`

```rust
AnkaAction::PolicyInit { home: HomeChoice }  // `home` is a REQUIRED flag
AnkaAction::PolicyShow                       // read-only
```

| Rule | Requirement |
| --- | --- |
| Versioning | `init` writes a versioned policy header (e.g. `# anka-policy v1`). |
| Non-destructive | Existing `anka-exclude` and `anka-tombstones` are never rewritten, merged over, or truncated. If policy already exists, `init` refuses with a usage error unless an explicit `--force` path is added later. |
| HOME argument | `--home keep` or `--home exclude` is mandatory. No default, no inference: a missing flag is a usage error with exit ≠ 0. |
| `show` | Read-only. Prints resolved rules, effective scope behaviour, and the tombstone count. Never writes, never migrates, never indexes. |
| No side effects | Creating a policy must not start indexing, must not enable or start the timer, and must not touch the cache. |
| Consent | Policy creation is separate from tombstone initialization (plan Phase 3, line 148-150). |

### Shared output types (CLI + MCP)

The unused `AnkaRecallStore` / `AnkaImporter` traits are **out of scope this round** —
implementing them is not a requirement. What is required is one definition of the shape
both surfaces return:

```rust
pub struct AnkaStatusView { /* state, cache_path, coverage, counts, last_indexed_at, policy summary */ }
pub struct AnkaHitView    { /* id, forget_key, harness, project, provenance, session, occurred_at, snippet, score, confidence */ }
```

`raios-surface-cli/src/cli/anka.rs` and `raios-surface-mcp/…/tools_workspace.rs` both
build these views; `state` is computed from `coverage` inside the view, never guessed by
the caller. The existing contract DTOs are adopted only if they match this shape;
otherwise they stay untouched rather than becoming a second competing definition.

### Skeleton rules
1. Only `AnkaRoots::live()` reads the environment; every other path flows from the struct.
2. Provenance is established from harness metadata only. Prompt text never assigns scope.
3. Identity never contains mtime, observed timestamp, or content beyond a versioned fingerprint.
4. `Policy::load` fails closed. Absence of the initialized file is an error, not an empty allow.
5. Policy is evaluated in `index`, `search`, and `blame`, so recall changes without a rebuild.
6. `publish` and `forget` share one `AnkaLock`; a failed run leaves the previous valid cache.
7. `state` is computed from `coverage`, never from `last_indexed_at.is_some()`.

## 5. File boundary

Primary (all structure and its tests):
- `crates/raios-runtime/src/anka.rs`

Conditional, only if additive status/provenance fields require them:
- `crates/raios-core/src/anka.rs` (types: provenance, identity, decision)
- `crates/raios-contracts/src/anka.rs` (status DTO)
- `crates/raios-surface-cli/src/cli/anka.rs` and `args.rs` (`AnkaAction`)
- `crates/raios-surface-mcp/src/mcp/tools_workspace.rs` (`tool_anka_recall`)

Proposed split of the primary file, approved with the correction that the importer file
is `importers.rs` (not `imports.rs`) and `mod.rs` is the entry point that re-exports the
existing public functions, so no external call site changes:
`crates/raios-runtime/src/anka/{mod.rs, importers.rs, provenance.rs, policy.rs, identity.rs, lock.rs, publish.rs}`.
New helpers stay inside their owning module unless there is a demonstrated second consumer.

Documentation:
- `docs/ANKA.md`, README ANKA section, `memory.md`, `SIGMAP.md`

Service artifacts — convention located (plan required this before adding a directory):
- Precedent is **generated** unit content, not checked-in `.service` files:
  `crates/raios-surface-cli/src/cli/hub.rs:355-425` builds the unit string and writes to
  `~/.config/systemd/user/`, then runs `systemctl --user enable --now`.
- Static exception: `tools/raios-tray/raios-tray.service`.
- No `.timer` unit exists anywhere in the repository yet, so the timer pair
  (`raios-anka-index.service` + `.timer`) is new. It should follow the `hub.rs` generator
  pattern so `UMask`, `Persistent`, and the absolute binary path are produced from live values.
  *(Implemented 2026-10-08 as `cli/anka_timer.rs`: both units are generated at
  install time and still no unit file is checked in.)*

## 6. Decisions and remaining gates

Resolved this round:

1. **File split** — approved as `anka/{mod, importers, provenance, policy, identity, lock, publish}.rs`.
2. **Policy commands** — `anka policy init|show` approved with the contract in section 4.
3. **Identity split** — `record_id` and `forget_key` are separate; `dedup_by` never removes
   distinct events that share a forget group.
4. **Lock wording** — `AnkaLock` is a new contract, not a review of existing structure.
5. **Format rejection** — plain `schema_version` is replaced by the v2 envelope design.
6. **Shared output types** — required; implementing the unused traits is not.
7. **HOME retention** — decided 2026-10-08 (operator) as `keep`: the live `policy-init`
   runs with `--home keep`; home records stay recallable unfiltered but never satisfy a
   project filter.
8. **Timer** — closed 2026-10-08. Implemented as `cli/anka_timer.rs` following the
   `hub.rs` generator convention (units generated from live values, never checked in):
   `timer-install` writes and enables the `raios-anka-index.{service,timer}` pair and
   refuses to schedule until the policy is initialized; `timer-uninstall` disables and
   removes the pair while retaining policy, tombstones, and cache. The operative
   oneshot time bound is `TimeoutStartSec=300s` (a completed oneshot is no longer
   running, so `RuntimeMaxSec` would never bite); `MemoryMax=512M`,
   `UMask=0077`, daily `04:00` local with `Persistent=true` and a randomized delay.
   Acceptance verified live: gate refusal without a policy, `systemd-analyze verify`
   clean, manual invocation `Result=success`, next activation scheduled, failure and
   disable behavior exercised without waiting a day.

Still open:

1. None — the timer, the last gate, is closed (resolved item 8).

Must close before the live migration:

1. **Descriptor identity at open — closed 2026-10-08.** `open_regular_file` now opens
   with `O_NOFOLLOW` (new unix-targeted `libc = "0.2"` dependency; std exposes
   `OpenOptions::custom_flags` but not the flag constants) and then proves the
   *descriptor* with `fstat`: the fd must be regular and carry the same device + inode
   the pre-open `symlink_metadata` saw. Both swap shapes are refused — a symlink planted
   between the check and the open (the open itself fails: ELOOP on Linux, EEXIST on
   macOS), and a different regular file planted there (the identity comparison fails) —
   and the refusal is `InvalidInput`, the same "not a regular file" answer discovery
   already counts as `skipped_non_regular`, never an import abort. The post-open path
   re-check is retained only for its original contract (a path changed or gone before
   return is reported). Non-unix builds keep the plain open between the two path checks.
   Red/green proven: dropping `O_NOFOLLOW` makes
   `a_symlink_swapped_in_between_the_path_check_and_the_open_is_refused` red (it pins
   the O_NOFOLLOW refusal message); dropping the identity comparison makes
   `a_different_regular_file_swapped_in_between_the_checks_is_not_returned` red (the
   old code returned `Ok` carrying the attacker's file). Both tests swap the path
   through the `#[cfg(test)]` seam between check 1 and the open — the exact window the
   path checks cannot observe; the seam compiles out of non-test builds.

Phase 1 bounded-read round (verified):

1. **No unbounded drain.** `next_line_bounded` stops at the budget and returns
   `BoundedLine::Oversized` without consuming the rest of the line; a header reader
   drops the file on that outcome, and a scanning reader skips through
   `skip_rest_of_line`, which only walks `fill_buf`/`consume` and never allocates.
   Regression-proven: reverting to the old drain makes the two budget fixtures fail with
   `reading must stop at the budget, consumed 4194305 bytes instead`.
2. **Oversize verdict carries its own consequence.** `Oversized { line_complete }`:
   `take(max + 2)` swallows a terminator that fits the budget, so an unconditional skip
   after an oversized line deleted the *next* line (probe: `limit=8`,
   `"123456789\nNEXT\n"` → `next=EOF`). `line_complete` is decided only from what the
   bounded read already produced — terminator present, or a short read meaning EOF —
   and the caller skips only when it is `false`. Pinned across LF, CRLF, and
   newline-free EOF at the exact boundary, in the helper *and* through
   `first_top_level_string`; forcing the old unconditional skip turns the Claude
   production-path fixture red.
3. **History files stream bounded too.** `discover_history` no longer uses
   `BufReader::lines()` (which materialized a whole line). Every harness's history file
   reads through `HISTORY_LINE_MAX_BYTES` = 256 KiB — measured maxima are 81,021
   (Codex), 4,774 (OpenCode), 3,974 (Antigravity) — and an over-budget line is skipped
   through a fixed buffer and counted in a new `ImportReport::oversized`, its own
   outcome rather than `malformed`. `ImportReport` is internal to the `anka` module, so
   the new counter does not move the CLI/MCP contract; exposing it as a separate
   exclusion reason belongs to Phase 2 coverage. Line-number discriminators are proven
   unaffected by three-way control (oversized / malformed / absent line 2).

Phase 2 step 2 — lock and publish (verified):

1. **`lock.rs` — `AnkaLock` (defect C1 closed).** Advisory lock on `<cache>/anka.lock`
   via `std::fs::File::lock` (stable since 1.89, so no `libc` dependency), created `0600`
   with `O_EXCL` and, once present, opened through `provenance::open_regular_file` so a
   planted symlink is refused instead of locked. `index_in` and `forget_in` both take it
   and hold it across read-tombstones → build → publish and read-index → append →
   write-index respectively; readers stay lockless because publication is atomic.
   Deviation from the skeleton line as first drafted: `acquire` returns `anyhow::Result`
   rather than a dedicated `LockError` — the only distinct outcome callers need is
   "busy", and that is `try_acquire`'s `Ok(None)`.
   Red/green proven: removing both `acquire` calls turns
   `an_index_run_takes_the_lock_before_it_reads_tombstones` and
   `forget_waits_while_an_index_run_holds_the_lock` red with their own messages.
2. **`publish.rs` — defect C2 closed.** `create_owner_only` applies `mode(0o600)` at
   `open(2)`, so the file is owner-only from the instant it exists (umask can only
   remove bits); there is no chmod afterwards because there is no window to close.
   Temporary names carry pid + a process-wide counter and are claimed with
   `O_EXCL`, so concurrent publishers cannot interleave. `fsync` of the file before
   `rename`, `fsync` of the directory after, and a failure removes its own temporary and
   leaves the previous file byte-identical. `ensure_private_dir` now creates with
   `DirBuilder::mode(0o700)` (no permissive interval) and still hardens a directory made
   by older code. An existing world-readable destination becomes `0600` on publish.
3. **Testability without touching the real home.** `index`/`forget` are thin wrappers
   over `index_in(roots, cache, config, harness)` and `forget_in(cache, config, id)`,
   so the concurrency fixtures use temp directories instead of mutating process-global
   `HOME`/`XDG_CONFIG_HOME` — the exact mechanism that makes the unrelated
   `control_plane` test environment-dependent.

Phase 2 step 3 — publish phases and recall-side tombstones (verified):

1. **Publish failures carry their phase.** `PublishError` distinguishes
   `NotPublished` (nothing reached the destination; the previous file is intact) from
   `PublishedUnsynced` (the rename already happened, so readers see the new content;
   only crash-durability is unconfirmed). The old contract reported both as a generic
   error, which would tell a caller "the old file is intact" at the exact moment it is
   no longer true. `atomic_write` no longer creates or hardens directories itself —
   callers do that on entry — so a permission failure surfaces instead of being masked
   by a hidden `chmod`. A test injects a post-rename directory-sync failure and asserts
   the error is `PublishedUnsynced` *and* that the destination already holds the new
   content.
2. **A durable tombstone is never rolled back.** If the index publish fails after the
   tombstone lands, the tombstone stays: undoing it would resurrect the record, which is
   the exact outcome `forget` exists to prevent. The failure is reported with its phase,
   and the record stays hidden meanwhile.
3. **Recall checks tombstones, not just the rebuild.** `search_in` reads the index
   snapshot first and the tombstone set second, so a `forget` landing between the two
   reads is still honoured; the opposite order would briefly show a record the user just
   removed. `blame_in` routes through the same path, and the MCP recall tool inherits the
   check by calling `search`. Acceptance proven: with a durable tombstone and a stale
   index that still holds the record (the exact state a failed publish leaves behind),
   `search` and `blame` both return nothing. Red/green proven: removing the filter turns
   `a_durable_tombstone_hides_a_record_even_when_the_index_publish_fails` red with its
   own message.

Phase 2 step 3 — `forget_key` (verified):

1. **`record_id` and `forget_key` are separate handles.** `record_id` includes the
   discriminator and observed timestamp, so it is unique per stored record but volatile.
   `forget_key` is derived only from `SourceIdentity` — harness, stable source
   namespace, session, parent session, agent, and (for prompt-history records) a
   fingerprint of the redacted prompt — so it survives append/mtime changes. `forget`
   accepts a `record_id` and writes the `forget_key` to the tombstone.
2. **Prompt-history records carry a prompt fingerprint; transcript records do not.**
   History records have no native immutable ID, so their forget identity includes a
   hash of the redacted prompt (identical prompts share a forget group). Claude
   transcripts identify by source alone — appending a turn does not change the forget
   key.
3. **Recall filters by `forget_key`.** `search_in` and `blame_in` check tombstones
   against `record.forget_key`, not `record.id`. Red/green proven: removing the filter
   turns `a_durable_tombstone_hides_a_record_even_when_the_index_publish_fails` red.
4. **History identity uses a stable source namespace, not the resolved project.** The
   history importer resolves the display project from provenance, but `forget_key`
   uses the history file stem (`identity_project`). A provenance transition
   (Unknown → SessionCwd) changes the display project but not the forget key.
   Red/green proven: reverting to `spec.project` turns
   `provenance_transition_does_not_change_forget_key` red.
5. **The rebuild filter also uses `forget_key`.** `index_in` retains records whose
   `forget_key` is not tombstoned, so a rebuild cannot republish a forgotten record.
   Red/green proven: reverting to `record.id` turns
   `a_rebuild_does_not_republish_a_forgotten_record` red.
6. **`add_tombstone` propagates read errors other than `NotFound`.** Only a missing file
   may start from empty; permissions or I/O errors must leave the file untouched
   rather than being silently treated as empty and then overwritten. (Ported when
   `append_line` was deleted in favor of the atomic `write_tombstones`.) Red/green
   proven: reverting to `unwrap_or_default()` turns
   `add_tombstone_propagates_read_errors_other_than_not_found` red.

Phase 2 step 3 — tombstone migration (verified):

1. **The tombstone file is versioned and typed.** v2 is
   `{"schema_version":2,"entries":[{"type":"forget_key"|"record_id","value":"<64hex>"}]}`.
   Old vs new is detected by *appearance*, never by assumption: a file without
   `schema_version` is legacy only when every line is a 64-hex record id, and any
   non-hash legacy line, non-hash entry value, or unknown `schema_version` is
   corruption — an error naming the file, not an empty set that would silently
   drop the forget protections it holds. Matching is type-strict: a `forget_key`
   entry is compared against `record.forget_key`, a `record_id` entry against
   `record.id`, on recall (`search_in`/`blame_in`) and in the rebuild filter.
2. **Translation runs under `AnkaLock` inside `index_in`/`forget_in` only.**
   `migrate_tombstones` reads the old cache, resolves every legacy id to a
   record, re-derives its `forget_key`, and publishes translated tombstones
   *before* the rebuilt index. A tombstone publish that lands without its
   directory fsync (`PublishedUnsynced`) does **not** count as success — the
   error stops the caller before the index publish, and a retry re-publishes
   the versioned file instead of skipping it as already durable (closure fix
   1 below). Recall never migrates or rebuilds as a side effect.
3. **One unresolvable legacy id blocks the whole migration before any write.**
   Old `forget` deleted records from the cache, so tombstones with no matching
   record are expected — and they are exactly why the migration must stop rather
   than "resolve" them by dropping the entry, which would resurrect the record.
   The tombstone file and cache stay byte-identical, and `forget_in` runs the
   same migration first, refusing (writing nothing) while it is blocked.
   Red/green proven: dropping the bail turns
   `an_unresolvable_legacy_tombstone_blocks_the_migration_and_publishes_nothing` red.
4. **Translated entries retain the legacy `record_id` beside the new
   `forget_key`.** The not-yet-replaced cache keeps its old ids, and the old
   cache plus the new tombstones must never expose a record — the retained
   `record_id` entry is what keeps it hidden across the tombstone→index publish
   window; it is inert against the rebuilt index, whose ids differ. A versioned
   file is never re-translated, so retry and re-run are idempotent (tombstone
   bytes identical across runs). Red/green proven: publishing translated keys
   only turns
   `a_crash_between_the_tombstone_and_index_publish_is_safe_and_retries_idempotently` red.
5. **Keys for caches that predate `forget_key` are re-derived from verified
   sources, never guessed.** Claude's pre-identity layout is one record per
   `<slug>/<session>.jsonl` transcript, which is exactly what identity derives
   from today. Prompt-history records stored `"<file-stem>:<line>"` as the
   session and dropped the entry's native `session_id`, so the source line is
   re-read through `HISTORY_LINE_MAX_BYTES`, the redacted prompt must still
   equal the cached content (a rotated or edited file shifts line numbers), and
   only then is the native session recovered — or the file stem when the entry
   has none. Unverifiable (moved line, content mismatch, malformed session)
   blocks migration like any other unresolvable mapping. Red/green proven:
   ignoring the native session turns
   `a_legacy_history_tombstone_recovers_the_native_session_id` red.

Phase 2 step 4 — v2 envelope and coverage (verified):

1. **The envelope is rejected by the old reader structurally.** The cache is
   `{"schema_version":2,"records_v2":[…],"coverage":[…]}` — no legacy top-level
   field name appears in it. The old reader *requires* `records` and
   `indexed_sources`; serde ignores what it does not recognize but cannot invent
   what is missing, so an older binary fails with its own corrupt-index message
   instead of half-accepting the cache. `last_indexed_at` is derived from
   coverage on read (max stamp) rather than stored, so it cannot drift.
   Test: parsing the v1 shape against published bytes must fail.
2. **The new reader branches on the marker and refuses what it does not know.**
   Presence of `schema_version` means a versioned envelope, absence means the
   legacy shape; an unknown version is an error everywhere — recall fails safe,
   `status` reports `incompatible`, and even a rebuild declines to overwrite a
   newer cache. A *corrupt* file stays rebuildable (its message says to run
   `raios anka index`), which is why a full `index_in` never reads the previous
   cache and a scoped one does (it must preserve the other harnesses' records).
   Red/green proven: writing the legacy shape turns
   `an_old_cache_reader_cannot_accept_the_versioned_envelope` red.
3. **Coverage is per harness, fed from that harness's `ImportReport`.** Each
   entry carries `sources`, `records` (recounted from the records actually
   published), `indexed_at`, `full_refresh`, and `oversized` as its own
   exclusion reason — never folded into malformed, because an over-budget line
   is never parsed and its content never materializes. A full run writes an
   entry for every harness (discovered or not); a harness-scoped run replaces
   only its own slice, re-filters carried-forward records through the current
   tombstones and exclusions, and preserves the other entries verbatim — so the
   scoped run stays visible as scoped. Red/green proven: dropping the
   carry-forward turns
   `a_harness_scoped_refresh_preserves_the_other_harnesses_and_their_coverage` red.
4. **`state` is computed from coverage, never from `last_indexed_at` (defect C5
   closed).** `ready` = an entry for every harness; `partial` = a coverage gap
   (a scoped refresh, or a legacy cache whose coverage was reconstructed from
   its records); `empty` = no coverage and no records; `incompatible` = the file
   exists but this build cannot read it. The CLI no longer guesses: both
   `status` and `index` serialize the shared `AnkaIndexStatusDto`
   (contracts), which now carries `state` and `coverage`. Red/green proven:
   reverting to the timestamp guess turns
   `a_legacy_cache_reports_partial_status_with_reconstructed_coverage` red.

Phase 2 closure — three corrections (verified):

1. **An unconfirmed tombstone publish is an error, not success.**
   `write_tombstones`/`add_tombstone` propagate `PublishedUnsynced` like any
   other publish failure: the entries are visible, but visibility is what the
   caller stops over. `index_in`/`forget_in` therefore never reach the index
   publish on that path — records are hidden by the tombstone's *durability*,
   not its visibility, and a crash that reverts an unconfirmed tombstone would
   resurrect exactly what the index publish was about to strip. The retry does
   not skip the missing sync either: `migrate_tombstones` re-publishes an
   already-versioned file (byte-identical, never re-translated), re-running the
   file+directory fsync chain until it lands. The index-side publish may still
   report `PublishedUnsynced` as success — the tombstones are durable by then
   and the retained entries hide a reverted index either way. Red/green
   proven: restoring the `Ok(())` conversion turns
   `a_tombstone_publish_that_lands_without_confirmation_is_reported` and
   `an_unconfirmed_tombstone_publish_stops_the_index_and_a_retry_confirms_durability`
   red; restoring the versioned-file skip turns the latter red on its inode
   assertion (the retry must re-publish).
2. **Migration verifies the event identity, not just the prompt.** A rotation
   can leave another session's event at the line the old cache recorded — same
   prompt text, different event. Text equality cannot tell those apart, and a
   key derived from the wrong event would silently stop hiding the record after
   the next rebuild. `recover_from_line` therefore also compares the line's
   event time against the cache's `occurred_at`: Codex entries must carry `ts`
   *and* match (no `ts` = undistinguishable = stop). OpenCode and antigravity
   verify a present `timestamp`; when it is absent (legacy caches may predate
   timestamped entries, where the old importer fell back to the file's mtime —
   mtime itself is never event evidence), the line may stand for the cached
   event only when it claims nothing event-specific: without a native
   `session_id` the derived key is the fixed file-stem namespace plus the
   prompt group, identical for every line in the file, so no wrong event can be
   named and the mtime-fallback record migrates instead of being blocked; with
   a native `session_id` the key would name a session the line cannot be tied
   back to the cached event, so the migration stops. Unverifiable matches stop
   with nothing published. Red/green proven: reverting to text-only
   verification turns
   `a_legacy_migration_stops_when_the_same_prompt_sits_under_another_event` red;
   dropping the native-session guard turns
   `a_legacy_migration_stops_when_a_native_session_cannot_be_tied_to_an_event`
   red; requiring the timestamp outright turns
   `a_legacy_tombstone_migrates_through_the_old_cache_record` red.
3. **The schema gate refuses unreadable caches and invalid version types.**
   `check_cache_schema` passes only `NotFound` as "nothing to lose": an
   existing cache that cannot be read (permissions, I/O) stops the rebuild
   instead of being mistaken for an absent one and silently overwritten. A
   `schema_version` that is present but not a number is refused with its own
   message — in the gate *and* in `read_index` (recall fails safe, `status`
   reports `incompatible`) — never as "corrupt, rebuild it", because that
   message would invite overwriting a file this build cannot even interpret.
   Genuinely corrupt bytes (no parseable version field) stay rebuildable.
   Red/green proven: restoring the swallow-everything read turns
   `an_unreadable_cache_blocks_the_rebuild_instead_of_being_replaced` red;
   restoring the fall-through or the corrupt-index message turns
   `an_invalid_schema_version_type_is_refused_described_and_never_overwritten`
   red. `a_corrupt_index_stays_rebuildable` pins the behavior that must not
   regress.

Phase 3 — privacy policy, recall exclusions, breakdown (verified):

1. **Consent is explicit, versioned, and refuse-if-exists.** `policy-init` writes
   `anka-policy` as `# anka-policy v1` + `home = keep|exclude` + the normalized
   `home_path` the choice was made for — the typed HOME comparison and the
   encoded Claude slug derive from the consent itself, not from whatever
   environment reads it later. The file is created with `create_new` +
   `mode(0600)` (refusal is atomic: existence *is* the consent), fsync'd file
   and directory; a failed write removes our own partial file so the next init
   can retry. A second init refuses with an error and leaves the original
   untouched. `--home` is mandatory (clap `ValueEnum`): missing flag =
   `MissingRequiredArgument` (exit 2), invented value = `InvalidValue` — no
   default, no inference. `init` touches nothing else: `anka-exclude` and
   `anka-tombstones` stay byte-identical, no cache is created, no timer, no
   indexing. Tests: `policy_init_writes_a_versioned_consent_and_refuses_to_overwrite_it`,
   `policy_init_creates_nothing_but_the_policy_file`, three parse tests in
   `raios-surface-cli`.
2. **`load` fails closed; `status` reports the state instead of failing.**
   Missing file → "run `raios anka policy-init …` first"; wrong header,
   unknown/duplicate/missing keys → a `malformed:` message naming the file.
   There is no empty-allow path (`try_load`'s `Ok(None)` exists only to let
   `status` and `show` describe the gap). `index_in`, `search_in` (and through
   it `blame_in` and MCP `anka_recall`), and `forget_in` all load the policy
   before the lock and before any write, so nothing — not even tombstone
   migration — runs without consent. `AnkaIndexStatus.policy`
   (`AnkaPolicySummaryDto`, `#[serde(default)]` for older payloads) carries
   `initialized`/`home`/`exclude_rules` on every status. Red/green proven:
   turning NotFound into an empty policy makes
   `recall_index_and_forget_refuse_to_run_without_an_initialized_policy` red;
   swallowing the parse error makes
   `a_malformed_policy_is_refused_rather_than_fallback` red.
3. **Policy is evaluated at recall time, not only at index time (defect C3
   closed).** The same `admit` predicate runs in the `search_in` read path, so
   a rule written after the index lands takes effect on the very next query;
   the index itself is untouched until the next rebuild, and the rebuild
   applies the same predicate to carried-forward records. Red/green proven:
   removing the recall-side filter makes
   `exclusion_rules_take_effect_at_recall_before_any_rebuild` red.
4. **One admission predicate, precedence substring → HOME → unknown.** Any
   match excludes; the precedence only picks the reported reason, so the
   breakdown counts every record under exactly one bucket. Substrings are
   case-insensitive literal text — never glob or regex (`keeper*` does not
   match `/data/keeper`). Unknown provenance is excluded always, every
   harness, independent of the HOME choice, at index *and* at recall. The
   typed HOME rule matches the exact `home_path` or the exact encoded slug
   (`[A-Za-z0-9]` kept, every other character one `-`), so `/home/alaz` drops
   neither `/home/alaz/child` nor a different slug sharing its prefix; a slug
   that says HOME while the resolved path says otherwise (a session that
   started under HOME and moved) is rejected regardless of the HOME choice.
   Red/green proven (four reverts): prefix-instead-of-exact makes
   `home_exclude_drops_the_exact_home_record_but_keeps_child_projects` red;
   dropping the unknown check makes
   `unknown_provenance_is_excluded_by_default_at_index_and_recall` red;
   case-sensitive matching makes
   `substring_rules_are_case_insensitive_and_literal` red; removing the
   conflict check or the slug-form match makes the `home_keep…` / `…encoded_home_slug`
   tests red.
5. **HOME retention never leaks into project-filtered recall.** With
   `home = keep`, home records stay recallable without a filter but
   `is_home_label` records never satisfy `--project` — an unscoped label is
   not a project. Red/green proven: dropping the suppression from the
   project-filter closure makes
   `home_keep_retains_unscoped_records_but_never_matches_a_project_filter` red.
6. **`policy-show` is the diagnostic surface: rules, tombstones, and the
   kept/excluded breakdown of the current cache.** Sorted resolved
   `anka-exclude` rules, the `anka-tombstones` entry count, and — evaluated
   read-only against the cache as it exists *now* — kept vs excluded with the
   reason counts (substring/home/unknown_provenance/tombstone) plus a
   per-harness split and a cache state of `absent|ok|unreadable`. A rule
   written but not yet rebuilt shows its losses here before any publication;
   an uninitialized policy reports `initialized:false` with `cache: null`
   instead of erroring (recall keeps erroring). Tests:
   `policy_show_reports_rules_tombstones_and_the_kept_excluded_breakdown`,
   `policy_show_reports_the_missing_state_without_failing`.
7. **Fixture honesty.** The default fixture line now carries
   `"project":"/srv/anka-fixture"`: under the default policy a metadata-less
   line is excluded, which would make every "the record exists" assertion
   vacuously true. The scoped-refresh test uses antigravity instead of codex
   for the second harness, because a codex line without session metadata has
   unknown provenance and is excluded by default (codex-fixture indirectness
   was accepted in review; tombstone translation carries the migration
   proofs). 12 new runtime tests → ANKA 81/81, CLI 67/67;
   `cargo fmt --check` and `cargo clippy --workspace --all-targets` clean.
   The env-dependent `control_plane::snapshot_generation_on_in_memory_db`
   test fails on this machine (live config sets `factory.enabled = true`) —
   separate tracked finding, not fixed, unrelated to this diff.
8. **Substring rules match every project claim, not only the display.**
   Correction after review: Claude records put the directory slug in
   `source.project` while the trustworthy directory exists only in the
   provenance, so an exclusion naming `/srv/private` matched nothing. `admit`
   now matches the display project, `Provenance.path` (new `#[serde(default)]`
   field carrying the normalized asserted path), the raw slug, and the legacy
   in-scope path. Red/green: dropping the provenance candidates makes
   `substring_rules_match_the_resolved_path_and_the_raw_slug_not_only_the_display`
   red.
9. **The HOME decision follows the consent's `home_path`, not the
   import-time label.** `is_home_label` compares the record's resolved path to
   the consented `home_path` (and the slug to `home_slug`); the
   `ProjectScope::HomeUnscoped` label — computed against whatever `$HOME` the
   indexing process saw — no longer decides, so moving the process home cannot
   stop excluding the home the consent names. Records without the path field
   (pre-correction caches) fall back to their scope, conservatively either
   way. `normalize_path` collapses interior runs of `/` (POSIX: `/home//alaz`
   *is* `/home/alaz`) so a double-slashed spelling cannot dodge the exact
   comparison, and `parse_policy` rejects any `home_path` that is not absolute
   and already normalized. Project-filter suppression is its own predicate
   (`suppressed_from_project_filter`): a home label never satisfies `--project`
   even when the consent names a different home. Red/green (three reverts):
   `the_home_decision_follows_the_consent_path_not_the_process_home`,
   `an_asserted_home_with_interior_double_slashes_is_normalized_before_the_home_comparison`,
   `a_policy_home_path_that_is_not_already_normalized_is_malformed`.
10. **An uninspectable cache is `unreadable`, not `absent`.** A `try_exists`
    failure now reports `state:"unreadable"` with the I/O detail; only a
    definitive "does not exist" is `absent`, because `absent` with zero
    records would claim a total loss that never happened. Red/green:
    restoring `unwrap_or(false)` makes
    `policy_show_reports_an_unreadable_cache_as_unreadable_not_as_absent` red.
11. **`policy-show` adds a pre-publication count.** The cache breakdown can
    only measure records the cache holds; a record the policy filtered at its
    first index left no trace there. `discovery` re-runs discovery read-only
    and applies the same tombstones + admission predicate, reporting
    `{state: ok|error, detail, discovered, kept, excluded, per_harness}` —
    what the *next* rebuild would publish and drop, countable before it runs.
    Both views share one `count_records` so any difference between them is a
    difference in inputs, never in rules. Red/green: removing the section makes
    `policy_show_counts_discovery_against_the_policy_before_publication` red.
