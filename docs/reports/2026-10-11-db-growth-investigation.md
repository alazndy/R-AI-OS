# Database growth investigation — 2026-10-11

Read-only measurements correct the stale handoff status: the October 10 maintenance had already reduced the live database below its cap. Current main file: 2,414,665,728 bytes (2.249 GiB); observed WAL: 4,181,832 bytes. Cap remains 2,621,440,000 bytes (2,500 MiB). There are no free pages. Current main file is only 1,261,568 bytes larger than the previous final recorded measurement of 2,413,404,160 bytes.

## Causes

1. The original excess came from missing file roots and generated/deployment artifacts retained in global search caches. October 10 maintenance removed 4,013,530 postings. Current preview finds only one missing BM25 file (1,383 postings) and one missing trigram file (2,481 postings); no excluded-directory candidates. Repeating stale pruning alone would not materially shrink the current database.
2. BM25 stores a copy of a line's snippet for every token occurrence. `ProjectIndex::index_file` explicitly clones the snippet inside the token loop; SQLite stores each copy in `bm25_postings`. Current 10,476,353 rows contain 744,240,227 snippet bytes (709.76 MiB), versus 69,063,051 token bytes. Snippet payload alone accounts for 74% of the 1,005,088,768-byte BM25 table. This is storage amplification by design, rather than evidence of accidental repeated insertions.
3. The shared traversal exclusions include `worktrees`, but omit `.worktrees`. The R-AI-OS `.worktrees` subtree contributes 1,115 indexed files / 1,150,234 BM25 postings (about 11% of all BM25 rows). Three inspected Rust files have byte-identical SHA-256 content in the main checkout and a nested worktree. This proves duplicated source content is being indexed, although independent worktrees can also contain legitimate different changes.
4. Vendored/generated files with eligible extensions and under the 1 MiB size limit remain eligible outside excluded directory names. NEXUS assets contribute 2,329,118 postings, and the separate `ai/math/es5` subtree contributes 821,088. Both include the same MathJax `mml-chtml.js` file (97,306 postings each). Other large inputs include lockfiles, packed JS bundles, data catalogs and generated C image data. These counts establish contributors; they do not justify automatically deleting all assets/data.

No orphan BM25 postings were found. Trigram indexes have different keys: `(trigram)`, unique `(trigram,file_id)`, and `(file_id)`. This investigation does not treat them as interchangeable duplicate indexes.

## Recommended next change

Normalize snippets into one row per file/line, retaining token occurrence semantics and search ranking, with an offline schema/migration rehearsal before owner-approved live changes. Exact savings require measuring unique snippet rows on a backup.

Avoid walking nested `.worktrees` while indexing a parent checkout, but preserve explicitly requested worktree-root searches. A blanket global deletion of all worktree cache rows would impair valid worktree workflows. Add targeted generated/vendor exclusions with previewable path counts rather than dropping all asset directories.

No production rows were deleted, no checkpoint/compaction performed, no daemon restarted, and no new backup was needed for these read-only queries. The previously verified pre-cleanup snapshot is documented in `2026-10-10-search-cache-maintenance.md`; any future destructive change requires a fresh verified backup and owner approval.

Evidence: `.tmp/db-growth-current.json`, `.tmp/db-growth-prune-preview.json`, `.tmp/db-growth-distribution.json`, `.tmp/db-growth-amplification.json`. Measurements are separate read-only observations; concurrent daemon writes may cause small differences between them.
