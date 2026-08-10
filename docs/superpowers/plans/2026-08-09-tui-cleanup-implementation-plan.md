# TUI Cleanup Implementation Plan

**Date:** 2026-08-09
**Status:** Approved for execution
**Specs:** `2026-08-09-tui-legacy-dashboard-migration-design.md`, `2026-08-09-tui-slash-command-routing-design.md`, `2026-08-09-tui-ocak-discoverability-design.md`

## Scope and sequencing

Sub-projects A and B share the retired dashboard handler at
`crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs:77-561`.
They will therefore land as one atomic migration before sub-project C. The
legacy UI fields can only be removed after deleting their dormant render and
file-navigation consumers; merely moving the keyboard branches would leave
the retired state reachable in code.

## Task 1 — Establish view and control-plane skeletons

1. In `app/state.rs:105-130`, add `ConstitutionView`, `ExtensionsView`,
   `TasksView`, `SearchView`, `ActiveAgentsView`, and `TimelineView` to
   `AppState`; remove `UIState::{menu_cursor,right_panel_focus,right_file_cursor,right_panel_scroll}`
   at `:845-854` once their consumers are deleted.
2. In `app/store.rs:11-61`, add `WorkFocus::{Projects,Ocak,Tasks}` and a
   `work_focus` field. This is needed because the existing Boolean only models
   Projects/Tasks and cannot identify the new selectable Ocak panel.
3. Add keyboard module declarations and empty view-handler methods under
   `app/events/keyboard/mod.rs:8-18`; add corresponding `AppState` dispatch
   arms in `:88-132`.
4. Add render dispatch arms in `ui/mod.rs:67-81` using the existing panel
   signatures: `render_constitution`, `render_extensions`,
   `render_tasks_view`, `render_search_panel`, `render_logs`, and
   `render_timeline`.

## Task 2 — Migrate the six view entry points and interactions

1. Route `/rules`, `/ext`, `/tasks`, `/search`, `/logs`, and `/timeline` in
   `app/events/commands.rs:88-185,355-360` to their dedicated view states.
   Preserve `/search` query execution and `/logs` refresh transport.
2. Make `/memory` execute the exact `/mempalace` lazy-build/state path in
   `commands.rs:94-113`.
3. Move Constitution interaction from `keyboard/dashboard.rs:187-361` into
   `keyboard/constitution_view.rs`; preserve edit/creator modal precedence
   and return to `Dashboard` only when no nested editor/creator is active.
4. Move Extensions interaction from `dashboard.rs:117-186` into
   `keyboard/extensions_view.rs`, retaining masking and config save behavior.
5. Add `keyboard/tasks_view.rs` for local markdown task movement and
   completion persistence only; intentionally do not retain c/x/o/a dispatch.
6. Add `keyboard/search_view.rs`, `active_agents_view.rs`, and
   `timeline_view.rs`. Search retains result-open behavior; agent navigation
   clamps `selected_agent_idx`; timeline is read-only.

## Task 3 — Remove the retired dashboard surface

1. Reduce `keyboard/dashboard.rs` to command-palette, help, quit, Git-diff,
   and the live `handle_control_dashboard_key` route handler. Delete the
   16-item menu, task dispatch, Projects launcher/opening, file navigation,
   and generic menu cursor branches.
2. Delete dead menu-dependent code in `ui/panels/content.rs`,
   `ui/filebrowser.rs::render_file_panel`, `ui/projects.rs::render_projects`,
   `app/mod.rs::current_menu_files`, and legacy-only action helpers in
   `app/events/actions.rs:597-626`; remove their module exports/imports.
3. Simplify the launcher footer in `ui/components.rs:143-175`, and replace
   the removed audit-tab cursor mutation in `app/events/bg_messages.rs:102-106`
   with a status update.

## Task 4 — Add WORK project sorting and Ocak discoverability

1. Move former Projects sort cycling to the live WORK key path in
   `keyboard/dashboard.rs::handle_control_dashboard_key`, using the existing
   `self.projects.sort.next()` and reset its cursor.
2. Extend `app/control_navigation.rs:15-89` for three `WorkFocus` targets,
   bounded selection counts, cursor syncing, and left/right focus movement.
3. In `ui/routes/work.rs:37-253`, render six individual selectable Ocak
   summary lines and visual selection. Map them through a pure
   `ocak_command_prefix(usize) -> &'static str` helper to
   `/ocak product `, `/ocak cycle `, `/ocak change `, `/ocak support `,
   `/ocak quality `, and `/ocak release `; every prefix is accepted by
   `factory_command_from_input` in `app/events/commands.rs:380-552`.
4. On Enter while `WorkFocus::Ocak` is selected, set
   `ui.command_mode = true`, write the prefix to `ui.command_buf`, and reset
   `ui.palette_cursor`. Do not call `execute_command` or send a factory
   command.

## Task 5 — TDD and verification

1. Add unit tests for `WorkFocus` navigation, clamping, prefix mapping, and
   no-auto-submit palette opening in `control_navigation.rs`.
2. Add handler tests for active-agent bounds, view close behavior, and local
   task completion persistence.
3. Extend `ui/routes/tests.rs` with selected-Ocak rendering, and add
   AppState-view render coverage using `TestBackend`.
4. Run `cargo fmt --all -- --check`, `cargo test --workspace`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
   `raios security`, `raios deps`, regenerate `SIGMAP.md`, update
   `README.md` and `memory.md`, then run `raios pre-flight` before commit.
