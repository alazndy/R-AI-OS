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

- `$XDG_CONFIG_HOME/raios/anka-exclude`: one case-insensitive project pattern
  per line; matching records are skipped at indexing time.
- `$XDG_CONFIG_HOME/raios/anka-tombstones`: record IDs created by
  `raios anka forget`; tombstoned records stay excluded on later rebuilds.

The original harness transcript is never modified by either control.

## Public Surface

```text
raios anka status
raios anka index [--harness <name>]
raios anka search <query> [--project <path>] [--harness <name>]
raios anka blame <path>
raios anka forget <record-id>
```

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
