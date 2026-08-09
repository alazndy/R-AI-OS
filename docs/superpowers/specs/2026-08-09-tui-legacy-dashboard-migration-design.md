# TUI Legacy Dashboard Migration — Design Spec (Revised)

**Date:** 2026-08-09 (revised same day, before planning — see Revision Note)
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira
**Sub-project:** A of 3 (TUI cleanup — see sibling specs B, C)

## Revision Note

A full, contiguous read of the entire dead block
(`app/events/keyboard/dashboard.rs:77-561`, all 485 lines, not just the
Constitution/Extensions-labeled sections originally sampled) found it more
tangled than first scoped: the shared `Up`/`Down`/`Left`/`Right`/`Enter`
handlers (`:373-538`) interleave cursor/focus logic for **five** legacy
panels at once — Tasks (`menu_cursor == 0`), Constitution (`== 1`), Search
(`== 6`), Projects (`== 7`), Extensions (`== 15`) — not just the two this
spec originally scoped. Removing `menu_cursor` (this spec's stated goal)
is impossible without resolving all five, since they share match arms.

Investigated and decided (with Göktuğ) for the two newly-found ones:

- **Tasks (`menu_cursor == 0`, `:487-493` checkbox toggle + cursor nav in
  the shared handlers)**: backed by `self.tasks.list`, populated via
  `raios_runtime::tasks::save_tasks`/`load_tasks` — **local markdown-file
  tasks**, confirmed structurally distinct from WORK route's real,
  daemon-backed task list (`store.snapshot.work.tasks`,
  `ui/routes/work.rs:207`). Not redundant — a genuine, separate, still
  legitimate feature. **Migrate**, same pattern as Constitution/Extensions.
- **Projects (`menu_cursor == 7`, `:103-116` sort/launcher +
  cursor/focus/open logic spread across the shared handlers)**: its `Enter`
  action (`open_project_detail`, opening `AppState::ProjectDetail`) is
  redundant — that view is already reachable live today via `/open <name>`
  and from `app/events/keyboard/project.rs:53`/`app/events/actions.rs:391`.
  Its `L` launcher key (`show_launcher = true`) is *also* redundant — the
  same flag is already set from `ProjectDetail`'s own live key handler
  (`project.rs:31`), confirmed independent of this dead code. Its **sort
  cycling** (`self.projects.sort = self.projects.sort.next()`,
  `SortMode::next()` at `app/state.rs:400`) is the one genuinely unique
  piece — WORK route's own project list (`ui/routes/work.rs`) has no sort
  mechanism today. **Delete the rest, add sort-cycling as one new
  keybinding on WORK's existing project list** — not a new view.

This expands the spec's file-change scope but not its underlying goal:
finish what the 4-route redesign started, so `menu_cursor` and its ~470
lines of interleaved dead logic can be deleted in full, not partially.

## Context

(Unchanged from original scoping — see the original audit that started this
cleanup effort.) Two parallel surfaces exist in `raios-surface-tui`: the 4
real, live routes (`Route::Now/Work/Explore/Govern`), and a pre-redesign
legacy 16-item `menu_cursor`-driven dashboard, unreachable from normal
keyboard play (`handle_dashboard_key` guards everything except `q`/`?`/`/`
once the 4-route handler declines a key) and never rendered (`render_dashboard`
calls only `render_route_view` for content, never consulting `menu_cursor`).

Every one of Constitution, Extensions, Tasks, and Search (Search is spec
B's concern, Search's *render* function is real per spec B, but its
*interaction* logic lives in this same dead file and must be extracted in
the same pass as everything else here — coordinate with spec B's plan on
sequencing, see Open Question) has a complete, working render
implementation already sitting in `ui/panels/`, `ui/health.rs`-sibling
files, or similar — orphaned by the redesign, not broken. Confirmed
redundant with something real and live: legacy task-*dispatch* (`c`/`x`/
`o`/`a` at `:496-511`, superseded by `Command::LaunchAgent`, already used
by NOW's real "LaunchCodexAgent" next-action) and all of Projects(7) except
sort-cycling (see Revision Note).

## Scope

- **Migrate to dedicated `AppState` views** (same pattern as each other,
  modeled on the existing `HealthView`/`MemPalaceView` precedent —
  `render_health_view` in `ui/health.rs`, dispatched from `ui/mod.rs:76`;
  `handle_health_view_key` in `app/events/keyboard/health.rs`, dispatched
  from `app/events/keyboard/mod.rs:107`; `Esc` returns to
  `AppState::Dashboard`):
  - **Constitution** (`menu_cursor == 1`): item editing, creator wizard,
    global-write y/N gate (`:187-201`, `:209-361`). Render already exists:
    `render_constitution` (`ui/panels/constitution.rs:15`).
  - **Extensions** (`== 15`): tab switch, inline config editing, lazy-load
    (`:117-186`). Render already exists: `render_extensions`
    (`ui/panels/extensions.rs:23`).
  - **Tasks** (`== 0`, markdown-backed): checkbox toggle (`:487-493`),
    cursor nav (extracted from the shared `:373-538` handlers). No
    dedicated render function currently exists under this name — check
    `ui/panels/content.rs`'s `0 => render_recent(frame, inner, app)` arm
    first; if `render_recent` is this panel's real renderer, reuse it
    unchanged; if not, this task's plan must build a minimal one matching
    `self.tasks.list`'s shape (`Task { text, completed, agent, project,
    .. }`, `raios-runtime/src/tasks.rs:8`).
- **Delete**: legacy task-dispatch (`c`/`x`/`o`/`a`, `:496-511`); all of
  Projects(7)'s legacy list/cursor/`L`-launcher/Enter-to-ProjectDetail
  logic; the generic 16-item `Up`/`Down` menu-cursor cycling; `menu_cursor`,
  `right_panel_focus`, and any other now-unreferenced `ui.*` fields
  (verify via compiler warnings, not inspection alone).
- **Add**: one new keybinding on WORK route's existing project list —
  cycle `self.projects.sort` (reuse the same `SortMode::next()` the dead
  code already called; do not reimplement sort logic).
- **Explicitly out of scope**: sibling specs B (Search/Logs/Timeline/Memory
  — shares this same dead file, see Open Question on sequencing) and C
  (Ocak discoverability).

## Architecture

Same established `AppState`-view pattern, applied three times (Constitution,
Extensions, Tasks) plus one small WORK-route addition (sort-cycling) plus
deletions. No new daemon calls anywhere in this spec — every migrated
piece is local UI state relocation; the WORK sort-cycling addition is also
purely local (client-side sort of an already-fetched list, matching what
the dead code already did).

## Components

| Component | Change |
|---|---|
| `app/state.rs` | Add `AppState::ConstitutionView`, `ExtensionsView`, `TasksView`. Remove `menu_cursor`, `right_panel_focus`, and any other field the compiler flags as unused after all deletions. |
| `ui/mod.rs` | Three new render-dispatch arms, matching `HealthView`'s exact call shape: `AppState::ConstitutionView => render_constitution(frame, frame.area(), app)`, `AppState::ExtensionsView => render_extensions(frame, frame.area(), app)`, `AppState::TasksView => render_recent(frame, frame.area(), app)` (or the real render function found during planning if `render_recent` turns out not to be Tasks' renderer — verify before writing this task). |
| New: `app/events/keyboard/constitution.rs` (matching `keyboard/health.rs`'s naming convention exactly) | `handle_constitution_view_key`: the moved logic from `:187-201,209-361`, with every `self.ui.menu_cursor == 1 && self.ui.right_panel_focus` guard prefix stripped (the new handler is only ever called when already in `ConstitutionView`, so the guard is redundant) — leaves e.g. `KeyCode::Char(n @ '1'..='9') if !self.constitution.item_editing && !self.constitution.creator.active => { ... }`. Add `KeyCode::Esc if !self.constitution.item_editing && !self.constitution.creator.active => { self.state = AppState::Dashboard; }` as the base close case (the old generic `Esc if right_panel_focus` catch-all doesn't survive — this view has no separate "panel focus" concept, it's the whole screen). |
| New: `app/events/keyboard/extensions.rs` | `handle_extensions_view_key`: same stripping treatment applied to `:117-186`. |
| New: `app/events/keyboard/tasks.rs` | `handle_tasks_view_key`: checkbox toggle (`:487-493`, guard stripped) + cursor nav (extracted from `:373-538`'s `menu_cursor == 0` arms, guard stripped). `Esc`/`q` closes to `AppState::Dashboard`. |
| `app/events/keyboard/mod.rs` | Three new dispatch arms in the state-match (mirroring `AppState::HealthView => { self.handle_health_view_key(key); Ok(()) }` exactly), alphabetically placed among the existing `constitution`/`dashboard`/`editor`/`extensions`/`health`/`project`/`setup`/`tasks` module list. |
| `app/events/commands.rs` | `/rules` (`:269`-area, currently sets `AppState::HealthView`'s sibling pattern — verify exact current line) sets `self.state = AppState::ConstitutionView`. `/ext` sets `ExtensionsView`. Whatever command currently opens the legacy Tasks panel (verify — may not exist as a slash command today, since Tasks was only reachable via direct `menu_cursor` value before the redesign; if no command exists, this plan must add one, e.g. `/tasks`) sets `TasksView`. |
| `app/events/keyboard/dashboard.rs` | Delete the entire `:77-561` legacy block except lines `:89-102` (quit, help, GitDiffView — unrelated global affordances, keep) and the `/`/`Tab` command-palette opener (`:363-372`, keep). Every other match arm in this range is either migrated (above) or deleted (task-dispatch, all of Projects(7)). |
| `ui/routes/work.rs` | Add one keybinding (verify WORK's existing key-dispatch file/location and follow its established style — likely `app/events/keyboard/dashboard.rs`'s WORK-specific section per the earlier route audit, not this legacy file) cycling `self.projects.sort = self.projects.sort.next()` on the currently-focused project list. |

## Data Flow

Unchanged from each panel's pre-redesign (dormant) behavior — this is a
relocation of working local-state logic behind new entry/exit points, not
a rewrite. The one new piece (WORK sort-cycling) is also local: no daemon
round-trip, sorts the already-fetched `self.projects.list` in place using
the existing `SortMode` enum.

## Error Handling

Unchanged — each migrated flow's existing validation (Constitution's
global-write gate, Extensions' config-field validation) carries over
verbatim.

## Testing

- Render + key-handler tests for all three new views, following whatever
  test convention actually covers `HealthView` today (if none exists,
  establish one using the golden-render pattern already proven for the 4
  real routes — `ui/routes/tests.rs::golden_render_work_route` —
  `TestBackend`/`Terminal`, assert on visible text).
- A test confirming legacy task-dispatch keys (`c`/`x`/`o`/`a`) and
  Projects(7)'s `L`/Enter-to-ProjectDetail/sort-cycle-in-old-location are
  all gone (no-ops) outside any migrated context.
- A test confirming WORK's new sort keybinding actually re-orders the
  visible project list.
- A compile-time check (not a runtime test) that `menu_cursor`/
  `right_panel_focus` produce no dangling-reference warnings after removal.

## Open Question for the Implementation Plan

**Sequencing with spec B**: both specs delete/rewrite large, overlapping
regions of the same file (`app/events/keyboard/dashboard.rs`'s shared
`:373-538` handlers touch Tasks/Constitution/Search/Projects/Extensions
together). The implementation plan (or the SDD execution order decided
after both plans are written) should execute spec A and spec B as
**one combined pass** over this file, or explicitly sequence A fully
before B (recommended, since A's deletions simplify what B's plan has to
read) rather than attempting them as truly parallel, independent branches
that would conflict merging back into this one file.

**Tasks' real render function**: not confirmed during brainstorming whether
`render_recent` (`content.rs`'s `0 =>` arm) is genuinely the Tasks-list
renderer or something else entirely (the name doesn't obviously match) —
the implementation plan must read `ui/panels/content.rs`'s neighboring
context and `render_recent`'s actual body before committing to reusing it.
