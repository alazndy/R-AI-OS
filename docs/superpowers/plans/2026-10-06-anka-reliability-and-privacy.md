# ANKA Reliability and Privacy Plan

Date: 2026-10-06
Owner: Codex Kaira
Status: Proposed implementation plan; implementation and deployment have not started.
Revision: 2 — incorporates provenance/coverage review and bounded header measurements.

## Objective and scope

Repair OpenCode prompt parsing and Claude subagent discovery, make exclusions and
forget durable across rebuilds, and enable a daily local rebuild only after the
privacy and concurrency checks pass.

This iteration covers existing JSONL sources plus bounded Codex rollout-header
reads for provenance only. Codex rollout body ingestion and the OpenCode SQLite
database remain separate follow-up work. Semantic search, automatic context
injection, and promotion into curated memory remain outside this scope.

## Verified baseline

The existing cache reports 2,315 records from 61 sources: 2,170 Codex, 87
Antigravity, 58 Claude, and zero OpenCode records. These are an existing-cache
snapshot, not acceptance targets for the new implementation.

- OpenCode prompt history has 50 valid lines, all using `text`; none of the
  observed lines has project or timestamp metadata. The parser expects `input`.
- Claude discovery stops at depth 3. The current source tree has 60 JSONL files
  at depth 2 and 66 at depth 4. File counts can change during active sessions;
  empty or unsupported files need not yield a record.
- Claude project attribution uses the immediate parent directory. Increasing
  depth alone would attribute nested agent records to `subagents`.
- Exclusions currently match only the stored project string. Generic history
  labels cannot prove which project a prompt belongs to.
- Record hashes include timestamp and content. For records without a timestamp,
  file mtime is substituted; appending to history therefore changes older IDs.
- A harness-specific rebuild currently replaces the entire cache. This must be
  corrected before using per-harness rebuilds for verification.
- `raios cron add` schedules an agent prompt, not a direct shell command.
- The repository has unrelated uncommitted work. SigMap's import impact output
  reports no edges for `anka.rs`, but explicit CLI and MCP call sites exist;
  import-graph output alone is insufficient to determine the change boundary.
- Revision-2 measurement: 2,173 Codex history rows across 162 sessions; 187
  rollout headers with 187 valid metadata IDs and no conflicting cwd values.
  Joining history `session_id` to metadata `payload.id` resolves 159 sessions
  and 2,167 rows; six rows remain unresolved. The number of unresolved sessions
  must not be confused with the number of unresolved prompt rows.
- Of those resolved Codex rows, 1,432 have cwd exactly equal to the user's home
  directory and 735 have another cwd. Header provenance alone therefore does
  not settle the recall/privacy trade-off.
- Every one of the ten current Claude project directory names matches the
  literal substring `-home-alaz`; only one equals it. Adding that substring to
  the existing exclusions would remove every current Claude project.
- Review reports zero matches for the four previously suggested exclusions.
  Treat their presence as future protection, not cleanup of current exposure.

## Proposed decisions

1. Treat importer correctness, provenance, privacy filtering, persistence, and
   scheduling as separate responsibilities within the existing ANKA module.
2. Keep public recall contracts compatible where possible. Cache-only metadata
   can add schema version, source identity, and provenance quality. New status
   fields must be additive and reviewed at the CLI/DTO boundary.
3. Separate provenance from scope and retention. Classify records as `project`,
   `home_unscoped`, or `unknown`; a known home cwd is not an unknown path or a
   proven project. The proposed default excludes truly unknown provenance.
   HOME retention is an explicit owner decision, pending in this revision.
   Never infer ownership from prompt content.
4. Apply privacy policy both before cache publication and during recall, so
   editing exclusions immediately prevents old cache entries from being returned.
5. Preserve existing exclusions and tombstones during every migration, rebuild,
   installation, and rollback. Missing files are not evidence of an existing
   documentation violation; requiring initialized privacy policy is a deliberate
   new behavior.
6. Use a systemd user service and timer for direct daily indexing. Enable it only
   after a manual rebuild succeeds with the approved policy and tested binary.
