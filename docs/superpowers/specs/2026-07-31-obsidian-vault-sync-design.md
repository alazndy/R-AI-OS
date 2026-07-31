# Obsidian Vault Sync — Design (Phase 1 of 4)

## Context

This is the first of a four-part effort to make raios's project and memory
data browsable in Obsidian. The full scope (agreed with the user 2026-07-31)
is, in build order:

1. **Vault skeleton & taxonomy + `raios obsidian-sync`** (this spec)
2. Claude auto-memory → Obsidian-compatible bridge (frontmatter + tunnels
   into the project notes this phase creates)
3. MemPalace (Wing/Room/Drawer) → periodic export
4. raios control-plane / instinct data → periodic export

Phases 2–4 are each their own future spec. This document covers phase 1
only: a new vault, its folder/tag taxonomy, and a new `raios obsidian-sync`
CLI subcommand that populates it from the 68 projects raios already tracks.

### Why a new vault instead of reusing Vault101

`~/dev/core/Vault101/Vault101/` is an existing Obsidian vault (`.obsidian`
present, dataview/templater/tasks-plugin/git plugins installed) but it
predates the current Linux/raios workspace: its `MASTER.md` still references
`C:\Users\turha\Desktop\Dev Ops` and it has only 2 stray project notes
(UniControl, Guardian Glass) against the 68 projects raios now tracks. It
also mixes in unrelated personal content (Finans.md, Takvim/, Excalidraw/).
The user chose to start a clean vault at `~/Obsidian/` and pull over
whatever's useful from Vault101 (plugin config, Homepage.md dashboard
pattern) in a later, separate step — not as part of this build.

## Goals

- A new Obsidian vault at `~/Obsidian/` containing one note per raios
  project, organized by category, tagged by status.
- A repeatable `raios obsidian-sync` command that regenerates those notes
  from the current state of `raios_core::entities` + each project's
  `memory.md` — safe to run any time, e.g. after a work session.
- No changes to raios's read-side data model (`EntityProject`, the
  `projects` DB table, `memory.md` format) — this phase is additive/export
  only.

## Non-goals (explicitly deferred)

- Claude auto-memory, MemPalace, and control-plane/instinct export (phases
  2–4).
- Porting Vault101's plugins, Homepage.md dashboard, or personal notes.
- Git-tracking the vault itself, or registering it as a raios project.
- MCP or TUI exposure of the new command — CLI only in this phase.
- Adding a "production" status tier to raios (see Open Question below —
  explicitly rejected for this phase).

## Architecture

New file `crates/raios-surface-cli/src/cli/obsidian_sync.rs`, following the
existing command pattern (`health.rs`, `workspace.rs`):

- `Commands::ObsidianSync { vault: Option<String>, dry_run: bool }` added to
  the `Commands` enum in `args.rs`.
- Dispatch line in `mod.rs`:
  `Commands::ObsidianSync { vault, dry_run } => obsidian_sync::cmd_obsidian_sync(vault, dry_run, &cfg.dev_ops_path, cli.json)`.
- `cmd_obsidian_sync` → `raios_runtime::obsidian::sync_vault(dev_ops, vault, dry_run)`
  → `raios_core::entities::discover_all_entities(dev_ops)` for the project
  list (see "Amendment: syncing all real projects, not just DB-active ones"
  below for why this is `discover_all_entities`, not `load_entities`), and
  `raios_runtime::filebrowser` helpers to read each project's `memory.md`.
- No writes to `raios-core`'s DB schema — `discover_all_entities` is a pure
  filesystem scan (via `raios_core::mempalace::build`), no DB round-trip at
  all; this command is pure read-from-filesystem, write-to-vault.

## Vault layout

```
~/Obsidian/
  Projeler/
    ai/
      <project-name>.md   (one per project in this category)
      _MOC.md              (category index — links to every project note)
    web/
    embedded/
    tools/
    core/
    audio/
    mobile/
    archives/
  Proje Atlası.md          (root MOC — links to all 8 category MOCs + totals)
```

Category folders match raios's existing `category` values 1:1 (`ai`, `web`,
`embedded`, `tools`, `core`, `audio`, `mobile`, `archives`) so the sync
logic needs no category-name translation table.

raios only ever writes plain Markdown into this tree. It does not create or
touch `.obsidian/`; that's created by the Obsidian app the first time the
user opens `~/Obsidian/` as a vault.

## Frontmatter schema

Per-project note:

