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
- `cmd_obsidian_sync` calls `raios_core::entities::load_entities(dev_ops)`
  for the project list (already carries `name`, `category`, `local_path`,
  `github`, `status`, `last_commit`, `version`) and
  `raios_runtime::filebrowser` helpers to read each project's `memory.md`.
- No new reads of `raios-core`'s DB schema are needed; this command is pure
  read-from-raios, write-to-vault.

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

← [[_MOC|ai projeleri]]

<verbatim memory.md content>
```

`status` uses raios's actual DB vocabulary as-is: `active`, `archived`,
`beklemede`, `waiting`. See Open Question below for why there's no
"production" tag.

Category `_MOC.md`:

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

## Testing

- Rust unit tests for the note-generation function: given a fixture
  `EntityProject` + a fixture `memory.md` string, assert the exact
  frontmatter and body produced (including the missing-`memory.md` case).
- Manual verification after implementation: run `raios obsidian-sync`
  against the real `~/dev` workspace into real `~/Obsidian`; confirm note
  count matches `raios projects` output; open the folder in Obsidian and
  confirm `[[wikilinks]]` between project notes and their category `_MOC`
  resolve; spot-check frontmatter on a few notes across different
  categories/statuses.

## Open Question (resolved)

raios's `projects.status` column only has four real values in the DB today
(`active`, `archived`, `beklemede`, `waiting`) — there is no "production"
tier. The user asked for "aktif, production gibi tagler" but on discovering
this gap, chose to **use the four existing statuses as-is** rather than
extend `EntityProject`/the DB schema to add a production concept. Adding
that tier (new column, migration, `load_entities`/`save_entities` changes)
is out of scope for this phase and can be proposed as its own follow-up if
still wanted later.

## Rollout

This phase ships as a `raios` binary change (new subcommand) plus one
manual run to populate `~/Obsidian/` the first time. No data migration, no
changes to existing projects' `memory.md` files, fully reversible (delete
`~/Obsidian/` to undo).