7. Project-filtered recall never returns `home_unscoped` or `unknown` records.
   If HOME retention is enabled, global recall may return them with explicit
   scope/provenance metadata. Exposing that label is required in CLI and MCP
   results; an internal-only label would mislead callers.

## Coverage decision and rollout gate

| Codex classification | Measured history rows | Proposed handling |
| --- | --- | --- |
| Session header cwd other than HOME | 735 | Retain unless exclusions or tombstones match; attribution is session-level |
| Session header cwd exactly HOME | 1,432 | Owner choice: labeled global recall or exact HOME exclusion |
| No matching/usable session header | 6 | Exclude by the proposed unknown-provenance default |

OpenCode's 50 current prompts still lack project evidence. This iteration fixes
their parser but does not claim 50 additional searchable records: under the
proposed unknown-provenance default they remain excluded. Retaining these prompts
requires a separate explicit unknown-history opt-in or a later metadata source;
neither is silently bundled into HOME retention.

Before production rebuild, record the chosen HOME/unknown policy and a per-harness
dry-run coverage ledger: discovered, parsed, retained, excluded by rule, HOME,
unknown, tombstoned, and invalid. Every accepted loss must have a named reason.
Do not enable a policy that silently turns a 2,315-record cache into roughly 213
records. Fixture acceptance must protect eligible records as well as exclude
protected records. Live counts are refreshed at rollout and need not equal this
planning snapshot.

## Phase 0 — Isolate and specify

- Capture the current Git state and check task ownership before implementation.
  Do not reset, stash, commit, or install unrelated pending work.
- Implement from an isolated checkout of an agreed base revision. Verify ANKA
  files against the live tree before transferring the final patch.
- Record the installed binary path/version and cache schema separately from
  source revision; testing source does not update the installed CLI.
- Run `raios security` and `raios deps` before security-sensitive edits. Review
  scanner findings against the affected paths rather than trusting them blindly.
- Review signatures/contracts first: explicit source/config/cache roots for
  tests, project provenance, stable identity, policy evaluation, cache migration,
  and the lock protecting index/forget operations.
- Include `resolve_codex_provenance()` in the structural review. Specify the
  authoritative join key, bounded read limit, provenance enum, HOME classification,
  duplicate-header conflict behavior, and additive CLI/MCP scope exposure.
- Resolve task ownership through the available control-plane inbox. A clean
  isolated checkout protects files but does not supersede another task's owner.

Acceptance: an approved structural design and file boundary; no implementation
against another agent's active task; existing source transcripts untouched.

## Phase 1 — Importer and provenance correctness

### Codex provenance without rollout body ingestion

- Build a session-to-cwd map by reading only the first line of each discovered
  rollout under supported active/archive roots. Set a strict byte bound (initial
  proposal: 256 KiB per header); never read the rest of a file to recover a header.
- Require `type == session_meta`, an object payload, a valid string `payload.id`,
  and an absolute cwd. Join history `session_id` to `payload.id`. Treat
  `payload.session_id` only as a documented legacy alias verified by a fixture;
  reject conflicting aliases. Never make filename extraction authoritative.
- Use filename-derived IDs only as diagnostics for disagreement or missing
  metadata. Missing/malformed/oversized headers leave provenance unknown.
- Deduplicate identical ID/cwd pairs; conflicting cwd values for the same ID
  make that session ambiguous and excluded by default. Discovery order must not
  determine attribution. Capture a consistent map for each rebuild.
- Label provenance as `session_cwd`, independent of lexical/exact search score.
  This proves startup cwd, not prompt-level project ownership: an in-session
  directory change remains an explicit attribution limitation. Do not use this
  metadata as a security ownership/authorization boundary.
- Classify exact HOME cwd as `home_unscoped`. Normalization must not read or
  modify rollout bodies or follow transcript symlinks outside supported roots.
- Preserve raw session identity independently of derived cwd so improved
  attribution does not change durable forget identity.

### OpenCode

- Prefer a nonempty string `text`; accept legacy `input` as a fallback.
- Validate optional project and timestamp values. Preserve a valid source
  timestamp; otherwise distinguish filesystem observation time from event time.