```yaml
---
tags: [proje, "kategori/ai", "durum/active"]
category: ai
status: active
local_path: /home/alaz/dev/ai/some-project
github: https://github.com/alazndy/some-project
last_commit: 2026-07-30
version: "1.2.0"
synced: 2026-07-31T22:00:00
---
# some-project

← [[ai-MOC|ai projeleri]]

<verbatim memory.md content>
```

`status` uses raios's actual DB vocabulary as-is, rendered verbatim
whatever string it holds — currently observed on this machine: `active`,
`archived`, `beklemede`, `waiting`. `production`, `early`, and `legacy`
are also valid, supported status values in raios's schema (see Open
Question below) and will render as `durum/production` etc. automatically
if/when they appear in the data — no rendering-code change needed.

Category MOC file is named `<category>-MOC.md` (e.g. `ai-MOC.md`), not a
bare `_MOC.md`, specifically so every category's MOC has a vault-wide
unique filename — Obsidian resolves `[[wikilinks]]` by filename, and 8
identically-named `_MOC.md` files (one per category folder) would make
`[[_MOC]]` ambiguous everywhere it's used.

```yaml
---
tags: [moc, "kategori/ai"]
---
# ai projeleri

- [[some-project]] — active
- [[other-project]] — beklemede
...
```

Root `Proje Atlası.md` links to each category's `_MOC.md` and prints a
static count summary (total projects, per-category count, per-status
count) — plain text, regenerated each run, no Dataview dependency required
for correctness. (Dataview-powered dashboards, like Vault101's
Homepage.md, are a nice-to-have left for the later "port from Vault101"
step, not required for this phase to be useful.)

## CLI

```
raios obsidian-sync [--vault <path>] [--dry-run] [--json]
```

- `--vault` default: `~/Obsidian`.
- `--dry-run`: computes and prints/returns what would be written (counts,
  paths) without touching disk.
- `--json`: machine-readable summary — `{ "written": N, "skipped": N,
  "errors": [...] }` — consistent with the `--json` convention used by
  `health`/`stats`/`commit`.
- Creates `~/Obsidian/Projeler/<category>/` directories as needed
  (`mkdir -p` semantics).

## Regeneration model

Every run **fully overwrites** each project note, MOC, and the root index.
This was an explicit user choice (full-copy-from-memory.md, not
summary+link) traded against: any manual edits made directly inside a
project note in Obsidian are lost on the next sync. This is expected
behavior, not a bug — if a user wants to keep hand-written content, it
belongs in a *different* note that links to the project note, not inside
it. Worth stating plainly in the command's `--help` text so it isn't a
surprise.

## Error handling

- Missing `memory.md` for a project: still emit a note with frontmatter +
  a literal `_memory.md not found_` line under the heading. Do not abort
  the whole sync for one missing file.
- Vault directory (or category subdirectory) missing: create it.
- Project name collision across categories: not handled specially in this
  phase — last one written wins. Checked against the current 68 projects;
  no collisions exist today. Documented as a known limitation, not solved
  preemptively (YAGNI — no evidence it will happen).
- Unreadable/corrupt `memory.md` (I/O error mid-read): treat like "missing"
  for that one project, log to stderr (or the `errors` array in `--json`
  mode), continue with the rest.
- Known limitation: orphaned project notes are not pruned. Every one of the
  8 known category MOCs (see Vault layout above) is rewritten on every
  non-dry-run sync, including empty ones — so a category's MOC can never go
  stale-and-orphaned just because its last project disappeared. But
  individual project notes are a different story: if a project is deleted,
  renamed, or its git/`memory.md` markers stop satisfying
  `raios_core::mempalace`'s project-root detection (see "Amendment: syncing
  all real projects" below) so it no longer appears in the current run's
  project list, its old `<name>.md` note file is left in place — it is
  simply never linked from any MOC again. Full pruning of stale individual
  project notes was considered and explicitly deferred (minimum-viable fix
  only); a future phase can add it if it becomes a real problem.
