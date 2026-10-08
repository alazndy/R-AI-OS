# ANKA — Agent Narrative Knowledge Archive

## Objective

ANKA provides read-only recall over historical coding-agent transcripts. It is
inspired by Deja-vu's retroactive search model, but remains a native R-AI-OS
feature governed by R-AI-OS security and memory policies.

## Authority Boundary

ANKA is evidence, not project truth.

- Curated decisions remain in `workspace.db` (`mem_items`, lineage, and control plane).
- ANKA stores rebuildable, redacted transcript search material outside `workspace.db`.
- A future promotion flow must require an explicit user or policy-approved action before writing curated memory.

## Cache Boundary

Default path: `$XDG_CACHE_HOME/raios/anka` (or the platform cache equivalent).

The cache will be owner-only, rebuildable from local source transcripts, and
excluded from source control. It must never be treated as a synchronization or
authority channel.

Privacy controls live beside the normal R-AI-OS configuration:

- `$XDG_CONFIG_HOME/raios/anka-policy`: the consent record written by
  `raios anka policy-init --home keep|exclude` — a versioned header plus the
  mandatory HOME retention choice and the home path it was made for. Recall,
  indexing, and forget all refuse to run until it exists and parses; there is
  no empty-allow fallback.
- `$XDG_CONFIG_HOME/raios/anka-exclude`: one case-insensitive **literal**
  project substring per line (never globs, never regexes), matched against
  every project claim a record carries — the resolved provenance path, the raw
  harness slug, and the display label; matching records are skipped both at
  indexing time and on every recall, so a rule change takes effect on the next
  query without waiting for a rebuild.
- `$XDG_CONFIG_HOME/raios/anka-tombstones`: record IDs created by
  `raios anka forget`; tombstoned records stay excluded on later rebuilds.

Records whose provenance cannot name a project are excluded by default, and
HOME records follow the typed choice in `anka-policy` — an exact match against
the consent's recorded `home_path` (absolute and already normalized; interior
`//` collapses first) or its exact encoded slug, never a substring, so child
projects stay eligible. The decision stays bound to that recorded path even if
the process `$HOME` changes later. The original harness transcript is never
modified by any of these controls.

## Public Surface

```text
raios anka status
raios anka index [--harness <name>]
raios anka search <query> [--project <path>] [--harness <name>]
raios anka blame <path>
raios anka forget <record-id>
raios anka policy-init --home keep|exclude
raios anka policy-show
```

`policy-init` creates the privacy consent file and nothing else — no
indexing, no cache, no timer, and it never overwrites an existing policy.
`policy-show` is read-only: resolved exclusion rules, tombstone count, the
kept/excluded breakdown of the current cache (`absent|ok|unreadable` — an
uninspectable cache reports `unreadable` with the I/O detail, never a
zero-record `absent`), and a **pre-publication count**: freshly discovered
sources evaluated against the same policy and tombstones the next rebuild
applies, so a rule change's losses are countable before any rebuild publishes
them.
`status` carries a `policy` summary (`initialized`, `home`, `exclude_rules`)
alongside the cache state.

`index` currently discovers local Claude Code JSONL sessions plus the existing
Codex, OpenCode, and Antigravity history files. The index is lexical and local;
automatic context injection is intentionally not part of this phase.

## MCP

`anka_recall` is the sole MCP exposure. It searches the existing cache but
cannot index, forget, share, synchronize, or promote any record. Non-empty
responses are wrapped as untrusted historical evidence so stored transcript
text cannot be treated as current instructions.

## Security Requirements Before Enablement

1. Redact credentials before any cache write; never offer an opt-out flag.
2. Support project exclusions and durable forget tombstones before indexing.
3. Limit recall output and frame it as untrusted historical text.
4. Preserve harness, project, session, and timestamp provenance on every hit.
5. Keep automatic context injection disabled until explicit review.
6. Require an initialized `anka-policy` for recall, indexing, and forget —
   missing, malformed, or unreadable policies fail closed with an error, and
   unknown-provenance records are excluded by default under every policy. A
   `home_path` that is not an absolute, already-normalized path counts as
   malformed: formatting must never decide what HOME means.