- Mark absent project metadata as unknown. Do not label a prompt as scoped just
  because a fallback display string exists.
- Stream JSONL lines and count malformed, empty, and unsupported entries without
  logging prompt contents.

### Claude

- Discover normal sessions and documented `session/subagents/agent-*.jsonl`
  paths through depth 4; avoid following symlinks outside the source root.
- Resolve the containing project directory relative to `.claude/projects`,
  rather than using the JSONL file's immediate parent.
- Prefer trustworthy `cwd` metadata, with parent-session provenance for
  subagents. Treat the encoded directory slug as a conservative fallback;
  do not reconstruct an exact filesystem path from its lossy encoding.
- Include project, parent session, and agent identity in source identity so
  equal filenames in different locations do not collide.
- Preserve the existing text extraction limits and document them; discovering
  a transcript is not equivalent to capturing all reasoning or tool output.

Acceptance: Codex fixtures resolve by metadata ID without reading any rollout
body; valid eligible rows survive filtering. All 50 current OpenCode prompts are
parseable before policy filtering; their expected retained count is explicitly
zero under the proposed unknown-provenance default. Nested Claude fixtures retain
their owning project and parent session. The coverage ledger accounts for every
retained/skipped record rather than forcing a numeric growth target.

## Phase 2 — Stable forget and cache migration

### Identity semantics

- Claude forget hides a transcript source identified by harness, containing
  project, parent session, and optional agent ID. Appending conversation text,
  changing mtime, or editing content must not revive it.
- History records with native immutable IDs use those IDs, namespaced by harness
  and source. A Codex `session_id` identifies a session, not an individual prompt;
  never deduplicate or forget all its prompts accidentally by treating it as a
  prompt ID. Use session identity, a native immutable event timestamp if present,
  and a redacted prompt fingerprint for prompt identity. Do not use line number,
  derived project/cwd, or file mtime as durable identity.
- For prompt history without native IDs, use a versioned fingerprint of the
  redacted prompt within its source namespace. Identical redacted prompts share
  a forget identity; this conservative behavior must be explicit in the docs.
- Keep observed timestamps and content out of native/source-based identity.

### Migration

- Introduce a versioned cache format and recognize the current unversioned cache.
- Before rebuilding, translate old tombstones through their matching old-cache
  records into new forget identities. Write the translated tombstones durably
  before publishing the replacement index.
- If an old tombstone cannot be resolved because its record was already removed
  or the old cache is unavailable, stop migration and report the ambiguity.
  Do not silently discard it. Offer explicit source-wide exclusion as a
  conservative resolution; user approval is required to discard old protections.
- Write owner-only files with private permissions at creation, use unique
  temporary filenames, and publish the cache atomically. Avoid a permissive
  temporary-file window followed by chmod.
- Serialize index and forget under the same interprocess lock. A crash or busy
  lock must leave the last valid cache and all durable tombstones intact.
- A harness-specific rebuild replaces only that harness's records and preserves
  the others. Store per-harness coverage/timing internally so combined status
  does not misrepresent a partial refresh as a full refresh.
- Incompatible-cache recall must fail safely. Recall must never perform migration
  or rebuild as a side effect, including through MCP.

Acceptance: forgotten records stay hidden after append, mtime changes, history
rotation/reordering, restart, full rebuild, and harness-specific rebuild. Old
unresolvable tombstones are surfaced, never silently lost.

## Phase 3 — Privacy policy and existing-cache cleanup

- Initialize policy explicitly; use the existing `anka-exclude` file for
  case-insensitive literal substring rules. Document that these are not glob or
  regex patterns. Keep tombstone initialization separate from policy consent.
- Preserve existing rules. The four suggested names `Secrets_and_Configs`,
  `Personal`, `Backups`, and `Education` currently have no reported matches;
  list them only as optional future protection, not a remedy for current HOME
  exposure. Evaluate candidate rules against the coverage ledger before writing.
- Handle HOME with a dedicated typed policy, not a substring exclusion. Compare
  normalized cwd to the exact user home path; for Claude's fallback slug compare
  the containing project directory name to the exact encoded HOME slug. Do not
  add `-home-alaz` or `/home/alaz` as broad substring rules: both can remove child
  projects unintentionally. Apply HOME behavior consistently across harnesses.
