# Codex handoff health report — 2026-10-09

## Result and scope

The authorized validation and diagnosis are complete. Build, 1,117 tests, strict
Clippy, formatting and crate diff checks pass. The shared database has an
integrity-checked, SHA-256-verified online backup. Database cleanup is proposed
below and requires Göktuğ's approval; no deletion, reindex, index drop, checkpoint,
VACUUM, daemon restart or binary installation was performed in this session.

Incoming task: `55fb638b-e1ad-42ed-9227-861bf96286cc`, assigned to `codex_kaira`.
The durable handoff was read directly with read-only SQLite queries. Its pending
delivery approval was not manually changed or consumed by spawning another agent.
No changes were made to `tools/raios-workspace*`, `docs/superpowers/plans`, README.md
or SIGMAP.md. No repository commit was created.

## Backup

- Command: `raios db backup --json --keep 10`.
- Snapshot: `/home/alaz/.config/raios/backups/workspace-20261009T175836896453780Z-29eac85f19b5.db`.
- SHA-256: `5c99a8d4b7e72d7f76ff0b1b520959dea8153af30acc2515214656188378d0f1`.
- Size: 2,733,383,680 bytes; permissions: `0600`.
- SQLite quick integrity check: `ok`; independent `sha256sum -c`: `OK`.
- Retained snapshots: 1; pruned snapshots: none.
- Backup inspection used `mode=ro&immutable=1` on the completed snapshot, avoiding
  attempts to create WAL companion files beside the protected backup.

## Database diagnosis

The measured snapshot is 2.546 GiB; the configured cap is 2,621,440,000 bytes
(2,500 MiB / 2.441 GiB). The excess is 111,943,680 bytes (106.8 MiB). The CLI's
rounded 2.5/2.4 GB display uses binary units. Only two wholly free pages were
observed before backup. VACUUM can also compact partially filled pages; its
standalone savings were not measured and would not resolve the indexing policy
or stale-root problems.

Live `dbstat` measurements taken during the backup:

| Object | Allocated bytes | MiB |
| --- | ---: | ---: |
| bm25_postings | 1,134,637,056 | 1,082.1 |
| idx_bm25_token | 182,312,960 | 173.9 |
| idx_bm25_postings_file | 141,049,856 | 134.5 |
| trigram_postings | 295,575,552 | 281.9 |
| idx_trigram_file | 352,231,424 | 335.9 |
| idx_trigram | 267,124,736 | 254.8 |
| idx_trigram_postings_file | 245,514,240 | 234.1 |

The snapshot contains 11,356,859 BM25 postings and 18,005,684 trigram postings.
`bm25_postings.snippet` alone stores 849,935,748 bytes (810.6 MiB) of raw text.
`ProjectIndex::index_file` creates one row per token occurrence and repeats the
same up-to-100-character line snippet for each token. This is a structural storage
cost, not evidence that all those rows are stale or duplicate token records.

Filesystem checks found:

| Cache | Missing files | Postings associated with missing files |
| --- | ---: | ---: |
| BM25 | 206 | 268,977 |
| Trigram | 375 | 646,145 |

The missing BM25 postings are about 2.4% of its total. All 375 missing trigram
files are outside `/home/alaz/dev`: `/tmp/env-i-deploy` (303),
`/tmp/kaira-notifications-pr` (32), `/tmp/raios-handoff-fix` (40).
Reindexing only `/home/alaz/dev` cannot remove those paths because both indexers
restrict the cached deletion candidates to the current root. This is a cache
lifetime/scope gap for temporary projects.

Existing-file checks found no cached file under today's SKIP_DIRS and no cached
existing file above today's 1 MiB file limit. However, the skip policy permits
generated output and copied third-party source:

| Path component or root | Indexed BM25 files | Token/posting estimate |
| --- | ---: | ---: |
| .firebase | 211 (162 still exist) | 910,238 |
| storybook-static | 139 | 433,409 |
| graphify-out | 72 | 265,761 |
| NEXUS/assets/drawio | 471 | 2,329,118 |
| /home/alaz/dev/ai/math | 84 | 824,849 |

The first three categories account for approximately 14.2% of BM25 token volume.
These estimates use cached `doc_length`; they are not exact allocated-byte
predictions. Third-party drawio/math paths should be reviewed separately rather
than excluded globally using broad directory names.

