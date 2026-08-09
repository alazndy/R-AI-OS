# TUI Legacy Dashboard Migration — Design Spec

**Date:** 2026-08-09
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira
**Sub-project:** A of 3 (TUI cleanup — see sibling specs B, C)

## Context

A general audit of `raios-surface-tui` found two parallel surfaces sharing one
`App` struct: the 4 typed control-plane routes (`Route::Now/Work/Explore/Govern`,
`app/route.rs:7-17`, all real, all backed by authenticated daemon commands via
`Client::send_command`/`send_query`), and a pre-redesign "legacy" 16-item
`menu_cursor`-driven dashboard (`app/events/keyboard/dashboard.rs:77-561`, ~470
lines).

The legacy surface is unreachable from normal keyboard play:
`handle_dashboard_key` (`dashboard.rs:77-88`) runs the 4-route handler first,
and if that doesn't consume the key, guards everything else down to only `q`,
`?`, `/`. The ~470-line match arm below — Constitution editor (item-editing,
a creator wizard, a global-write y/N gate), Extensions panel (tab/edit/lazy-load),
and legacy per-agent task dispatch (`c`/`x`/`o`/`a` → claude/codex/opencode/agy)
— is dead on the keyboard path. It remains reachable only through five slash
commands (`/rules`, `/ext`, plus three covered by sibling spec B), which set
`self.ui.menu_cursor`/`right_panel_focus` — but `render_dashboard`
(`ui/panels/dashboard_main.rs:13-29`) unconditionally renders only
`render_route_view` for content and never consults `menu_cursor`. So even the
slash-command entry points produce no visible panel.

Verified before scoping this migration: the legacy task-dispatch keys
(`c`/`x`/`o`/`a`, `menu_cursor == 0`) are redundant, not just unreachable.
`Command::LaunchAgent` already exists in the real typed contract
(`raios-contracts/src/command.rs:135`) and NOW route's real "LaunchCodexAgent"
next-action (`app/operations.rs:42,128`) already uses it. This part gets
deleted, not migrated.

## Scope

- **Migrate**: the Constitution editor (`menu_cursor == 1` and its associated
  `self.constitution.*` state — item editing, creator wizard, global-write
  confirmation gate) and the Extensions panel (`menu_cursor == 15` and
  `self.ext.*` state — tab handling, inline editing, lazy-load).
- **Delete**: legacy task-dispatch (`menu_cursor == 0`'s `c`/`x`/`o`/`a` keys),
  the generic 16-item menu navigation (`Up`/`Down`/`Left`/`Right` cycling
  through `menu_cursor` 0-15), and `menu_cursor`/`right_panel_focus` fields
  themselves once nothing references them.
- **Explicitly out of scope**: sibling specs B (slash-command routing to
  EXPLORE/FileView) and C (Ocak panel discoverability) — independent
  sub-projects, each gets its own plan/SDD cycle.

## Architecture

Two new `AppState` variants, `ConstitutionView` and `ExtensionsView`, added
next to the existing `HealthView`/`MemPalaceView` (`app/state.rs:119,123`) —
the same established pattern: opened via a slash command (`/rules`, `/ext`)
setting `self.state`, closed via `Esc` back to the previous route, with their
own dedicated render function and key-handler function each, registered at
the same three sites `HealthView`/`MemPalaceView` already use
(`ui/mod.rs`, `app/events/commands.rs`, `app/events/keyboard/mod.rs`).

The Constitution and Extensions state structs (`self.constitution.*`,
`self.ext.*`) and their editing logic move unchanged — this is a relocation
of working logic behind a new entry/exit mechanism, not a rewrite. No new
daemon calls, no behavior change to the editing flows themselves.

## Components

| Component | Change |
|---|---|
| `app/state.rs` | Add `AppState::ConstitutionView`, `AppState::ExtensionsView`. Remove `menu_cursor: usize`, `right_panel_focus: bool` fields once nothing references them (verify via compiler warnings, not by inspection alone). |
| `app/events/keyboard/dashboard.rs` | Delete the ~470-line legacy match arm (`:77-561`) except: the outer `q`/`?`/`/` handling stays (still needed), and the actual Constitution/Extensions key-handling logic (item editing, creator wizard, tab/edit/lazy-load) is *moved*, not deleted, into two new files. |
| New: `app/events/keyboard/constitution_view.rs` (or similarly named, matching whatever convention `health_view.rs`/`mempalace_view.rs`-equivalent files already use — verify the real existing file name before creating) | Key handler for `AppState::ConstitutionView`, containing the moved item-editing/creator-wizard/global-write-gate logic verbatim. |
| New: `app/events/keyboard/extensions_view.rs` (same naming-convention caveat) | Key handler for `AppState::ExtensionsView`, containing the moved tab/edit/lazy-load logic verbatim. |
| New: `ui/routes/constitution_view.rs` / `ui/routes/extensions_view.rs` (or wherever `health_view`'s render lives — match its real location) | Render functions for the two new views, moved from whatever rendered them before they went dead (if a render implementation still exists dormant somewhere; if none exists, build minimally from the state shape, matching `HealthView`'s visual style). |
| `app/events/commands.rs` | `/rules` and `/ext` arms now set `self.state = AppState::ConstitutionView` / `ExtensionsView` directly, instead of `menu_cursor`/`right_panel_focus`. |
| `app/events/keyboard/dashboard.rs` (task-dispatch removal) | Delete the `menu_cursor == 0`, `c`/`x`/`o`/`a` dispatch block entirely — confirmed redundant with `Command::LaunchAgent`. |

## Data Flow

Unchanged from today's (currently unreachable) behavior: this is local UI
state only. Opening `/rules` or `/ext` sets `self.state`; the render loop
dispatches on it to the new dedicated render function; key events dispatch to
the new dedicated handler; `Esc` sets `self.state` back to
`AppState::Dashboard` (or whatever the return-to-normal variant is named —
verify against `HealthView`'s own `Esc` handling). No daemon round-trip
differs from before.

## Error Handling

Unchanged — the moved logic's existing validation (e.g. the Constitution
editor's global-write y/N confirmation gate) carries over verbatim.

## Testing

- Render + key-handler tests for both new views, shaped like whatever
  existing tests cover `HealthView` (find and mirror that file's test
  structure).
- A compile-time check that removing `menu_cursor`/`right_panel_focus`
  produces no dangling references — the implementation plan should treat any
  leftover reference as a signal that something wasn't fully migrated, not
  something to silently `#[allow(dead_code)]`.
- A test confirming the legacy task-dispatch keys are gone (i.e., pressing
  `c`/`x`/`o`/`a` outside of any text-editing context is a no-op) — protects
  against reintroducing dead paths.

## Open Question for the Implementation Plan

The exact file/function names for `HealthView`'s render and key-handler
implementations were not verified line-by-line during brainstorming (only
their existence and general location were confirmed). The implementation
plan must read those real files first and name the new Constitution/Extensions
files and functions to match that established convention exactly — this spec
describes the shape, not the literal file names.