- Known limitation: a second, pre-existing vault writer exists in the TUI.
  `crates/raios-surface-tui/src/app/services.rs`'s `create_vault_note`
  function (reachable via the TUI's `/vault-create` command) is a separate,
  older vault-writing feature that this phase did not touch or unify. It
  writes a different note format (flat file layout, different frontmatter,
  a generated stub body instead of `memory.md` content, never-overwrite
  instead of always-overwrite) to a different — and currently
  misconfigured — target path (`config.vault_projects_path`), so the TUI's
  "has vault note" badge will not reflect notes written by
  `raios obsidian-sync` or the new `raios new` vault step. This is
  consistent with this phase's Non-goals ("no MCP/TUI in this phase"); the
  user was consulted directly and explicitly decided to leave the TUI
  feature untouched for now, documenting it as a known gap. Reconciling or
  retiring it is left for a future phase.

## Testing

- Rust unit tests for the note-generation function: given a fixture
  `EntityProject` + a fixture `memory.md` string, assert the exact
  frontmatter and body produced (including the missing-`memory.md` case).
- Manual verification after implementation: run `raios obsidian-sync`
  against the real `~/dev` workspace into real `~/Obsidian`; confirm note
  count is close to `raios projects`' output (see "Amendment: syncing all
  real projects" below for why it's not expected to be an exact match);
  open the folder in Obsidian and confirm `[[wikilinks]]` between project
  notes and their category MOC resolve; spot-check frontmatter on a few
  notes across different categories/statuses.

## Open Question (resolved)

raios's `projects.status` column only has four real values in the
**current database's data** on this machine today (`active`, `archived`,
`beklemede`, `waiting`). The user asked for "aktif, production gibi
tagler" but at the time this spec was written, the team believed there was
no "production" tier at all in raios and chose to **use the four
currently-observed statuses as-is**.

**Correction (added during final review):** that belief was wrong.
`crates/raios-core/src/db/projects.rs`'s upsert logic actually whitelists
`production`, `active`, `early`, `legacy`, `waiting` — a `production`
status IS a valid, supported value in raios's schema, not a hypothetical
extension. `crates/raios-core/src/mempalace.rs`'s status-normalization
function (`normalize_status`) can actively produce `production`, `early`,
or `legacy` from a project's `memory.md` text. Those values simply don't
happen to be present in this machine's current database rows — that's a
data-population fact, not a schema limitation. No code change is needed to
support them: this feature's status-tag rendering
(`render_project_note`/`render_moc`/`render_atlas` in
`crates/raios-runtime/src/obsidian.rs`) is generic over whatever string
`status` holds, so a `durum/production` tag will simply appear
automatically the first time a project's `memory.md` says "production" and
`raios discover` runs. The four currently-observed values above remain
accurate as *today's* data, just not as the schema's ceiling.

## Amendment: `raios new` integration (added during planning)

While mapping this spec to real code, a second, already-existing vault
integration point turned up: `crates/raios-runtime/src/new_project.rs`
step 9 calls `update_vault_atlas()`, which is meant to append a row to a
vault's "Proje Atlası.md" on every `raios new`. It is dead code — it only
checks a hardcoded Windows path
(`C:\Users\turha\Documents\Obsidian Vaults\Vault101\...`) and a Linux
fallback path that has never existed on this machine
(`Vault101/Projeler/Proje Atlası.md`) — so it has silently no-op'd on
every project creation since the Linux migration. The user asked to fix
this forward rather than leave it dead or just delete it: `raios new`
should keep the new `~/Obsidian` vault in sync going forward, not only via
manual `raios obsidian-sync` runs.

Resolution: `update_vault_atlas()` is deleted. Step 9 of `new_project::create`
instead calls the same sync engine this spec builds for the CLI command
(see Architecture amendment below), so there is exactly one code path that
writes project notes into the vault — used both by the manual bulk command
and automatically by project creation.

## Amendment: testability split (added during planning)

`raios_core::entities::load_entities()` reads from the single global
`~/.config/raios/workspace.db`, not from whatever `dev_ops` path is passed
in — so a test that passes a tempdir as `dev_ops` would still load the
real machine's real 68+ projects out of the real database, making the sync
engine's own tests slow and non-deterministic on any machine that already
has raios set up (which is every machine this ships to).

To keep the engine testable, the sync logic is split into two layers in
`raios-runtime`:

- `sync_vault_projects(vault: &Path, projects: &[EntityProject], dry_run: bool) -> ObsidianSyncReport` —
  does the actual note/MOC/atlas rendering and writing for an explicit,
  caller-supplied project list. Fully hermetic: tests pass hand-built
  `EntityProject` fixtures pointing at tempdir paths, no DB involved.
- `sync_vault(dev_ops: &Path, vault: &Path, dry_run: bool) -> ObsidianSyncReport` —
  thin wrapper: loads the project list, then delegates to
  `sync_vault_projects`. This is what the CLI command and `raios new` call.
  (Originally `load_entities`; see the next amendment for why this became
  `discover_all_entities`.)

