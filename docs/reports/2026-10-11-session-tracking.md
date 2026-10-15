# Named wrapper sessions and work tracking — 2026-10-11

Author: Codex Kaira.

Wrapper sessions previously showed only agent, execution status, timestamps and exit information. Their canonical task title and description were generic and invisible in `raios sessions`. Sessions now expose a name, current work description, task ID/status, registered project name/path and full run ID, alongside the existing execution fields and final review summary.

## Usage

```sh
raios run codex --project /home/alaz/dev/core/R-AI-OS --name "Database maintenance" --work "Investigate cache growth"
raios sessions --active
raios sessions codex --project /home/alaz/dev/core/R-AI-OS --search "Database" -n 10 --json
raios session-label <full-run-id> --name "Database verification" --work "Check backup integrity"
```

Wrapper options precede agent arguments; use `--` before upstream agent flags. `session-label` can update either field independently, including labels on completed sessions, without changing task or execution status. Work descriptions are explicitly supplied; they are not inferred from transcripts or promoted from untrusted historical recall. Unnamed historical sessions keep their existing generic labels until explicitly updated. Current work and final review remain separate fields.

## Storage and boundaries

Uses existing `cp_tasks.title` and `description`, joined through the wrapper run's task ID. No new database schema or separate state file. Named launch writes task, contract, run, process witness and labels in the existing atomic start transaction. Explicit labels must be persisted before the child launches; a tracking failure aborts a named launch. Legacy callers retain compatible entry points.

Label updates require a full existing wrapper run ID with a valid task link and write an audit event in the same transaction. Local-user CLI access follows the existing trusted local maintenance boundary. Audit payloads contain IDs and changed-field indicators rather than label contents. Empty, oversized, control-character-bearing and secret-like labels are rejected. Names are limited to 120 characters and work descriptions to 2,000. Secret rejection is heuristic and does not justify supplying credentials.

Agent, running-state, exact project name/path and literal text filters apply in SQL before `LIMIT`; matching sessions cannot be hidden by another agent's recent activity. Relative project paths are canonicalized. Text search covers name, work, final summary and run ID; SQL parameters treat `%` and `_` literally. SQLite's built-in lowercase behavior limits case folding for non-ASCII text. Deterministic ordering uses timestamp and run ID.

## Validation

- Full offline workspace tests: 1,252 passed, zero failures; two existing documentation examples ignored.
- Strict workspace all-target/all-feature Clippy, format check and whitespace checks passed.
- Core regression tests cover canonical names/work fields, partial relabeling, preservation of terminal execution/task states, audit rollback, missing-run rejection, safe labels, project joins and pre-limit literal filtering.
- CLI parser test verifies wrapper labels are not forwarded to the agent and filter/update commands parse.
- Optimized release CLI build passed. Release and installed-binary subprocess smoke tests passed in temporary HOME/XDG environments with an isolated SQLite DB and a fake Codex executable: named launch, labels excluded from upstream arguments, work search, active filter, relabeling without state changes, invalid-label/missing-ID rejection, console output and audit verification. The fake agent receives the wrapper's existing network-access configuration in addition to forwarded arguments; the smoke check preserves that behavior.
- Installed CLI SHA-256: `85b6843d4bbbad928155f5193741384c1a5b6e7e9aa1ddc3eca092b813465806`. Previous CLI retained at `.tmp/session-tracking-previous-raios`; replacement checked the previous deployed hash and retained ANKA v2 compatibility/cache contents.
- Live `raios sessions --active --top 1 --json` confirms the new fields are available for existing sessions without bulk relabeling. Daemon remains active at PID 498441, zero restarts; only the CLI was replaced.
- Evidence: `.tmp/session-tracking-tests.log`, `.tmp/session-tracking-clippy.log`, `.tmp/session-tracking-release.log`, `.tmp/session-tracking-smoke-{release,installed}.json`, `.tmp/session-tracking-installed.json`, and `.tmp/session-tracking-live-list.json`.

## Scope

No edits to `tools/raios-workspace*`, `docs/superpowers/plans`, README or SIGMAP. Native workspace UI integration remains separate under that protected scope; these commands and canonical task fields provide the CLI tracking behavior now. No commit, push, paid model call, or historical bulk relabeling.
