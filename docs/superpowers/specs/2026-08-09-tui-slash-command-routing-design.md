# TUI Slash-Command Routing to Existing Real Panels — Design Spec

**Date:** 2026-08-09
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira
**Sub-project:** B of 3 (TUI cleanup — see sibling specs A, C)

## Context

Four slash commands in the command palette (`app/events/commands.rs`) produce
no visible effect, but for two different reasons discovered during
brainstorming — not uniformly "dead":

- **`/search` (`commands.rs:126-146`)** and **`/logs` (`commands.rs:183-196`)**
  perform *real* backend work — `/search` runs a local index search or sends
  `daemon_search_command` for remote hubs; `/logs` sends a real
  `daemon_get_logs_command` request to replay history from the daemon. But
  both write their results into a legacy, parallel local state
  (`self.search.*`, a `SearchState` struct at `app/state.rs:610`) that no
  render path reads. Meanwhile, the real EXPLORE route (`ui/routes/explore.rs`)
  already has working search and logs panels — its search box reads/writes
  `store.explore_search.{is_editing,query}` (confirmed at `explore.rs:53-81`)
  and submits via the same `daemon_search_command` path; its logs panel
  renders `store.snapshot.explore.recent_logs`, populated reactively from
  daemon-pushed snapshots. Two parallel, non-communicating systems exist for
  the same capability — one visible and real, one invisible and real.
- **`/timeline` (`commands.rs:179-182`)** only sets `menu_cursor`/
  `right_panel_focus` — no real side effect at all. EXPLORE's trace-timeline
  panel (`store.snapshot.explore.recent_traces`) is the obvious real
  destination by naming and purpose.
- **`/memory` (`commands.rs:94-98`)** only sets `menu_cursor`/
  `right_panel_focus`/`right_file_cursor` — no real side effect. `/view` and
  `/edit` (`commands.rs:113-124`) are real, working commands that open a file
  via `find_file_by_name` + `open_file_view`/`open_file_edit` into the
  existing `FileView`/`FileEdit` `AppState` variants. `/memory`'s naming and
  the `right_file_cursor` hint both point at "open a project's `memory.md`" —
  the same mechanism `/view`/`/edit` already use, just pre-scoped to that
  filename.

## Scope

- **Rewire `/search`, `/logs`, `/timeline`**: switch to `Route::Explore` and
  drive it through its own real, existing state (`store.explore_search`,
  the snapshot-sourced panels) instead of the dead parallel `self.search.*`
  system.
- **Remove** the now-fully-unused `SearchState` struct and its field once
  nothing references it.
- **Rewire `/memory [project]`**: reuse `find_file_by_name` +
  `open_file_view`, scoped to the target project's `memory.md`.
- **Explicitly out of scope**: sibling specs A (Constitution/Extensions
  migration — `/rules`, `/ext` are covered there, not here) and C (Ocak
  discoverability).

## Architecture

No new state, no new daemon calls, no new panels. This sub-project's entire
job is deleting a dead parallel implementation and redirecting four command
entry points at infrastructure that already exists and already works —
`Route::Explore` plus its established `store.explore_search`/
`store.snapshot.explore.*` data path, and the existing `FileView` open
mechanism.

## Components

| Component | Change |
|---|---|
| `app/events/commands.rs` `/search` arm | On non-empty `arg`: set `self.route = Route::Explore`, set `store.explore_search.query = arg.to_string()`, `store.explore_search.is_editing = false` (already submitted, not mid-edit), then invoke whatever function EXPLORE's own Enter-in-edit-mode handler calls to submit (verify its exact name in `ui/routes/explore.rs`'s key handler before writing this) — do not duplicate that submit logic, call the same function. |
| `app/events/commands.rs` `/logs` arm | Keep the existing `daemon_get_logs_command` send unchanged (it already works) — add `self.route = Route::Explore` so the user actually sees `recent_logs` update. |
| `app/events/commands.rs` `/timeline` arm | Replace the `menu_cursor`/`right_panel_focus` mutation with `self.route = Route::Explore` (trace panel is passively populated from the snapshot already — no further action needed). |
| `app/events/commands.rs` `/memory` arm | Replace with a call to `find_file_by_name("memory.md", ...)` scoped to the given project argument (or the currently-selected project if `arg` is empty — verify against `/view`'s exact argument-handling convention) followed by `open_file_view`. |
| `app/state.rs` | Remove `SearchState` struct and `self.search` field once the migration leaves no reference to it. |

## Data Flow

`/search`/`/logs`/`/timeline`: command palette input → route switch to
Explore → EXPLORE's existing real data path (unchanged) → existing real
render (unchanged). `/memory`: command palette input → `find_file_by_name` →
`open_file_view` → existing `FileView` render (unchanged).

## Error Handling

`/memory` on a project with no `memory.md`: match `/view`'s existing
not-found behavior exactly (verify what that is — likely a `sync_status`
message, matching the pattern already used elsewhere in `commands.rs`) rather
than inventing new error UX.

## Testing

- Unit tests confirming each of the four commands sets `self.route` (and,
  for `/search`/`/memory`, the correct downstream state) correctly.
- A test confirming `SearchState`/`self.search` has no remaining references
  after removal (compile-time signal, not a runtime test).
- A regression test that EXPLORE's own native search/logs/timeline behavior
  (reached via the EXPLORE route's own UI, not the slash command) is
  unaffected — this sub-project must not change EXPLORE's own working code
  path, only add new entry points into it.

## Open Question for the Implementation Plan

The exact function EXPLORE's Enter-in-edit-mode key handler calls to submit
a search (referenced above as "whatever function... invoke") was not named
during brainstorming — only its existence and the state fields it reads/writes
were confirmed. The implementation plan must read `ui/routes/explore.rs`'s
key-handling code (likely in `app/events/keyboard/` under an explore-specific
file) and name it exactly before writing this task.