Both `raios obsidian-sync` (CLI) and `new_project::create`'s vault step
call `sync_vault`/`sync_vault_projects` — the same engine, so the two
integration points cannot drift into different note formats.

## Amendment: syncing all real projects, not just DB-active ones (added post-ship)

After the first real sync, the user looked at `~/Obsidian` and pointed out
it only had 11 of their 68 real projects — several categories (`ai`,
`archives`, `audio`) had nothing but an empty MOC. This traced back to
`sync_vault`'s original data source, `raios_core::entities::load_entities`,
which is DB-backed and deliberately excludes any project whose `status`
row is `waiting` or `beklemede` — a filter applied consistently by every
existing DB-backed raios command (`health`, `stats`, `commit`, `discover`).
66 of the 68 real projects on this machine carry one of those two statuses
in the DB (a lifecycle classification, unrelated to each project's actual
`memory.md` content), so `load_entities` returned only 11.

The user was asked directly and confirmed: they want the vault to reflect
every real project, not the DB-curated subset — "everything in the vault
should be real." Since re-deriving "real" from the DB's lifecycle status
was rejected (that status isn't about project reality, just DB bookkeeping
staleness — confirmed by re-running `raios discover`, which re-scans the
filesystem and still only returned 11, because the *exclusion* happens
after the scan, not because the scan itself is incomplete), the fix
changes `sync_vault`'s data source entirely:

- New function `raios_core::entities::discover_all_entities(dev_ops)` — a
  pure filesystem scan via `raios_core::mempalace::build` (the same
  scanner `raios discover` uses to find fresh projects, but without the
  DB round-trip or the `waiting`/`beklemede` filter applied afterward).
  Every project mempalace recognizes as a project root (has `.git`,
  `Cargo.toml`, `package.json`, `.raios.yaml`, or `memory.md` alongside a
  `src`/`app`/`lib`/`scripts` folder) with a `memory.md` file is included,
  regardless of status. `status` on the resulting `EntityProject` comes
  from `mempalace`'s own per-project status parsing of `memory.md`
  (`production`/`active`/`early`/`legacy`, or `—` if unparseable) — not
  from the DB at all. `github`/`last_commit`/`stars` are `None` for every
  project (that metadata is DB-only and this path never touches the DB).
- `sync_vault` now calls `discover_all_entities` instead of
  `load_entities`. This applies to both `raios obsidian-sync` and
  `raios new`'s vault step (they share `sync_vault`), so the two stay
  consistent — deliberately diverging from `health`/`stats`/`commit`/
  `discover`, which remain DB-scoped on purpose (this is the one command
  in the family whose whole point is "show me everything that's real").

**Result: 65 of 68 projects sync**, not all 68. The remaining 3-project
gap is not a bug in this fix — it's `raios projects`' own scan
(`discover_memory_files`, a flat "any directory with a `memory.md`" walk,
no project-root reasoning) being *more* permissive than is actually
correct:
- `ai-trader/backend/memory.md` and `ai-trader/frontend/memory.md` are
  sub-components of the single `ai-trader` project (which itself syncs
  correctly). `raios projects` counts these as 2 separate top-level
  projects; `mempalace::build` correctly recognizes `ai-trader` as one
  project root (it has `.git`) and, per its own documented behavior, does
  not recurse into a project root looking for nested projects — so
  `backend`/`frontend` are (correctly) not surfaced as independent
  top-level vault notes.
- `core/Vault101/Vault101/memory.md` is an empty (0-byte) file nested two
  directories inside `core/Vault101/`, which itself has `.git` at the
  *outer* level. `mempalace` recognizes the outer `Vault101/` folder as
  the project root and (per the same "don't recurse into a project root"
  rule) never reaches the inner, git-less, memory-less `Vault101/Vault101/`
  directory `raios projects` found via its flat walk.

Both gaps were reviewed and judged not worth forcing: fixing them would
mean either weakening `mempalace`'s shared project-root detection (used
elsewhere, e.g. `raios discover`) or special-casing monorepo sub-projects,
neither of which this phase's scope justifies for 3 edge-case entries out
of 68. This is recorded as a known, accepted gap, not a TODO.

## Rollout

This phase ships as a `raios` binary change (new subcommand) plus one
manual run to populate `~/Obsidian/` the first time. No data migration, no
changes to existing projects' `memory.md` files, fully reversible (delete
`~/Obsidian/` to undo).