- Match both trustworthy project metadata and the containing project slug for
  Claude. Reject conflicting/ambiguous provenance conservatively.
- Exclude truly unknown provenance by the proposed default across every harness.
  HOME retention and any unknown-history opt-in are independent policy choices.
  If HOME is retained, make its unscoped label visible and exclude it from
  project-filtered recall. Do not claim directory exclusions protect arbitrary
  prompt content in HOME or a session that changed cwd after startup.
- Read policy and tombstones on recall as well as rebuild. Missing, malformed,
  or unreadable initialized policy must produce a safe error, not unrestricted
  fallback. An explicitly empty, initialized policy is a distinct valid choice.
- Keep redaction before every persistent content write; never put transcript
  text, snippets, credentials, or personal project names in operational logs.
- Replace the currently broader cache through a policy-filtered rebuild. Do not
  retain an unfiltered backup as part of routine rollout. Source transcripts are
  never removed or edited.

Acceptance: excluded fixtures and unknown-project fixtures under the default
policy cannot be returned by CLI search, blame, or MCP recall, including before
the first rebuild after a policy change. Eligible known-project fixtures remain
searchable. HOME fixtures follow the explicitly selected retention policy;
unscoped records never match a project filter. The coverage ledger documents
losses before publication. All cache/config files are 600 and private directories
are 700 on Unix.

## Phase 4 — Verification and manual rollout

Use temporary source/config/cache roots with synthetic transcripts. Never let
tests read real history or change the production `workspace.db`.

| Scenario | Required result |
| --- | --- |
| Current/legacy OpenCode shapes; malformed and empty lines | Supported records parsed; unsupported entries counted; no panic |
| Codex header ID versus filename; aliases and conflicting duplicate IDs | Metadata is authoritative; conflict/invalid headers do not misattribute rows |
| Oversized/missing header; large rollout body containing sentinel data | Reads stop at the bounded first line; body is neither read nor cached |
| Several prompts in one Codex session; repeated prompts with different native timestamps | Distinct events survive; attribution changes do not invalidate forget |
| Claude nested agent; duplicate agent filenames across sessions | Correct project/parent attribution; distinct source identities |
| Mixed case exclusions; unknown or conflicting project | Deterministic exclusion; no scope inferred from prompt text |
| Exact HOME versus HOME child project across harnesses | HOME policy affects only the root scope; child projects remain eligible |
| Retained HOME queried globally versus with a project filter | Visible unscoped label globally; no match for project-filtered recall |
| Complete coverage ledger under both HOME choices | Every retained/lost row explained; eligible records are not silently removed |
| Policy change with stale cache | Search, blame, and MCP stop returning protected records immediately |
| Forget followed by append, mtime touch, rotation, or restart | Forgotten record remains hidden |
| Legacy cache with matched and unmatched tombstones | Safe migration or explicit refusal; no loss of protection |
| Partial rebuild | Other harness records and coverage remain intact |
| Index racing forget; two index processes | No lost tombstones; no corrupt or partly written index |
| Failure during migration/publication; malformed cache | Safe error; previous valid cache remains readable where compatible |
| Permissions and redaction | No permissive temporary files; no raw secrets in cache or logs |
| MCP contract | Bounded output; historical-evidence wrapper; no write side effects |

Run focused ANKA tests, then affected CLI/MCP checks and Rust formatting/clippy
for the changed crates. Broaden to workspace tests only if shared contracts or
common infrastructure changed. Run the repository's required pre-commit checks
and regenerate architecture/docs before any eventual commit.

Build the tested binary from the isolated implementation revision. Perform one
manual full rebuild only after the migration and policy preflight succeeds.
Measure elapsed time, peak memory, cache size, and per-harness accepted/skipped
counts. Require no unexpected source growth and no unexpected skipped-source
errors; record the baseline before choosing operational limits.

Acceptance: focused regressions pass; live policy and permissions verified;
installed binary demonstrably contains the tested ANKA changes.

## Phase 5 — Daily refresh

