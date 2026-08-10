# TUI Slash-Command Routing — Design Spec (Revised)

**Date:** 2026-08-09 (revised same day, before planning — see Revision Note)
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira
**Sub-project:** B of 3 (TUI cleanup — see sibling specs A, C)

## Revision Note

The original version of this spec assumed `/search`, `/logs`, `/timeline`,
`/memory` had no real render implementation and should be rerouted to
EXPLORE's existing panels. Deeper investigation during implementation
planning (reading `ui/panels/content.rs`'s dispatcher, which — like spec A's
Constitution/Extensions — routes all 16 legacy `menu_cursor` items to
complete, real render functions that are simply unreachable from any live
call site) found this assumption wrong for three of the four commands:

- **`/logs` (`menu_cursor == 9`)** → `render_logs` (`ui/panels/logs.rs:14`)
  shows `app.system.active_agents` — an **active daemon-spawned agent
  process list**, unrelated to EXPLORE's `recent_logs` (a daemon event-log
  stream). Different data entirely, and newly relevant given the tmux-steer
  feature this session also shipped.
- **`/timeline` (`menu_cursor == 8`)** → `render_timeline`
  (`ui/panels/timeline.rs:14`) shows `app.timeline.activities` — a local UI
  activity/event log, unrelated to EXPLORE's `recent_traces` (daemon
  tool-call traces).
- **`/search` (`menu_cursor == 6`)** → `render_search_panel`
  (`ui/search.rs:99`) does read the same dead `self.search.*` state
  originally flagged, but the panel it backs is a genuinely different
  feature from EXPLORE's daemon-search: an instant, client-side local index
  search. Not a duplicate to delete — a distinct capability to revive.
- **`/memory` (`menu_cursor == 5`)** → `render_mempalace_info`
  (`ui/mempalace.rs:14`) is not a memory.md viewer at all — it's a small
  MemPalace summary widget ("N rooms · N projects") that hints at the
  already-working `/mempalace` full view (`AppState::MemPalaceView`, live
  today). This one *is* redundant, but with `/mempalace`, not with EXPLORE.

Decision (confirmed with Göktuğ): revive each of the first three as its own
dedicated `AppState` view, following spec A's exact pattern — not reroute to
EXPLORE, not delete. `/memory` becomes a plain alias for `/mempalace`'s
existing behavior. EXPLORE's own panels are untouched by this spec, same as
before.

## Scope

- **Revive `/search`** → new `AppState::SearchView`, wrapping the existing
  `render_search_panel` plus the existing-but-currently-dead cursor/open
  logic for `self.search.{cursor,results}` (found in the legacy dashboard
  key handler, real and complete — verified at
  `app/events/keyboard/dashboard.rs`, relative lines ~312-383 and ~440
  within the dead block: `Up`/`Down` move `self.search.cursor`, an
  open-result action reads `self.search.results.get(self.search.cursor)`).
- **Revive `/logs`** → new `AppState::ActiveAgentsView`, wrapping the
  existing `render_logs`. Unlike search, this one needs **new** key logic:
  `app.system.selected_agent_idx` (`app/state.rs:682`) is read by the
  render function but is never mutated anywhere in the codebase today — no
  existing cursor-navigation logic to move. This spec adds it.
- **Revive `/timeline`** → new `AppState::TimelineView`, wrapping the
  existing `render_timeline`. Read-only display, matching its current
  (inert) design — no selection/interaction needed, `Esc` to close is
  sufficient.
- **Alias `/memory`** → same handler as `/mempalace`: `self.state =
  AppState::MemPalaceView` (plus whatever build-triggering logic the real
  `/mempalace` arm already performs — copy it, don't duplicate a second
  code path).
- **Explicitly out of scope**: EXPLORE route's own panels (untouched);
  sibling specs A (Constitution/Extensions — same pattern, separate plan)
  and C (Ocak discoverability).

## Architecture

