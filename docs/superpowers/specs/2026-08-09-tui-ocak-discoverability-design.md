# TUI Ocak (Product Factory) Discoverability — Design Spec

**Date:** 2026-08-09
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira
**Sub-project:** C of 3 (TUI cleanup — see sibling specs A, B)

## Context

The Product Factory ("Ocak") command grammar (`factory_command_from_input`,
covering 30+ subcommands across workspace/product/intake/charter/
requirement/change/cycle/stage/quality/release/support entities) is fully
implemented, unit-tested, and dispatched via `Client::send_factory_command`
— entirely real. But it is only reachable as a raw `/ocak <verb> <args>`
string typed into the command palette; the WORK route's Ocak panel
(`ui/routes/work.rs`) is a read-only projection of
`snapshot.work.factory: FactoryOverviewSnapshot` with no interaction bound
to it.

`FactoryOverviewSnapshot` (`raios-contracts/src/factory.rs:540+`) is a set of
aggregate counters — `product_count`, `active_cycle_count`,
`pending_change_request_count`, `open_support_items`,
`blocking_quality_profiles`, plus draft-releases-awaiting-approval — not a
single-entity state machine. Unlike NOW's `OperationsConsole` (one
prioritized list of next actions across the whole system), Ocak tracks
multiple products, cycles, and change requests potentially at different
stages simultaneously — there is no single "next valid action" the way NOW
has one. Building a full command-grammar-as-buttons UI for 30+ subcommands
across this multi-entity structure was considered and explicitly rejected as
disproportionate scope for this cleanup pass.

## Scope

- **In scope**: make the WORK route's Ocak summary lines selectable/
  navigable, and let selecting one pre-fill the command palette with the
  `/ocak <verb>` prefix relevant to that line, so the user can find the
  right command without memorizing the grammar.
- **Explicitly out of scope**: exposing the full command grammar as direct
  keybindings/buttons; any new daemon calls or new aggregate data the
  snapshot doesn't already carry; sibling specs A and B.

## Architecture

Purely a navigation + command-palette-prefill feature — no new state beyond
a selection cursor on the Ocak panel (mirroring the existing project/task
list cursor pattern already used elsewhere in WORK), and no new daemon
round-trip. Selecting a line and pressing `Enter` opens the command palette
(the same one `/`/`Tab` already opens) with its input pre-populated by a
verb-prefix string; the user still reviews and submits it themselves —
this sub-project never auto-submits a Factory command on the user's behalf.

## Components

| Component | Change |
|---|---|
| WORK route's Ocak panel render (`ui/routes/work.rs`) | Add cursor/selection rendering to the summary lines, matching the existing project-list/task-list selection visual style in the same route. |
| WORK route's Ocak panel key handling (wherever `dashboard.rs`/`mouse.rs` currently handle WORK route input) | Add `↑`/`↓`/`k`/`j` (or reuse the existing focus-cycle key if the panel joins the route's existing focus rotation) to move the Ocak cursor; `Enter` on a selected line opens the command palette pre-filled. |
| New: a small mapping function, e.g. `ocak_command_prefix_for_summary_line(line: OcakSummaryLine) -> &'static str` (naming illustrative — implementation plan verifies against real code) | Maps each of the 6 summary fields (`product_count`, `active_cycle_count`, `pending_change_request_count`, `open_support_items`, `blocking_quality_profiles`, draft-releases) to its corresponding `/ocak <verb>` prefix — verify each prefix against the real `factory_command_from_input` grammar (`app/events/commands.rs`) rather than guessing subcommand names. |

## Data Flow

Selection is local UI state. `Enter` on a selected line calls the same
command-palette-open mechanism `/`/`Tab` already use, passing a pre-filled
string instead of an empty one — no new code path into the daemon; the
actual command submission still goes through the existing, already-tested
`/ocak` dispatch when the user presses their own Enter in the palette.

## Error Handling

None new — this sub-project never submits a command itself, so it inherits
whatever validation `factory_command_from_input` already performs when the
user eventually submits.

## Testing

- A test per summary line confirming it maps to the correct `/ocak` verb
  prefix (verified against the real grammar, not assumed).
- A test confirming `Enter` on a selected line opens the palette with that
  prefix and does *not* auto-submit.

## Open Question for the Implementation Plan

The exact subcommand verb for each of the 6 summary lines (e.g. whether
`pending_change_request_count` maps to `/ocak change list` or a differently
named verb) was not verified against `factory_command_from_input`'s real
grammar during brainstorming. The implementation plan must read that
function's real accepted verbs before writing the mapping table.