- Deliver a user service and timer alongside the implementation, then install
  and enable them during the approved operational rollout.
- Use an absolute path to the tested binary and direct `anka index`; no agent,
  LLM call, or agent quota is required.
- Set a daily calendar schedule in the user's local timezone, `Persistent=true`,
  a small randomized delay, and `UMask=0077`. Document that a user timer runs
  while the user manager is active; enabling linger is a separate system choice.
- Reuse the ANKA lock; overlapping manual/automatic rebuilds cannot interleave.
- Verify initial policy/migration readiness before activation. A privacy or
  migration error keeps the timer disabled; periodic job failures must be visible
  through service status/journal with content-free diagnostics.
- Set resource/time limits from Phase 4 measurements. Verify unit syntax and
  one manual service invocation before checking the next scheduled activation.
- Rollback stops/disables the timer and retains policy/tombstones. Never revert
  to a binary that ignores the new privacy/schema protections or restore an
  unfiltered old cache. An incompatible rollback leaves recall disabled.

Acceptance: the unit points to the tested binary, a service invocation completes
successfully, the next activation is scheduled, and failure/disable behavior is
verified without waiting a day.

## Progress

### Phase 1 — Importer/provenance correctness (complete)

- Module split: `anka/{mod, importers, provenance, lock, publish}.rs`.
- Codex header resolver, depth-4 Claude walk, symlink rejection, `archived_sessions`,
  `ImportReport`, bounded reads (header 256 KiB, Claude line 1 MiB, history 256 KiB).
- `Oversized { line_complete }` boundary contract; `ImportReport::oversized` as a
  separate exclusion reason.
- 40/40 ANKA tests green; full lib suite 402/403 (only the env-dependent
  `control_plane` test fails — separate finding).

### Phase 2 step 2 — Lock and publish (complete)

- `AnkaLock` via `std::fs::File::lock` (stable 1.89+, no `libc`); `try_acquire`
  returns `Ok(None)` for busy.
- `publish.rs`: `create_owner_only` with `mode(0o600)` at `open(2)`, unique
  `pid+counter` temps with `O_EXCL`, fsync file → rename → fsync dir.
- `index_in`/`forget_in` hold the lock across their read-modify-publish sections.
- Red/green proven for both lock tests.

### Phase 2 step 3 — Publish phases, recall-side tombstones, and `forget_key` (complete)

- `PublishError` distinguishes `NotPublished` (pre-rename; old file intact) from
  `PublishedUnsynced` (post-rename; new content visible, durability unconfirmed).
- `atomic_write` no longer creates/hardens directories — callers do it on entry.
- `search_in` reads index first, tombstones second; `blame_in` routes through the same
  path; MCP recall inherits the check.
- Acceptance: durable tombstone + failed index publish ⇒ `search` and `blame` return
  nothing. Red/green proven.
- A durable tombstone is never rolled back, even when the index publish fails.
- `record_id` (volatile, includes discriminator + timestamp) and `forget_key` (durable,
  from `SourceIdentity`) are separate handles. `forget` accepts `record_id`, writes
  `forget_key` to tombstone.
- Prompt-history records carry a redacted-prompt fingerprint in `forget_key`; Claude
  transcripts identify by source alone.
- History identity uses a stable source namespace (`identity_project` = history file
  stem), not the resolved display project. Provenance transitions do not change
  `forget_key`. Red/green proven.
- The rebuild filter (`index_in`) also uses `forget_key`. Red/green proven.
- `add_tombstone` propagates read errors other than `NotFound` (ported when
  `append_line` was deleted). Red/green proven.
- Recall filters by `forget_key`. Red/green proven.

### Phase 2 step 3 (remaining) — tombstone migration (complete)

- v2 tombstone file: `{"schema_version":2,"entries":[{"type","value"}]}`; legacy
  detected by 64-hex line appearance only; non-hash/unknown-version ⇒ corrupt error
  (never an empty set). Type-strict matching on recall and in the rebuild filter.
- `migrate_tombstones` runs under `AnkaLock` inside `index_in`/`forget_in` only;
  recall never migrates. Translated tombstones publish first, rebuilt index second;
  `PublishedUnsynced` counts as success.