The current source contradicts the older memory note that *only forced*
reindexing can evict missing paths: both `ProjectIndex::load_or_build` and
`trigram::ensure_index` now evict missing/changed/ineligible cached files within
the selected root even with `force=false`. Remaining stale rows indicate that
those roots have not been revisited, or are no longer revisitable. A forced
workspace reindex alone still misses the deleted `/tmp` roots.

## Proposed maintenance — approval required

Recommended first pass:

1. Add `.firebase`, `storybook-static` and `graphify-out` to the shared generated
   output exclusion policy, with meaningful exclusion/pruning regression tests.
2. Introduce a dry-run-capable maintenance operation that enumerates both cache
   file tables globally, reports missing/ineligible files and only prunes those
   cache rows after an explicit apply decision. Re-check filesystem state at
   application time; reject permission/stat errors instead of treating them as
   missing files. Use foreign-key cascades and a transaction; preserve memory,
   sessions, audit and control-plane records.
3. Validate/release the index policy before rebuilding relevant cache roots so
   excluded output cannot immediately return. Coordinate any installed binary
   update and daemon restart with the other agent.
4. After a fresh verified backup and writer quiescence, apply cache pruning and
   run VACUUM once. Verify database integrity, BM25/locate behavior and size, then
   resume the daemon. Record actual savings; do not assume stale rows alone will
   recover the full 106.8 MiB excess.

Additional optimization: `idx_trigram(trigram)` overlaps the left prefix of the
unique covering index `idx_trigram_file(trigram, file_id)`. EXPLAIN on the actual
candidate query already selects `idx_trigram_file`; removing the narrower index
could recover approximately 254.8 MiB of allocated pages after compaction, enough
to cover today's excess on its own. This needs fixture/query/performance
validation and a change to `open_trigram_db`, which currently recreates the index
on each open. Dropping it only in the live DB would provide temporary relief.

Longer term: normalize line snippets into a separate per-file/per-line table
instead of copying them into every posting. This needs a schema migration and
search equivalence/performance evidence; it was not implemented in this handoff.

Source anchors: `crates/raios-runtime/src/search/indexer.rs` (policy, per-token
snippets, root-scoped eviction); `crates/raios-runtime/src/search/trigram.rs`
(root-scoped eviction and index creation);
`crates/raios-runtime/src/system_scan/db_budget.rs` (cap).

## Validation repairs and evidence

- Re-exported `extract_plist_key` from `version/formats/mod.rs`. Existing tests in
  `version/mod.rs` and `version/semver.rs` previously failed to compile after the
  format extraction (five E0425 errors).
- Isolated temporary Git fixture repos from the developer's global signing
  setting using repository-local `commit.gpgsign=false`. Two existing tests
  previously timed out after 121 seconds waiting for the signing agent. This
  changes only disposable test repositories; project/global signing settings
  remain intact.
- Applied rustfmt to the touched disk/version files.

Final commands, all exit 0:

```text
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check -- crates
```

Tests: 1,117 passed, zero failed; two pre-existing ignored doc examples.
Logs: `.tmp/codex-handoff-health-2026-10-09/cargo-test.log` and `clippy.log`.
Local Linux validation only; no remote CI or cross-platform run was performed.

## Reflect gaps — report only

`raios reflect --json` still reports **68/100** across its currently reflected
project set. SigMap context missing: `adc-web-refactor`, `gtab`, `raios-tray`,
`AI Trader`, `VCT-ai360`, `env-i-v2`. README missing: `Management_Panels`,
`Open_Source`. No documents in those projects were created or edited.

## Continuation

Return handoff recorded successfully as `codex_kaira -> claude_kaira`:
task `45512959-65ab-421d-9f82-6f68641982d0`, approval
`c426b662-911d-49ad-a29f-a2cb24d6bd14`. Delivery is queued; Claude was not launched.

Authorized diagnosis and crate validation are done. Next action: obtain Göktuğ's
approval for the scoped cache policy/pruning/compaction plan before modifying
shared database contents or operating the live daemon. The existing backup is a
recovery point for this report; take a fresh snapshot immediately before applying
maintenance because the daemon continues writing.