Same established pattern as spec A: each revived panel gets an `AppState`
variant, a dedicated render dispatch entry (`ui/mod.rs`, matching
`AppState::HealthView => render_health_view(frame, app)` — note the existing
render functions here take `(frame, area, app)`, one arg more than
`render_health_view`; the dispatch call passes `frame.area()` as `area`,
matching how `content.rs`'s dead dispatcher itself called them), and a
dedicated key-handler method registered in
`app/events/keyboard/mod.rs`'s state-dispatch match.

## Components

| Component | Change |
|---|---|
| `app/state.rs` | Add `AppState::SearchView`, `ActiveAgentsView`, `TimelineView`. Add `selected_agent_idx` mutation support (it's already a field — no new field, just new code that writes it). |
| `ui/mod.rs` | Three new dispatch arms: `AppState::SearchView => render_search_panel(frame, frame.area(), app)`, `AppState::ActiveAgentsView => render_logs(frame, frame.area(), app)`, `AppState::TimelineView => render_timeline(frame, frame.area(), app)` — calling the existing functions directly, no relocation of `ui/search.rs`/`ui/panels/logs.rs`/`ui/panels/timeline.rs` needed (unlike spec A's Constitution/Extensions, these render files don't need to move — "files that change together should live together," and these were already correctly isolated). |
| New: `app/events/keyboard/search_view.rs` (naming to match spec A's established convention for the new keyboard files — verify exact convention when spec A's plan is written first, or independently confirm during this plan) | `handle_search_view_key`: moves the real cursor/open logic identified above from the dead block, adapted to the new `AppState` variant instead of `menu_cursor == 6`. |
| New: `app/events/keyboard/active_agents_view.rs` | `handle_active_agents_view_key`: **new** logic — `Up`/`Down`/`k`/`j` move `self.system.selected_agent_idx` within `0..self.system.active_agents.len()` bounds (bounds-check pattern mirrors `handle_health_view_key`'s existing `if self.health.cursor > 0` / `if self.health.cursor + 1 < self.health.report.len()` style), `Esc`/`q` returns to `AppState::Dashboard`. |
| New: `app/events/keyboard/timeline_view.rs` | `handle_timeline_view_key`: `Esc`/`q` only — read-only panel, no cursor. |
| `app/events/commands.rs` | `/search` arm: on non-empty `arg`, keep the existing real search-execution logic (local index search or `daemon_search_command` for remote hubs) unchanged, but set `self.state = AppState::SearchView` instead of `menu_cursor`/`right_panel_focus`. `/logs` arm: keep the existing `daemon_get_logs_command` send (unchanged, still useful — it refreshes the daemon-known agent list) and set `self.state = AppState::ActiveAgentsView`. `/timeline` arm: set `self.state = AppState::TimelineView`. `/memory` arm: replace its body with exactly what the `/mempalace` arm does (`commands.rs:99-113`) — same build-trigger-if-needed logic, same `self.state = AppState::MemPalaceView`. |

## Data Flow

`/search`: unchanged real search execution (local index or daemon) → results
land in `self.search.results` (unchanged) → now actually rendered via
`AppState::SearchView`. `/logs`: unchanged daemon request → `self.system.active_agents`
already gets updated by whatever background message handler already
processes daemon agent-list pushes (verify this handler exists and is live
— it must be, since `render_logs` already reads real `active_agents` data
today in code, even though unreachable) → now actually rendered.
`/timeline`: `self.timeline.activities` already gets appended to by
whatever existing `add_activity`-style calls exist across the codebase
(confirmed at least one real call site: `commands.rs`'s `/search` arm
itself calls `self.add_activity(...)`) → now actually rendered. `/memory`:
identical to `/mempalace`'s existing real data flow.

## Error Handling

Unchanged from each panel's existing (currently unreachable but complete)
handling — e.g. `render_search_panel`'s existing "No index — use /search
<query> to build" / "Building index..." states carry over verbatim.

## Testing

- Golden-render tests for the three new views, following the exact pattern
  already established for the 4 real routes
  (`ui/routes/tests.rs::golden_render_work_route` — `TestBackend` +
  `Terminal` + a `Store`, assert on visible text via `get_rendered_text`),
  adapted since these are `AppState`-driven full-screen views rendered via
  `App` rather than route-driven `Store` views — verify whether an
  equivalent test harness already exists for `AppState` views (check for
  existing `HealthView` tests first; spec A's plan may already establish
  this pattern, in which case reuse it exactly rather than inventing a
  second harness).
- A test confirming `Up`/`Down` on `ActiveAgentsView` moves
  `selected_agent_idx` within bounds and does not panic on an empty
  `active_agents` list (`0..0` range).
- A test confirming `/memory` and `/mempalace` produce identical resulting
  `self.state` and identical build-trigger behavior (same code path, not a
  parallel copy).

## Open Question for the Implementation Plan

The exact file naming convention for the new keyboard-handler files depends
on what spec A's plan settles on first (spec A is the template this spec
follows) — if spec A and B are planned/executed in parallel rather than
sequentially, this spec's plan must independently confirm the convention
against the real, already-existing `app/events/keyboard/health.rs` rather
than assuming spec A's naming choices.