- One unresolvable legacy id blocks everything before any write; `forget_in`
  migrates first and refuses (writes nothing) while blocked.
- Translation retains the legacy `record_id` entry beside the new `forget_key`, so
  old cache + new tombstones never expose a record across the publish window;
  versioned files are never re-translated (idempotent retry/re-run).
- Pre-identity keys re-derived from verified sources: Claude's one-transcript
  layout; history via source-line re-read (256 KiB bound, redaction equality,
  native `session_id` recovery, stem fallback). Unverifiable ⇒ blocked.
- 5 migration tests over real old JSON shapes; red/green proven for the blocked,
  native-session, and retention paths.

### Phase 2 step 4 — v2 envelope and coverage (complete)

- Cache envelope `{"schema_version":2,"records_v2":[…],"coverage":[…]}` — no legacy
  top-level field names, so the old reader (requires `records`/`indexed_sources`)
  fails structurally; `last_indexed_at` derived from coverage on read.
- New reader branches on the marker: unknown schema version refuses everywhere
  (recall fails safe, `status` = `incompatible`, rebuild declines to overwrite);
  corrupt stays rebuildable, which is why a full `index_in` never reads the
  previous cache and a scoped one does.
- `AnkaIndex.coverage` per harness from that harness's `ImportReport`, incl.
  `oversized` as its own exclusion reason; scoped refresh preserves the other
  harnesses' records (re-filtered through current tombstones/exclusions) and their
  coverage entries verbatim (`full_refresh` stays visible as scoped).
- `state` derived from coverage (ready/partial/empty/incompatible), never from
  `last_indexed_at` — defect C5 closed. CLI `status`/`index` serialize the shared
  `AnkaIndexStatusDto` (contracts) carrying `state` + `coverage`; MCP recall already
  spoke the shared types.
- 6 step-4 tests; red/green proven for state, envelope, and preservation paths.
- Workspace suite: ANKA 62/62, full lib 425/425 this run (the env-dependent
  `control_plane` test passed here — still a separate tracked finding, not fixed),
  `cargo fmt --check` clean, `cargo clippy --workspace --all-targets` clean.

### Phase 2 closure — three corrections (complete)

- `write_tombstones`/`add_tombstone` propagate `PublishedUnsynced` instead of
  counting it as success: the index publish is never reached on an unconfirmed
  tombstone (records hide by durability, not visibility), and
  `migrate_tombstones` re-publishes an already-versioned file — byte-identical,
  never re-translated — so a retry re-runs the fsync chain rather than skipping
  it as "already durable".
- Migration verifies the event identity against the cache's `occurred_at`, not
  just the redacted prompt: Codex requires a matching `ts` (absent = stop);
  opencode/antigravity verify a present `timestamp`, and when it is absent the
  line must also lack a native `session_id` — the derived key is then the fixed
  file-stem namespace + prompt group, identical for every line in the file, so
  no wrong event can be named and the mtime-fallback record migrates; a native
  session with no timestamp cannot be tied to the cached event (stop). mtime is
  never event evidence. Same prompt under another session/timestamp stops the
  migration with nothing published.
- `check_cache_schema` passes only `NotFound`: an unreadable cache stops the
  rebuild instead of being overwritten. A non-numeric `schema_version` is
  refused with its own message in the gate *and* in `read_index` (never as
  "corrupt, rebuild it"); genuinely corrupt bytes stay rebuildable.
- 7 new tests; red/green proven for all seven revert paths (error swallowed,
  versioned skip, text-only recovery, I/O swallowed, type fall-through, wrong
  refusal message, native-session guard dropped / timestamp made mandatory).
- Workspace suite: ANKA 69/69, full lib 432/432, workspace 1174/0,
  `cargo fmt --check` clean, `cargo clippy --workspace --all-targets` clean.

### Phase 3 — Privacy policy, recall exclusions, kept/excluded breakdown (complete)

- New module `crates/raios-runtime/src/anka/policy.rs`. Consent file
  `anka-policy` = `# anka-policy v1` header + `home = keep|exclude` + `home_path`
  recorded at init (the typed HOME comparison and the encoded Claude slug derive
  from the consent itself, not from a later environment). `init` is
  refuse-if-exists (`create_new`, owner-only from instant of existence, fsync
  file + directory) and touches nothing else: no cache, no indexing, no timer;
  `anka-exclude`/`anka-tombstones` stay byte-identical. `--home` has no default
  (clap `ValueEnum`; missing flag = usage error, invented values = invalid value).
- `AnkaPolicy::load` fails closed: missing → "run `policy-init` first", and
  malformed/unreadable files (wrong header, unknown key, duplicate key, missing
  key) are refused as malformed — never an empty allow. `try_load` distinguishes
  NotFound (`Ok(None)`) from real failures. Loaded at the top of `index_in`,
  `search_in` (so `blame_in` and MCP `anka_recall` inherit it), and `forget_in` —
  before the lock and before any write.
- One admission predicate shared by index and recall, precedence
  substring → HOME → unknown (precedence only picks the reported reason, any
  match excludes): case-insensitive literal substrings from `anka-exclude`;
  the typed HOME rule matching the exact `home_path`, the exact encoded home
  slug (`[A-Za-z0-9]` kept, everything else `-`), with slug/path conflicts
  rejected regardless of the HOME choice; unknown provenance excluded always,
  every harness, independent of HOME.
- HOME=keep: unscoped records stay recallable unfiltered but `is_home_label`
  records never satisfy a `--project` filter. Rule changes take effect on the
  very next query — no rebuild required — and the rebuild filter applies the
  same predicate to carried-forward records.
- `AnkaIndexStatus` carries `AnkaPolicySummary { initialized, home,
  exclude_rules }` (core type; `AnkaPolicySummaryDto` with `#[serde(default)]`
  for older payloads). `status` never fails on policy — it reports the state;
  `policy-show` is the diagnostic surface: sorted resolved rules, tombstone
  count, and the kept/excluded breakdown of the *current* cache (reason counts
  substring/home/unknown_provenance/tombstone, per-harness kept/excluded,
  cache state absent|ok|unreadable) so pending losses are documented before any
  rebuild publishes them; reports `initialized:false` instead of erroring.
- CLI: `raios anka policy-init --home keep|exclude`, `raios anka policy-show`.
- Fixture migration: policy starts as `home = keep`, default history line gains
  `"project":"/srv/anka-fixture"` (the default policy drops a metadata-less line,
  which would make every "the record exists" assertion vacuous), SID-9 migration
  line likewise; the scoped-refresh test switched codex → antigravity because
  codex lines without session metadata have unknown provenance and are excluded
  by default — the codex-fixture indirectness (D6) was accepted in review, with
  tombstone translation carrying the migration proofs.
- 12 new runtime tests (ANKA 81/81) + 3 CLI parse tests (CLI 67/67). Red/green
  proven for all 9 revert paths: recall admit filter, fail-closed load, typed
  HOME exactness, unknown-provenance default, no-fallback on malformed policy,
  project-filter suppression, substring case folding, conflict rejection,
  exact-slug match.
- `cargo fmt --check` clean, `cargo clippy --workspace --all-targets` clean.
  The env-dependent `control_plane::snapshot_generation_on_in_memory_db` test
  fails on this machine because the live `~/.config/raios/config.toml` sets
  `factory.enabled = true` (separate tracked finding, unrelated to this diff,
  not fixed).
- Deferred, not done here: the operator's HOME keep/exclude choice (blocks only
  the live `policy-init` + cache rebuild — code takes it as a mandatory
  argument), TOCTOU `O_NOFOLLOW` on provenance file opens (mandatory before any
  live policy/cache transition, separate commit), systemd timer (final gate
  after manual rollout).

### Phase 3 corrections — review probes closed (complete)

Review with synthetic probes found four defects after the Phase 3 commit; all
four are fixed, each with a regression fixture and a verified red/green revert:

- **P1 — substring rules now evaluate every project claim.** `admit` matches
  `anka-exclude` patterns against the display project, the provenance path
  (`Provenance.path`), the raw harness slug, and the legacy in-scope path —
  Claude records put only the directory slug in `source.project`, so a rule
  naming the real directory (e.g. `/srv/private`) previously matched nothing.
- **P1 — the HOME decision is bound to the consent's recorded `home_path`.**
  `Provenance` gained `#[serde(default)] path: Option<String>` (the normalized
  asserted path, set by `direct`/`session_cwd`); `is_home_label` compares that
  path to `home_path` (and the slug to `home_slug`) instead of trusting the
  import-time `HomeUnscoped` label, which was computed against whatever `$HOME`
  the indexing process saw. Legacy records without the field fall back to their
  scope (conservative either way). Project-filter suppression is a separate
  predicate (`suppressed_from_project_filter`) so an unscoped label can never
  satisfy `--project` even when the consent names a different home.
- **P1 — `normalize_path` collapses interior runs of `/`.** POSIX resolves
  `/home//alaz` as `/home/alaz`, so the double-slashed spelling of the
  consented home must not slip past the exact comparison as a scoped project.
- **P2 — a non-canonical `home_path` in the consent is malformed at load.**
  `parse_policy` requires absolute and already-normalized (rejects
  `relative/path`, trailing `/`, interior `//`) — formatting must not decide
  what HOME means.
- **P2 — an uninspectable cache reports `unreadable`, not `absent`.**
  `try_exists()` errors carry `state:"unreadable"` + detail instead of
  collapsing to a zero-record `absent` claim.
- **Pre-publication count:** `policy-show` now also evaluates freshly
  discovered sources against the same policy + tombstones the next rebuild
  applies (`discovery: {state, detail, discovered, kept, excluded,
  per_harness}`), because the cache breakdown can only measure records the
  cache actually holds — a record the policy filtered at its first index left
  no trace there.
- 7 new tests (ANKA 88/88; CLI unchanged 67/67); all 6 reverts verified RED
  with byte-identical restores; workspace 1195/1196 (sole failure remains the
  known env-dependent `snapshot_generation_on_in_memory_db`).

## Implementation boundary

Primary implementation: `crates/raios-runtime/src/anka.rs` and its tests. Use small
internal helpers; split files only if the reviewed skeleton warrants it.

Conditional surface changes: `crates/raios-core/src/anka.rs`,
`crates/raios-contracts/src/anka.rs`, the ANKA CLI handler/arguments, and MCP
recall tests if additive status/provenance fields require them. The MCP public
tool remains read-only.

Documentation: `docs/ANKA.md`, the ANKA README section, project `memory.md`, and
`SIGMAP.md`. Timer/service artifacts belong under the repository's existing
service-installation convention, to be located before adding a new directory.

## Separate follow-up: Codex rollout bodies and OpenCode SQLite

Header-only Codex provenance belongs to this iteration. Before adding rollout
body ingestion or SQLite transcript import, sample actual schemas and measure the corpus.
Design streaming reads, bounded redacted chunks, incremental updates, provenance,
retention, stable forget identity, and index-size limits. Do not equate a roughly
960 MB source corpus with its eventual cache size; extraction and limits determine
that. Decide explicitly whether reasoning and tool output should be included.
For SQLite, investigate read-only snapshot consistency and avoid copying or
modifying the live database during discovery.

## Recorded instinct and restart note

- Situation: expanding transcript discovery or enabling automatic refresh.
- Signal: generic project labels, mtime/content-based IDs, or incomplete impact
  graphs despite explicit CLI/MCP callers.
- Response: verify provenance and durable forget before increasing coverage;
  test actual entry points and use source-independent fixture roots.
- Expected payoff: broader recall does not revive forgotten data or bypass
  exclusions, and automation does not repeatedly amplify an importer defect.

Restart state: this session revised the plan and verified bounded Codex headers
without reading rollout bodies. No importer, production cache,
privacy file, installed binary, or timer was changed. Next action is structural
review of Phase 0, with HOME retention still requiring the owner's preference,
followed by isolated implementation of Phases 1–3 and their regression tests.
Timer activation is the final operational step. Header-resolution coverage and
policy-retention coverage are separate measurements.
