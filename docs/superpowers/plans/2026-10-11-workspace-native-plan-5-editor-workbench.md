# Workspace Native — Plan 5: Editor, File Tree and Workbench Shell (P3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **Tasks 0-10 are for the executor. Task 11 is run by the planner or the owner on the reference machine, never by an unattended executor.**

**Status: DRAFT for owner review.** Nothing in this plan has been executed except the pure code of Task 1, which was compiled and tested before it was written into this document (21 tests, clippy clean).

**Goal:** Turn the native workspace from a terminal view into an application: one window (`rwk-app`) with a project folder picker, a lazy file tree, a code editor (soft wrap off, line numbers, folding, syntax highlighting, revision-checked saves, a read-only Git diff view), up to four terminal sessions in a dock, a typed command catalog with palette and exact-modifier shortcuts, and guards that stop unsaved edits from being lost. It is the first build a person can actually use.

**Architecture:** Toolkit-neutral logic stays in `rwk-core::workbench` (command catalog, shortcut resolver, command availability, open-document session, appearance parsing) and is tested without a window. `rwk-ui` gains three entities on top of `gpui-base` 0.7.1 (`EditorState`/`Editor`, `TreeState`/`Tree`, `h_resizable`): `EditorView`, `FileTreeView`, `Workbench`, and the syntax highlighter adapter. `rwk-app` is the thin binary that owns the window, the folder picker and the wiring. Terminals reuse `TerminalView` and `PtyTerminalHost` from Plans 4 and 4b unchanged.

**Tech Stack:** Rust 1.96.0; `gpui-pre =0.3.8` and `gpui-base =0.7.1` (both in the supply-chain-verified family, owner decision A of 2026-10-06); `syntect` with the pure-Rust `fancy-regex` feature for highlighting. No `gpui-component`, no tree-sitter.

**Spec:** `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`, section 7 (editor, workbench, views), section 8 (state; inspector and handoff are P4), section 9 (gates), section 10 (P3). Builds on Plan 4b (`feat/workspace-native-p2b`).

## Plan series

| Plan | Phase | Status |
|---|---|---|
| 1-3b | P0 daemon contract, P1 `rwk-core` | done |
| 4 | P2a terminal engine + headless gates | done; G8 known risk |
| 4b | P2b terminal view + UI gates | built; reference-machine run (its Task 10) pending |
| **5 (this)** | P3 editor, file tree, workbench shell, commands, keyboard, app binary | draft |
| 6 | P4 inspector, handoff composer, consent dialog | after Plan 5 |
| 7 | P5 memory view, LCARS skin, icons, parity check, removal of the Tauri version | after Plan 6 |

## Why this plan is written before Plan 4b's Task 10

The spec's P2 stop criterion says that if the terminal gates fail, the project reassesses before P3. Plan 4b's gate run (its Task 10) could not be taken on the executor's machine (the compositor delivered no frames) and has not been run on the reference machine yet. **The owner chose on 2026-10-11 to continue with P3 planning and implementation regardless.** The risk is stated here and carried: if Plan 4b's Task 10 later fails U1-U5, Tasks 5-9 of this plan (everything that renders) may be thrown away or redone on another toolkit; Task 1 (pure workbench logic) and the pure parts of Tasks 3 and 8 survive any toolkit. The plan therefore front-loads the toolkit-independent work and puts the first window-dependent task behind a probe (Task 2) that can be run on the reference machine at once.

## Decisions in this plan (owner review before execution)

The plan implements the recommendation of each decision; say so if you want another option.

| # | Decision | Recommendation (implemented) | Alternative |
|---|---|---|---|
| E1 | Editor component | `gpui_base::input::{EditorState, Editor}` with `soft_wrap(false)`, line numbers and folding on, as the spec's spike measured (30K lines 272 ms, 500K 579 ms without wrap) | Write a custom editor element: months of work |
| E2 | Highlighting | Implement `gpui_base`'s `InputHighlighter` trait over `syntect` (`default-fancy`, pure Rust, languages loaded lazily): the crate ships no highlighter of its own (tree-sitter lives in `gpui-component`, which the spec rules out). Highlight only files up to 1 MiB; larger files show plain text with a visible note | tree-sitter grammars via `gpui-component`: pulls the fragmented component crate |
| E3 | File tree | `gpui_base::tree::{Tree, TreeState, TreeItem}`; children are listed lazily through `ProjectRoot::list` when a folder expands; entries sort directories first, then names, case-insensitively. Breeze icons and the file-type classifier are P5 | Own virtual list: duplicates arrow-key and focus handling the component already has |
| E4 | Layout | `h_resizable` panel group (files / editor / dock) with the component's resize handle; evaluate `DockArea` in Task 2 and keep the flex split unless `DockArea` is clearly simpler | `DockArea` from the start: more machinery than four fixed panes need |
| E5 | Open project | `cx.prompt_for_paths` (directories only) through the XDG portal; the chosen path goes through `ProjectRoot::open`, which canonicalizes it and holds the capability. The UI never sends a client-built root path to the daemon | A text field for a path: invites traversal mistakes |
| E6 | Strings | Turkish UI text as in Workspace 0.1 (labels, reasons, dialogs); code, identifiers and docs stay English | English UI: breaks 0.1 parity |
| E7 | Appearance | Only `{skin: standard, tone: dark|light}` is wired in P3 (editor style via `set_editor_style`, terminal palette swap); the `lcars` value parses and persists but renders as standard until P5. Stored as JSON under `$XDG_CONFIG_HOME/raios-workspace-native/appearance.json`, validated by `parse_appearance` | Skip appearance until P5: tone switching is part of the 0.1 feature set |
| E8 | Heavy I/O | File reads, saves, directory listings and `git diff` run on the background executor; results come back through weak entity handles with a project generation stamp, and a result for an older generation is dropped (spec section 8) | Run on the UI thread: a slow disk would freeze typing |
| E9 | Editor-open gate | Measured here (moved from Plan 4b): E1 30K lines and E2 500K lines against `EditorState` directly. 500K lines exceed the product's 2 MiB document limit, so E2 is a capability gate of the component, not a statement about product files | Drop E2: loses the spike's headroom evidence |
| E10 | Agent and handoff commands | Present in the catalog, disabled with the reason "Bu sürümde henüz yok" until P4 (`WorkbenchState::control_plane_features`) | Hide them: the palette would differ from 0.1 |

## Public API (the skeleton for review)

```text
rwk_core::workbench                                   // pure, no toolkit (Task 1, already written and tested)
  CommandId { Commands, Project, Files, Save, Diff, NewTerminal, ToggleTerminal, FocusEditor, FocusFiles,
              NextTerminal, PreviousTerminal, FocusTerminal(u8), ClearTerminal, CopyTerminal, PasteTerminal,
              StopTerminal, Agent, Handoff, Memory, Refresh, Settings, Shortcuts, Balanced, EditorLayout, AgentsLayout }
  catalog() -> &'static [Command]                     // 28 commands: id, Turkish label, shortcut text, bindings
  resolve_shortcut(&Keystroke, KeyboardContext, consume_repeat: bool) -> Option<CommandId>
  available(CommandId, &WorkbenchState) -> CommandState { enabled, reason }
  DocumentSession { request_open, request_leave, opened, edited, begin_save, save_succeeded, save_failed, close, ... }
  Appearance { skin: Skin, tone: Tone }; parse_appearance(Option<&str>) -> Appearance; serialize_appearance

rwk_ui::editor
  SyntectHighlighter: gpui_base InputHighlighter                       // Task 3
  highlighter_factory() -> InputHighlighterFactory                     // chooses by file extension, None above 1 MiB
  EditorView::new(window, cx) -> Self; open(Document, cx); text(cx) -> String; show_diff(String, cx); save(ProjectRoot, cx)
  EditorEvent { Edited, Saved(Document), SaveFailed(AppError), Conflict }

rwk_ui::file_tree
  FileTreeView::new(cx); set_project(ProjectRoot, cx); refresh(cx)
  FileTreeEvent { OpenFile(String) }                                   // relative path, after the guard allowed it

rwk_ui::workbench
  Workbench::new(window, cx) -> Self                                   // owns panes, dock, session registry, DocumentSession
  Workbench::run(CommandId, window, cx)                                // the single dispatch path for buttons, shortcuts, palette
  Workbench::state(cx) -> WorkbenchState                               // feeds `available`
  CommandPalette, ConfirmDialog (unsaved changes, stop session), Toasts

rwk_app (binary `rwk-app`)
  main(): application, window, folder picker, appearance persistence, wiring
```

## Gates

| Id | Gate | Limit | Spec source |
|---|---|---|---|
| E1 | editor open, 30K-line file (`EditorState` set_value to first painted frame) | <= 350 ms | editor open |
| E2 | editor open, 500K-line buffer (capability gate, see E9) | <= 700 ms | editor open |
| A1 | `rwk-app` spawn to first painted frame (empty window) | <= 300 ms | cold start |
| A2 | idle PSS with a project open, one file open and 4 live terminals, 10 s after the last output | <= 150 MB | idle PSS |

Same rule as Plans 4 and 4b: the harness measures other processes' CPU use, a failure on a busy machine is INCONCLUSIVE (exit 3), a gate that cannot be measured (no frames delivered to the window) is NOT MEASURED and also exit 3, and a pass always counts.

## Global Constraints

- Rust toolchain 1.96.0, edition 2021. `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` clean for `tools/raios-workspace-native`.
- GPUI family pins: `gpui = { package = "gpui-pre", version = "=0.3.8" }`, `gpui_base = { package = "gpui-base", version = "=0.7.1" }`. Adding or changing any pinned crate re-runs `scripts/verify_gpui_supply_chain.py` against the resulting `Cargo.lock` before the commit. `syntect` and its dependencies are new third-party crates: run `cargo audit` and report every new advisory to the owner; never silence one.
- **Dependency boundary:** `rwk-core` keeps its direct dependency list (no `gpui*`, `wgpu`, `tokio`, `raios-core`, `raios-runtime`, `raios-surface-*`, `tauri`; `syntect` lives in `rwk-ui`). `rwk-ui` may add `gpui_base` and `syntect`. `rwk-app` depends on `rwk-ui`, `rwk-core`, `gpui`, `gpui_platform`.
- No `unwrap()`/`expect()` in non-test code; every public item has a doc comment; every `unsafe` block has a `// SAFETY:` comment.
- File content, terminal content and clipboard content never enter a log. Saves go through `ProjectRoot::save_document` only (revision-checked, atomic, 2 MiB, UTF-8). The UI never writes a file by another path.
- Unsaved edits are never discarded without an explicit user choice: switching file, switching project, closing the window and reload-after-conflict all pass through `DocumentSession::request_open` / `request_leave`.
- Heavy I/O never runs on the UI thread (decision E8). A result carries the project generation it was started for and is dropped when the generation moved on.
- Soft wrap stays off in every editor instance.
- Commits require the owner's explicit go-ahead; steps marked **(OWNER GO-AHEAD)** must not run without it. Commit trailer: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` only when Claude Sonnet 5.5 executes this plan; any other executor writes its own accurate line.
- Shell commands avoid pipes, `cd`, `tee`, `sed` and `awk`: run cargo from the repository root with `--manifest-path tools/raios-workspace-native/Cargo.toml`, use `-q`. Put `CARGO_TARGET_DIR` on a disk with room (the GPUI build is several GB; `/tmp` is a small tmpfs on the reference machine). Window tests need a compositor that delivers frame callbacks and run only with `RWK_UI_DISPLAY_TESTS=1`.
- **Long builds:** give each cargo command the largest timeout the shell tool allows; if cut off, run it again.

## Review Focus

Failure modes most likely to bite a person using the workspace, each with a pinned test:

1. **Lost edits.** Switching file, switching project, closing the window, a failed save and a reload after a conflict must never discard dirty text without a choice (`a_clean_file_can_be_left_and_a_dirty_one_needs_a_decision`, `a_conflict_keeps_the_edits_and_blocks_saving_until_reload`, `edits_made_while_a_save_runs_keep_the_file_dirty`; window-level: `closing_with_unsaved_edits_asks_first`).
2. **Keys going to the wrong place.** Ctrl+S saves in the editor but reaches the shell in a terminal; Ctrl+C, Ctrl+K, Ctrl+L, Tab and arrows are never swallowed by the workbench while a terminal has focus; AltGr and IME composition never run commands (`plain_ctrl_keys_reach_the_shell_when_a_terminal_has_focus`, `composition_altgr_dialogs_and_repeats_never_run_commands`).
3. **The file changed on disk.** A save over a changed file shows a conflict and keeps the edits (`a_conflict_keeps_the_edits_and_blocks_saving_until_reload`, `a_save_over_a_changed_file_reports_a_conflict_and_keeps_the_text`).
4. **A file that cannot be edited.** Binary, non-UTF-8, oversized (> 2 MiB) and directory targets show a clear message and leave the current editor untouched (`opening_a_binary_file_shows_a_message_and_keeps_the_open_file`).
5. **Switching projects quickly.** A directory listing, file read or diff that finishes after the user moved to another project must not appear in the new project's tree or editor (`a_listing_from_the_previous_project_is_dropped`).
6. **Huge or hostile files.** A 1.5 MiB file opens without freezing the UI (highlighting off, a note shown); a very long single line does not hang layout (`a_file_above_the_highlight_limit_opens_unhighlighted`, `a_very_long_line_opens_within_the_editor_budget`).
7. **Terminal count and shutdown.** The fifth terminal is refused with a reason; closing the window stops owned sessions, giving agent wrappers up to 5 s (`the_fifth_terminal_is_refused_with_a_reason`, `closing_the_window_stops_owned_sessions`).

---

## File Structure

| File | Action | Task |
|---|---|---|
| `tools/raios-workspace-native/crates/rwk-core/src/workbench/{mod,commands,documents,appearance}.rs` | create | 1 |
| `.../rwk-core/src/lib.rs` | modify (`pub mod workbench;`) | 1 |
| `tools/raios-workspace-native/Cargo.toml`, `Cargo.lock` | modify (members, deps) | 2, 8 |
| `.../crates/rwk-ui/Cargo.toml` | modify (`gpui_base`, `syntect`) | 2, 3 |
| `.../crates/rwk-ui/tests/gpui_base_probe.rs` | create | 2 |
| `.../crates/rwk-ui/src/editor/{mod,highlight,view}.rs` | create | 3, 4 |
| `.../crates/rwk-ui/src/file_tree/{mod,view}.rs` | create | 5 |
| `.../crates/rwk-ui/src/workbench/{mod,layout,dock,palette,dialogs,state}.rs` | create | 6, 7 |
| `.../crates/rwk-ui/tests/{editor_headless,tree_headless,workbench_headless}.rs` | create | 3-7 |
| `.../crates/rwk-app/{Cargo.toml,src/main.rs,src/persistence.rs}` | create | 8 |
| `.../crates/rwk-ui-bench/src/{gates,child,main}.rs` | modify (E1, E2, A1, A2) | 9 |
| `CHANGELOG.md`, `memory.md`, `tools/raios-workspace-native/README.md` | modify | 10 |
| `docs/superpowers/reports/2026-10-workbench-gates.md` | create | 11 |

---

### Task 0: Worktree and baseline

**Files:** none changed.

- [ ] **Step 1: Branch from Plan 4b's result**

```bash
git worktree add .worktrees/workspace-native-p3 -b feat/workspace-native-p3 feat/workspace-native-p2b
```

All following commands run with that worktree as the repository root.

- [ ] **Step 2: Baseline**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml
cargo clippy -q --all-targets --manifest-path tools/raios-workspace-native/Cargo.toml -- -D warnings
```

Expected: all tests pass (219 at the end of Plan 4b), no warnings. Stop on a red baseline.

---

### Task 1: Workbench logic in `rwk-core` (pure; already compiled and tested)

The four files below were written, compiled and run before this plan was committed: 21 tests pass, `cargo clippy -D warnings` is clean. The executor still follows red-green: create the test modules first (they are inside each file), see the compile error for the missing module, then add the code.

**Files:**
- Create: `crates/rwk-core/src/workbench/mod.rs`, `commands.rs`, `documents.rs`, `appearance.rs`
- Modify: `crates/rwk-core/src/lib.rs` (add `pub mod workbench;` after `pub mod terminal;`)

**Interfaces:**
- Consumes: `Document`, `Revision` (`rwk_core::documents`), `ProjectId` (`project`), `AppError`, `ErrorCode` (`error`).
- Produces: everything listed under `rwk_core::workbench` in the API skeleton.

- [ ] **Step 1: `src/workbench/mod.rs`**

```rust
//! Workbench logic that does not depend on a toolkit: the command catalog, shortcut resolution,
//! command availability and the open-document session.

pub mod appearance;
pub mod commands;
pub mod documents;

pub use appearance::{parse_appearance, serialize_appearance, Appearance, Skin, Tone};
pub use commands::{
    available, catalog, resolve_shortcut, Binding, Command, CommandId, CommandState,
    KeyboardContext, Keystroke, WorkbenchState,
};
pub use documents::{DocumentSession, SaveBlocked, SwitchDecision};
```

- [ ] **Step 2: `src/workbench/commands.rs`**

```rust
//! The command catalog of Workspace 0.1, ported one to one: one typed id per action, the
//! exact-modifier shortcut table, and the reasons a command is unavailable. Buttons, shortcuts
//! and the command palette all go through the same [`available`] check.

/// Every action the workbench offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    /// Quick command palette.
    Commands,
    /// Open a project folder.
    Project,
    /// Search files.
    Files,
    /// Save the open file.
    Save,
    /// Git diff of the open file.
    Diff,
    /// New terminal or split session.
    NewTerminal,
    /// Show or hide the terminal dock.
    ToggleTerminal,
    /// Focus the editor.
    FocusEditor,
    /// Focus the file tree.
    FocusFiles,
    /// Select the next terminal.
    NextTerminal,
    /// Select the previous terminal.
    PreviousTerminal,
    /// Focus terminal 1 to 4.
    FocusTerminal(u8),
    /// Clear the selected terminal's screen.
    ClearTerminal,
    /// Copy the terminal selection.
    CopyTerminal,
    /// Paste into the selected terminal.
    PasteTerminal,
    /// Stop the selected session.
    StopTerminal,
    /// Launch an agent.
    Agent,
    /// Create a handoff.
    Handoff,
    /// Show the project memory.
    Memory,
    /// Refresh the file tree and RAIOS data.
    Refresh,
    /// Appearance settings.
    Settings,
    /// Keyboard shortcut list.
    Shortcuts,
    /// Balanced layout.
    Balanced,
    /// Editor-first layout.
    EditorLayout,
    /// Terminal and agent layout.
    AgentsLayout,
}

/// One key combination. Every binding needs Ctrl (or the platform key); `shift` and `alt` must
/// match exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// Layout-independent key name as GPUI reports it (`p`, `1`, `` ` ``, `left`, `right`).
    pub key: &'static str,
    /// Shift must be held.
    pub shift: bool,
    /// Alt must be held.
    pub alt: bool,
    /// The binding stays active while a terminal has focus.
    pub terminal: bool,
}

const fn bind(key: &'static str) -> Binding {
    Binding {
        key,
        shift: false,
        alt: false,
        terminal: false,
    }
}

const fn bind_terminal(key: &'static str, shift: bool, alt: bool) -> Binding {
    Binding {
        key,
        shift,
        alt,
        terminal: true,
    }
}

/// A command with its label and key bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The action.
    pub id: CommandId,
    /// Turkish label, as in Workspace 0.1.
    pub label: &'static str,
    /// Shortcut text shown next to the label.
    pub shortcut: Option<&'static str>,
    /// Key combinations that run the command.
    pub bindings: &'static [Binding],
}

const fn cmd(
    id: CommandId,
    label: &'static str,
    shortcut: Option<&'static str>,
    bindings: &'static [Binding],
) -> Command {
    Command {
        id,
        label,
        shortcut,
        bindings,
    }
}

const COMMANDS: [Command; 28] = [
    cmd(
        CommandId::Commands,
        "Hızlı komutlar",
        Some("Ctrl Shift P"),
        &[bind_terminal("p", true, false)],
    ),
    cmd(
        CommandId::Project,
        "Proje klasörü aç",
        Some("Ctrl O"),
        &[bind("o")],
    ),
    cmd(
        CommandId::Files,
        "Dosya ara",
        Some("Ctrl P / Ctrl K"),
        &[bind("p"), bind("k")],
    ),
    cmd(
        CommandId::Save,
        "Dosyayı kaydet",
        Some("Ctrl S"),
        &[bind("s")],
    ),
    cmd(
        CommandId::Diff,
        "Açık dosyanın Git diff görünümü",
        None,
        &[],
    ),
    cmd(
        CommandId::NewTerminal,
        "Yeni terminal / bölünmüş oturum",
        Some("Ctrl Shift T"),
        &[
            bind_terminal("t", true, false),
            bind_terminal("`", true, false),
        ],
    ),
    cmd(
        CommandId::ToggleTerminal,
        "Terminali göster / gizle",
        Some("Ctrl `"),
        &[bind_terminal("`", false, false)],
    ),
    cmd(
        CommandId::FocusEditor,
        "Editöre odaklan",
        Some("Ctrl Shift J"),
        &[bind_terminal("j", true, false)],
    ),
    cmd(
        CommandId::FocusFiles,
        "Dosya ağacına odaklan",
        Some("Ctrl Shift E"),
        &[bind_terminal("e", true, false)],
    ),
    cmd(
        CommandId::NextTerminal,
        "Sonraki terminal",
        Some("Ctrl Alt →"),
        &[bind_terminal("right", false, true)],
    ),
    cmd(
        CommandId::PreviousTerminal,
        "Önceki terminal",
        Some("Ctrl Alt ←"),
        &[bind_terminal("left", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(1),
        "1. terminale odaklan",
        Some("Ctrl Alt 1"),
        &[bind_terminal("1", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(2),
        "2. terminale odaklan",
        Some("Ctrl Alt 2"),
        &[bind_terminal("2", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(3),
        "3. terminale odaklan",
        Some("Ctrl Alt 3"),
        &[bind_terminal("3", false, true)],
    ),
    cmd(
        CommandId::FocusTerminal(4),
        "4. terminale odaklan",
        Some("Ctrl Alt 4"),
        &[bind_terminal("4", false, true)],
    ),
    cmd(
        CommandId::ClearTerminal,
        "Seçili terminalin görüntüsünü temizle",
        None,
        &[],
    ),
    cmd(
        CommandId::CopyTerminal,
        "Terminal seçimini kopyala",
        Some("Ctrl Shift C"),
        &[bind_terminal("c", true, false)],
    ),
    cmd(
        CommandId::PasteTerminal,
        "Seçili terminale yapıştır",
        Some("Ctrl Shift V"),
        &[bind_terminal("v", true, false)],
    ),
    cmd(CommandId::StopTerminal, "Seçili oturumu durdur…", None, &[]),
    cmd(
        CommandId::Agent,
        "Ajan başlat",
        Some("Ctrl Shift A"),
        &[bind_terminal("a", true, false)],
    ),
    cmd(
        CommandId::Handoff,
        "Handoff oluştur",
        Some("Ctrl Shift H"),
        &[bind_terminal("h", true, false)],
    ),
    cmd(CommandId::Memory, "Proje hafızasını göster", None, &[]),
    cmd(
        CommandId::Refresh,
        "Dosya ağacı ve Raios verisini yenile",
        None,
        &[],
    ),
    cmd(
        CommandId::Settings,
        "Görünüm ayarları",
        Some("Ctrl ,"),
        &[bind(",")],
    ),
    cmd(CommandId::Shortcuts, "Klavye kısayolları", None, &[]),
    cmd(
        CommandId::Balanced,
        "Dengeli yerleşim",
        Some("Ctrl 1"),
        &[bind("1")],
    ),
    cmd(
        CommandId::EditorLayout,
        "Editör yerleşimi",
        Some("Ctrl 2"),
        &[bind("2")],
    ),
    cmd(
        CommandId::AgentsLayout,
        "Terminal ve ajan yerleşimi",
        Some("Ctrl 3"),
        &[bind("3")],
    ),
];

/// The whole catalog in palette order.
pub fn catalog() -> &'static [Command] {
    &COMMANDS
}

/// Where keyboard focus is, which decides which bindings apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardContext {
    /// A terminal has focus: only bindings marked `terminal` apply, so Ctrl+C, Ctrl+K and the
    /// rest reach the shell.
    pub terminal: bool,
    /// A modal dialog is open: no shortcut applies.
    pub dialog: bool,
    /// A text input (palette query, search field) has focus: only the palette shortcut applies.
    pub input: bool,
}

/// One key press as the resolver needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystroke {
    /// Layout-independent key name.
    pub key: String,
    /// Control is held.
    pub ctrl: bool,
    /// The platform (super) key is held.
    pub platform: bool,
    /// Alt is held.
    pub alt: bool,
    /// Shift is held.
    pub shift: bool,
    /// An IME composition is in progress.
    pub composing: bool,
    /// The key is auto-repeating.
    pub repeat: bool,
    /// AltGr produced this key (a character, not a shortcut).
    pub alt_graph: bool,
}

/// The command a key press runs, or `None` when it belongs to the focused widget. Held keys do
/// not repeat commands unless `consume_repeat` is set.
pub fn resolve_shortcut(
    stroke: &Keystroke,
    context: KeyboardContext,
    consume_repeat: bool,
) -> Option<CommandId> {
    if (stroke.repeat && !consume_repeat)
        || stroke.composing
        || stroke.alt_graph
        || context.dialog
        || !(stroke.ctrl || stroke.platform)
        || (stroke.ctrl && stroke.platform)
    {
        return None;
    }
    let key = stroke.key.to_lowercase();
    for command in &COMMANDS {
        for binding in command.bindings {
            if matches!(
                command.id,
                CommandId::CopyTerminal | CommandId::PasteTerminal
            ) && !context.terminal
            {
                continue;
            }
            if binding.shift != stroke.shift || binding.alt != stroke.alt {
                continue;
            }
            if context.terminal && !binding.terminal {
                continue;
            }
            if context.input && !context.terminal && command.id != CommandId::Commands {
                continue;
            }
            if binding.key == key {
                return Some(command.id);
            }
        }
    }
    None
}

/// What the workbench knows when it decides whether a command can run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkbenchState {
    /// A project folder is open.
    pub project_open: bool,
    /// A file is open in the editor.
    pub file_open: bool,
    /// The open file has unsaved edits.
    pub dirty: bool,
    /// Live terminal sessions (0 to 4).
    pub terminals: u8,
    /// The terminal dock is visible.
    pub dock_visible: bool,
    /// The agent and handoff features exist in this build (P4).
    pub control_plane_features: bool,
    /// The daemon answered its last request.
    pub daemon_reachable: bool,
}

/// Why a command can or cannot run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandState {
    /// The command can run.
    pub enabled: bool,
    /// The reason it cannot, shown in the palette.
    pub reason: Option<&'static str>,
}

fn allow() -> CommandState {
    CommandState {
        enabled: true,
        reason: None,
    }
}

fn deny(reason: &'static str) -> CommandState {
    CommandState {
        enabled: false,
        reason: Some(reason),
    }
}

/// Maximum simultaneous terminal sessions (spec section 7).
pub const MAX_TERMINALS: u8 = 4;

/// Whether `id` can run in `state`, with the reason when it cannot.
pub fn available(id: CommandId, state: &WorkbenchState) -> CommandState {
    use CommandId::*;
    let needs_project = || {
        if state.project_open {
            allow()
        } else {
            deny("Önce bir proje klasörü açın")
        }
    };
    let needs_terminal = || {
        if state.terminals > 0 {
            allow()
        } else {
            deny("Açık terminal yok")
        }
    };
    match id {
        Commands | Project | Settings | Shortcuts | Balanced | EditorLayout | AgentsLayout => {
            allow()
        }
        Files | Refresh | FocusFiles | Memory => needs_project(),
        Save => match (state.file_open, state.dirty) {
            (false, _) => deny("Açık dosya yok"),
            (true, false) => deny("Kaydedilecek değişiklik yok"),
            (true, true) => allow(),
        },
        Diff | FocusEditor => {
            if state.file_open {
                allow()
            } else {
                deny("Açık dosya yok")
            }
        }
        NewTerminal => {
            if !state.project_open {
                deny("Önce bir proje klasörü açın")
            } else if state.terminals >= MAX_TERMINALS {
                deny("En fazla 4 terminal açılabilir")
            } else {
                allow()
            }
        }
        ToggleTerminal => allow(),
        NextTerminal | PreviousTerminal | ClearTerminal | CopyTerminal | PasteTerminal
        | StopTerminal => needs_terminal(),
        FocusTerminal(index) => {
            if index >= 1 && index <= state.terminals {
                allow()
            } else {
                deny("Böyle bir terminal yok")
            }
        }
        Agent | Handoff => {
            if !state.control_plane_features {
                deny("Bu sürümde henüz yok")
            } else if !state.project_open {
                deny("Önce bir proje klasörü açın")
            } else if !state.daemon_reachable {
                deny("Daemon'a ulaşılamıyor")
            } else {
                allow()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str) -> Keystroke {
        Keystroke {
            key: key.into(),
            ctrl: true,
            platform: false,
            alt: false,
            shift: false,
            composing: false,
            repeat: false,
            alt_graph: false,
        }
    }

    const EDITOR: KeyboardContext = KeyboardContext {
        terminal: false,
        dialog: false,
        input: false,
    };
    const TERMINAL: KeyboardContext = KeyboardContext {
        terminal: true,
        dialog: false,
        input: false,
    };

    #[test]
    fn the_catalog_has_unique_ids_and_labels() {
        let mut labels: Vec<&str> = catalog().iter().map(|c| c.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), catalog().len());
        assert_eq!(catalog().len(), 28);
    }

    #[test]
    fn plain_ctrl_keys_reach_the_shell_when_a_terminal_has_focus() {
        for key in ["c", "k", "s", "l", "p", "o", "1"] {
            assert_eq!(
                resolve_shortcut(&stroke(key), TERMINAL, false),
                None,
                "{key}"
            );
        }
    }

    #[test]
    fn terminal_bindings_still_work_inside_a_terminal() {
        let mut s = stroke("t");
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::NewTerminal)
        );
        let mut s = stroke("`");
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::ToggleTerminal)
        );
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, TERMINAL, false),
            Some(CommandId::NewTerminal)
        );
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let mut s = stroke("p");
        assert_eq!(resolve_shortcut(&s, EDITOR, false), Some(CommandId::Files));
        s.shift = true;
        assert_eq!(
            resolve_shortcut(&s, EDITOR, false),
            Some(CommandId::Commands)
        );
        s.alt = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
    }

    #[test]
    fn copy_and_paste_commands_exist_only_in_a_terminal() {
        let mut c = stroke("c");
        c.shift = true;
        assert_eq!(resolve_shortcut(&c, EDITOR, false), None);
        assert_eq!(
            resolve_shortcut(&c, TERMINAL, false),
            Some(CommandId::CopyTerminal)
        );
    }

    #[test]
    fn composition_altgr_dialogs_and_repeats_never_run_commands() {
        let mut s = stroke("s");
        s.composing = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let mut s = stroke("s");
        s.alt_graph = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let s = stroke("s");
        let dialog = KeyboardContext {
            dialog: true,
            ..EDITOR
        };
        assert_eq!(resolve_shortcut(&s, dialog, false), None);
        let mut s = stroke("s");
        s.repeat = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        assert_eq!(resolve_shortcut(&s, EDITOR, true), Some(CommandId::Save));
    }

    #[test]
    fn ctrl_and_the_platform_key_together_are_not_a_shortcut() {
        let mut s = stroke("s");
        s.platform = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
        let mut s = stroke("s");
        s.ctrl = false;
        s.platform = true;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), Some(CommandId::Save));
        let mut s = stroke("s");
        s.ctrl = false;
        assert_eq!(resolve_shortcut(&s, EDITOR, false), None);
    }

    #[test]
    fn a_text_input_only_lets_the_palette_shortcut_through() {
        let input = KeyboardContext {
            input: true,
            ..EDITOR
        };
        assert_eq!(resolve_shortcut(&stroke("s"), input, false), None);
        let mut p = stroke("p");
        p.shift = true;
        assert_eq!(
            resolve_shortcut(&p, input, false),
            Some(CommandId::Commands)
        );
    }

    #[test]
    fn save_explains_why_it_is_unavailable() {
        let mut state = WorkbenchState::default();
        assert_eq!(
            available(CommandId::Save, &state).reason,
            Some("Açık dosya yok")
        );
        state.file_open = true;
        assert_eq!(
            available(CommandId::Save, &state).reason,
            Some("Kaydedilecek değişiklik yok")
        );
        state.dirty = true;
        assert!(available(CommandId::Save, &state).enabled);
    }

    #[test]
    fn the_fifth_terminal_is_refused_with_a_reason() {
        let state = WorkbenchState {
            project_open: true,
            terminals: 4,
            ..WorkbenchState::default()
        };
        let result = available(CommandId::NewTerminal, &state);
        assert!(!result.enabled);
        assert_eq!(result.reason, Some("En fazla 4 terminal açılabilir"));
        assert!(
            !available(
                CommandId::FocusTerminal(4),
                &WorkbenchState {
                    terminals: 3,
                    ..state
                }
            )
            .enabled
        );
        assert!(available(CommandId::FocusTerminal(4), &state).enabled);
    }

    #[test]
    fn agent_commands_fail_closed_without_the_feature_or_the_daemon() {
        let mut state = WorkbenchState {
            project_open: true,
            ..WorkbenchState::default()
        };
        assert_eq!(
            available(CommandId::Agent, &state).reason,
            Some("Bu sürümde henüz yok")
        );
        state.control_plane_features = true;
        assert_eq!(
            available(CommandId::Handoff, &state).reason,
            Some("Daemon'a ulaşılamıyor")
        );
        state.daemon_reachable = true;
        assert!(available(CommandId::Handoff, &state).enabled);
    }
}
```

- [ ] **Step 3: `src/workbench/documents.rs`**

```rust
//! The open-document session: dirty tracking, conflict handling and the guard that stops edits
//! from being lost when the user switches file, project or window. Pure state; the editor owns
//! the text and tells the session what happened.

use crate::documents::{Document, Revision};
use crate::error::{AppError, ErrorCode};
use crate::project::ProjectId;

/// Whether the user may leave the open document right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchDecision {
    /// Nothing would be lost.
    Allowed,
    /// There are unsaved edits: ask the user to save, discard or cancel.
    NeedsDecision,
}

/// Why a save cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveBlocked {
    /// No file is open.
    NoDocument,
    /// There is nothing to save.
    Unchanged,
    /// The file changed on disk; reload or reconcile first.
    Conflict,
}

/// Identifies one save attempt, so edits made while it was running are not marked as saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTicket(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Open {
    project_id: ProjectId,
    path: String,
    base: Revision,
    dirty: bool,
    conflict: bool,
    /// Counts edits; a save only clears `dirty` when no edit happened since it began.
    generation: u64,
    last_error: Option<ErrorCode>,
}

/// State of the file open in the editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentSession {
    open: Option<Open>,
}

impl DocumentSession {
    /// A session with no file open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Path of the open file, relative to its project.
    pub fn path(&self) -> Option<&str> {
        self.open.as_ref().map(|open| open.path.as_str())
    }

    /// True when the open file has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.dirty)
    }

    /// True when the last save found the file changed on disk.
    pub fn has_conflict(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.conflict)
    }

    /// The code of the last failed save that was not a conflict.
    pub fn last_error(&self) -> Option<ErrorCode> {
        self.open.as_ref().and_then(|open| open.last_error)
    }

    /// Whether the user may open `path` of `project` now. Reopening the same file is always
    /// allowed (it is a no-op for the caller); anything else needs a decision while dirty.
    pub fn request_open(&self, project: &ProjectId, path: &str) -> SwitchDecision {
        match &self.open {
            Some(open) if open.dirty && (open.project_id != *project || open.path != path) => {
                SwitchDecision::NeedsDecision
            }
            _ => SwitchDecision::Allowed,
        }
    }

    /// Whether the window, project or app may close now.
    pub fn request_leave(&self) -> SwitchDecision {
        if self.is_dirty() {
            SwitchDecision::NeedsDecision
        } else {
            SwitchDecision::Allowed
        }
    }

    /// A document was read from disk and loaded into the editor (also used to reload).
    pub fn opened(&mut self, document: &Document) {
        self.open = Some(Open {
            project_id: document.project_id.clone(),
            path: document.path.clone(),
            base: document.revision.clone(),
            dirty: false,
            conflict: false,
            generation: 0,
            last_error: None,
        });
    }

    /// The user changed the text.
    pub fn edited(&mut self) {
        if let Some(open) = &mut self.open {
            open.dirty = true;
            open.generation += 1;
        }
    }

    /// Starts a save: the document to write (the editor's current `text` on top of the revision
    /// the file was read at) and the ticket to hand back on success.
    pub fn begin_save(&self, text: &str) -> Result<(Document, SaveTicket), SaveBlocked> {
        let open = self.open.as_ref().ok_or(SaveBlocked::NoDocument)?;
        if open.conflict {
            return Err(SaveBlocked::Conflict);
        }
        if !open.dirty {
            return Err(SaveBlocked::Unchanged);
        }
        Ok((
            Document {
                project_id: open.project_id.clone(),
                path: open.path.clone(),
                content: text.to_owned(),
                revision: open.base.clone(),
            },
            SaveTicket(open.generation),
        ))
    }

    /// The write succeeded; `saved` carries the new revision. Edits made while the save ran keep
    /// the file dirty.
    pub fn save_succeeded(&mut self, saved: &Document, ticket: SaveTicket) {
        if let Some(open) = &mut self.open {
            if open.project_id == saved.project_id && open.path == saved.path {
                open.base = saved.revision.clone();
                open.conflict = false;
                open.last_error = None;
                if open.generation == ticket.0 {
                    open.dirty = false;
                }
            }
        }
    }

    /// The write failed. A conflict keeps the edits and blocks further saves until the file is
    /// reloaded; any other error keeps the file dirty and records its code.
    pub fn save_failed(&mut self, error: &AppError) {
        if let Some(open) = &mut self.open {
            if error.code == ErrorCode::Conflict {
                open.conflict = true;
            } else {
                open.last_error = Some(error.code);
            }
        }
    }

    /// The user chose to throw the edits away and close the file.
    pub fn close(&mut self) {
        self.open = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(path: &str, revision: &str) -> Document {
        Document {
            project_id: ProjectId::new("p1"),
            path: path.into(),
            content: "text".into(),
            revision: Revision::new(revision),
        }
    }

    fn open_dirty() -> DocumentSession {
        let mut session = DocumentSession::new();
        session.opened(&document("src/lib.rs", "r1"));
        session.edited();
        session
    }

    #[test]
    fn a_clean_file_can_be_left_and_a_dirty_one_needs_a_decision() {
        let mut session = DocumentSession::new();
        let project = ProjectId::new("p1");
        assert_eq!(
            session.request_open(&project, "a.rs"),
            SwitchDecision::Allowed
        );
        session.opened(&document("a.rs", "r1"));
        assert_eq!(session.request_leave(), SwitchDecision::Allowed);
        session.edited();
        assert_eq!(
            session.request_open(&project, "b.rs"),
            SwitchDecision::NeedsDecision
        );
        assert_eq!(
            session.request_open(&ProjectId::new("p2"), "a.rs"),
            SwitchDecision::NeedsDecision
        );
        assert_eq!(
            session.request_open(&project, "a.rs"),
            SwitchDecision::Allowed
        );
        assert_eq!(session.request_leave(), SwitchDecision::NeedsDecision);
    }

    #[test]
    fn saving_needs_an_open_dirty_file() {
        let mut session = DocumentSession::new();
        assert_eq!(
            session.begin_save("x").unwrap_err(),
            SaveBlocked::NoDocument
        );
        session.opened(&document("a.rs", "r1"));
        assert_eq!(session.begin_save("x").unwrap_err(), SaveBlocked::Unchanged);
        session.edited();
        let (document, _) = session.begin_save("new text").unwrap();
        assert_eq!(document.content, "new text");
        assert_eq!(document.revision, Revision::new("r1"));
        assert_eq!(document.path, "a.rs");
    }

    #[test]
    fn a_successful_save_moves_the_base_revision_and_clears_dirty() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(!session.is_dirty());
        session.edited();
        let (next, _) = session.begin_save("v3").unwrap();
        assert_eq!(next.revision, Revision::new("r2"));
    }

    #[test]
    fn edits_made_while_a_save_runs_keep_the_file_dirty() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.edited();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(session.is_dirty());
        let (next, _) = session.begin_save("v3").unwrap();
        assert_eq!(next.revision, Revision::new("r2"));
    }

    #[test]
    fn a_conflict_keeps_the_edits_and_blocks_saving_until_reload() {
        let mut session = open_dirty();
        session.save_failed(&AppError::new(ErrorCode::Conflict, "changed on disk"));
        assert!(session.is_dirty());
        assert!(session.has_conflict());
        assert_eq!(session.begin_save("x").unwrap_err(), SaveBlocked::Conflict);
        assert_eq!(session.request_leave(), SwitchDecision::NeedsDecision);
        session.opened(&document("src/lib.rs", "r9"));
        assert!(!session.has_conflict());
        assert!(!session.is_dirty());
    }

    #[test]
    fn another_failure_keeps_the_file_dirty_and_records_the_code() {
        let mut session = open_dirty();
        session.save_failed(&AppError::new(ErrorCode::InvalidInput, "too large"));
        assert!(session.is_dirty());
        assert!(!session.has_conflict());
        assert_eq!(session.last_error(), Some(ErrorCode::InvalidInput));
        assert!(session.begin_save("x").is_ok());
    }

    #[test]
    fn a_stale_save_result_for_another_file_is_ignored() {
        let mut session = open_dirty();
        let (_, ticket) = session.begin_save("v2").unwrap();
        session.opened(&document("other.rs", "r5"));
        session.edited();
        session.save_succeeded(&document("src/lib.rs", "r2"), ticket);
        assert!(session.is_dirty());
        let (next, _) = session.begin_save("v").unwrap();
        assert_eq!(next.revision, Revision::new("r5"));
    }
}
```

- [ ] **Step 4: `src/workbench/appearance.rs`**

```rust
//! The persisted appearance: `{skin, tone}`, validated the way Workspace 0.1 did. Anything that
//! does not parse falls back to the neutral appearance; a storage failure never blocks work.

use serde::{Deserialize, Serialize};

/// The visual skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Skin {
    /// The neutral skin.
    Standard,
    /// The GT-LCARS skin.
    Lcars,
}

/// Light or dark tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// Dark surfaces.
    Dark,
    /// Light surfaces.
    Light,
}

/// Skin and tone, stored and validated independently of each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    /// The skin.
    pub skin: Skin,
    /// The tone.
    pub tone: Tone,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            skin: Skin::Standard,
            tone: Tone::Dark,
        }
    }
}

/// Parses stored appearance text. Only a JSON object with a known `skin` and a known `tone` is
/// accepted (extra fields are ignored); missing, malformed or partly invalid data, including a
/// JSON array that serde would otherwise read as a tuple, yields the default.
pub fn parse_appearance(stored: Option<&str>) -> Appearance {
    stored
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
        .and_then(|value| serde_json::from_value::<Appearance>(value).ok())
        .unwrap_or_default()
}

/// The text to store.
pub fn serialize_appearance(appearance: &Appearance) -> String {
    serde_json::to_string(appearance).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_malformed_or_unknown_values_fall_back_to_standard_dark() {
        let neutral = Appearance::default();
        for stored in [
            None,
            Some(""),
            Some("not json"),
            Some("null"),
            Some(r#"{"skin":"neon","tone":"dark"}"#),
            Some(r#"{"skin":"lcars","tone":"sepia"}"#),
            Some(r#"{"skin":"lcars"}"#),
            Some(r#"["lcars","light"]"#),
        ] {
            assert_eq!(parse_appearance(stored), neutral, "{stored:?}");
        }
    }

    #[test]
    fn a_valid_pair_round_trips() {
        let chosen = Appearance {
            skin: Skin::Lcars,
            tone: Tone::Light,
        };
        assert_eq!(
            parse_appearance(Some(&serialize_appearance(&chosen))),
            chosen
        );
        assert_eq!(
            serialize_appearance(&chosen),
            r#"{"skin":"lcars","tone":"light"}"#
        );
    }

    #[test]
    fn extra_fields_do_not_invalidate_a_valid_pair() {
        let parsed = parse_appearance(Some(r#"{"skin":"lcars","tone":"dark","zoom":2}"#));
        assert_eq!(parsed.skin, Skin::Lcars);
        assert_eq!(parsed.tone, Tone::Dark);
    }
}
```

- [ ] **Step 5: Wire the module and run**

Add `pub mod workbench;` to `src/lib.rs`, then:

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --lib workbench
cargo clippy -q --all-targets --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core -- -D warnings
```

Expected: 21 passed; no warnings. `parse_appearance` rejects a JSON array (`["lcars","light"]`) on purpose: serde alone would read it as a tuple struct, which 0.1 never accepted.

- [ ] **Step 6: Commit (OWNER GO-AHEAD)** — `feat: add the workbench command catalog, document session and appearance parsing`

---

### Task 2: `gpui-base` dependency, API probe and `rwk-app` skeleton

Same method as Plan 4b's Task 1: pin the library surface by compiling against it, so no later task rests on a remembered signature. The signatures below were read from `gpui-base-0.7.1` on 2026-10-11.

**Files:**
- Modify: `crates/rwk-ui/Cargo.toml`, `tools/raios-workspace-native/Cargo.toml`
- Create: `crates/rwk-ui/tests/gpui_base_probe.rs`, `crates/rwk-app/Cargo.toml`, `crates/rwk-app/src/main.rs` (empty `fn main() {}` only)

- [ ] **Step 1: Dependencies**

`rwk-ui/Cargo.toml`: add `gpui_base = { package = "gpui-base", version = "=0.7.1" }` under `[dependencies]`. Add `crates/rwk-app` to the workspace members with `[package]` fields inherited from the workspace and `[[bin]] name = "rwk-app"`.

- [ ] **Step 2: The probe (the failing test)** — `tests/gpui_base_probe.rs` must compile and use exactly:

```rust
//! Compile-time pin of the gpui-base 0.7.1 surface Plan 5 uses.

use std::rc::Rc;

use gpui::{App, Context, Entity, SharedString, Window};
use gpui_base::input::{Editor, EditorState, InputEvent, InputHighlighter, InputHighlighterFactory};
use gpui_base::resizable::{h_resizable, resizable_panel};
use gpui_base::tree::{Tree, TreeEvent, TreeItem, TreeState};

fn editor_surface(window: &mut Window, cx: &mut Context<EditorState>) {
    // Builders: soft wrap off, line numbers and folding on, language by name.
    let mut state = EditorState::new(window, cx)
        .soft_wrap(false)
        .line_number(true)
        .folding(true)
        .language("rust");
    let _text: SharedString = state.value();
    state.set_disabled(true, cx);
    let factory: InputHighlighterFactory = Rc::new(|_language: &str| -> Option<Box<dyn InputHighlighter>> { None });
    state.set_highlighter_factory(factory, cx);
    let _ = InputEvent::Change;
}

fn editor_element(state: &Entity<EditorState>) -> Editor {
    Editor::new(state)
}

fn tree_surface(cx: &mut App) {
    let item = TreeItem::new("src", "src").child(TreeItem::new("src/lib.rs", "lib.rs")).expanded(true);
    let state = cx.new(|cx| TreeState::new(cx).items(vec![item]));
    let _element = Tree::new(&state);
    let _ = (TreeEvent::Expanded("a".into()), TreeEvent::Collapsed("a".into()));
}

fn resizable_surface() {
    let _group = h_resizable("workbench").child(resizable_panel().size(gpui::px(240.0)));
}
```

The probe is allowed to differ from this text where the real signature differs: fix it to the source in `~/.cargo/registry/src/*/gpui-base-0.7.1/src` and record the corrected signature in the Task 3-7 code before continuing. Also pin, in the same file or a second `#[test]`, each item below (the probe fails to compile if one is missing):

- `EditorState::set_value(&mut self, value, window, cx)` (read its exact parameters at `input/base/state.rs:948`), `EditorState::value()`.
- The `gpui_base::init(cx)` call at `lib.rs:228` and whatever the component needs at startup (`tree::init`, `dialog::init`, key bindings). Record which of them `rwk-app` must call and in what order.
- `gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(...) }` and `App::prompt_for_paths(options) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>>`.
- Whether `Editor` needs `gpui_base::Root` as the window root and how `Root` is built (`root.rs`), because dialogs and toasts are hosted there.
- Whether `DockArea` (`dock/dock_area.rs`) offers a simpler path than `h_resizable` for the terminal dock (decision E4); write one sentence in the commit message either way.

- [ ] **Step 3: Run it, see the compile errors, fix the probe**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --test gpui_base_probe
```

Expected first run: the dependency build (long), then PASS or errors that name the real signatures.

- [ ] **Step 4: Supply-chain gate, boundary and audit**

```bash
python3 tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py tools/raios-workspace-native/Cargo.lock --expected-owner huacnlee --allowlist tools/raios-workspace-native/scripts/gpui-supply-chain-allowlist.toml
cargo tree --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-core --prefix none -e normal
cargo audit --file tools/raios-workspace-native/Cargo.lock
```

Expected: 0 findings (the script now sees `gpui-base`'s own crate too); `cargo tree` shows no `gpui*`, `raios-*`, `tauri`, `wgpu`, `tokio` in `rwk-core`; `cargo audit` reports no vulnerability. Any new advisory goes to the owner.

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: add gpui-base and pin its API surface`

---

### Task 3: The syntax highlighter (`rwk-ui::editor::highlight`)

**Files:**
- Modify: `crates/rwk-ui/Cargo.toml` (`syntect = { version = "5", default-features = false, features = ["default-fancy"] }`; use the newest 5.x the lockfile resolves and record it)
- Create: `crates/rwk-ui/src/editor/{mod,highlight}.rs`, `crates/rwk-ui/tests/editor_headless.rs` (first tests)

**Interfaces:**
- Consumes: `gpui_base::input::{InputHighlighter, InputHighlighterFactory}`, `HighlightStyleResolver`, `FoldRange` as pinned in Task 2.
- Produces: `SyntectHighlighter` and `highlighter_factory() -> InputHighlighterFactory`.

- [ ] **Step 1: Pure tests first** (in `highlight.rs`'s `#[cfg(test)]`; they exercise syntect directly, no window):

```rust
#[test]
fn rust_keywords_get_a_style_and_plain_text_does_not() {
    let source = "fn main() { let x = 1; }\n";
    let spans = highlight_spans("rust", source).expect("rust is supported");
    assert!(spans.iter().any(|span| &source[span.range.clone()] == "fn"), "{spans:?}");
    assert!(highlight_spans("text", "just words").is_none());
}

#[test]
fn json_markdown_javascript_and_rust_are_supported_like_workspace_0_1() {
    for language in ["rust", "json", "markdown", "javascript", "typescript"] {
        assert!(language_supported(language), "{language}");
    }
}

#[test]
fn a_file_above_the_highlight_limit_opens_unhighlighted() {
    assert!(highlight_enabled(HIGHLIGHT_LIMIT_BYTES));
    assert!(!highlight_enabled(HIGHLIGHT_LIMIT_BYTES + 1));
}

#[test]
fn the_language_comes_from_the_file_extension() {
    assert_eq!(language_for_path("src/lib.rs"), Some("rust"));
    assert_eq!(language_for_path("package.json"), Some("json"));
    assert_eq!(language_for_path("README.md"), Some("markdown"));
    assert_eq!(language_for_path("app.ts"), Some("typescript"));
    assert_eq!(language_for_path("Makefile"), None);
}
```

Define `HIGHLIGHT_LIMIT_BYTES = 1024 * 1024`, `HighlightSpan { range: Range<usize>, scope: String }`, and the four functions the tests name. `highlight_spans` runs syntect's `ParseState`/`HighlightState` line by line and maps scopes to a fixed token-class table (`keyword`, `string`, `comment`, `number`, `type`, `function`, `constant`, `punctuation`); the table is resolved to colours by the editor style, never by syntect's own theme (so tone switching works).

- [ ] **Step 2: Run them (RED), implement (GREEN)**

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --lib highlight
```

- [ ] **Step 3: The `InputHighlighter` adapter.** `SyntectHighlighter` implements the trait pinned in Task 2: `language()` returns the language name; `update(edit, text, folding, window, cx)` re-parses from the first changed line (keep the per-line parse states in a `Vec` and truncate it at the edit's start line, so an edit near the top of a 30K-line file does not re-highlight the whole file on every keystroke; the first measurement of this cost is the E1 gate); `styles(range, resolver)` returns the cached spans inside `range` resolved through `resolver`; `fold_ranges(text)` derives folds from brace and indentation structure (a simple scan, no parser): lines ending in `{` fold to the matching `}`, and Markdown headings fold to the next heading of the same or higher level. `highlighter_factory()` returns `Rc::new(|language| ...)`, `None` for unsupported languages.

- [ ] **Step 4: Cost check (no gate yet, a number for the report).** In `editor_headless.rs`, an `#[ignore]` test `highlight_cost_of_a_30k_line_rust_file_is_reported` generates 30,000 lines of Rust-like text, times the first full highlight and a one-line edit at line 100 and at line 29,900 on the test platform, and prints both. Task 9's E1 gate uses the real editor; this test only tells the executor early whether Step 3's incremental design is needed or too slow.

- [ ] **Step 5: Commit (OWNER GO-AHEAD)** — `feat: add the syntect highlighter for the native editor`

---

### Task 4: `EditorView` (editor, saves, conflicts, diff)

**Files:**
- Create: `crates/rwk-ui/src/editor/view.rs`
- Modify: `crates/rwk-ui/src/editor/mod.rs`, `crates/rwk-ui/tests/editor_headless.rs`

**Interfaces:**
- Consumes: `DocumentSession`, `SwitchDecision`, `SaveBlocked` (Task 1); `ProjectRoot::{read_document, save_document, git_diff}`; the pinned `EditorState` surface.
- Produces: `EditorView`, `EditorEvent` (see API skeleton).

- [ ] **Step 1: Failing headless tests** (GPUI test platform with `CosmicTextSystem`, serial lock and wake-forwarding helper exactly as in Plan 4b's `view_headless.rs`; each test uses a real temporary project directory opened with `ProjectRoot::open`):

```text
opening_a_file_shows_its_text_with_soft_wrap_off
typing_marks_the_file_dirty_and_saving_writes_it_and_clears_dirty
a_save_over_a_changed_file_reports_a_conflict_and_keeps_the_text
opening_a_binary_file_shows_a_message_and_keeps_the_open_file
opening_a_file_over_two_mib_shows_a_message_and_keeps_the_open_file
edits_made_while_a_save_runs_keep_the_file_dirty_in_the_view
the_diff_view_is_read_only_and_colours_added_and_removed_lines
a_file_above_the_highlight_limit_opens_unhighlighted
a_very_long_line_opens_within_the_editor_budget
```

Write each as a complete test (arrange a temp project, drive the view through its public methods, assert on `EditorView`'s observable state: `text`, `is_dirty`, `status`, `session`), run them to see them fail for the right reason (`EditorView` missing), then implement.

- [ ] **Step 2: Implement.** `EditorView` owns `Entity<EditorState>` created with `.soft_wrap(false).line_number(true).folding(true)`, a `DocumentSession`, a status line and a project generation counter. `open(document)` (called only after the guard allowed it) sets the language from `language_for_path`, installs `highlighter_factory()` only when `highlight_enabled(content.len())`, calls `set_value`, then `session.opened`. A subscription to `InputEvent::Change` calls `session.edited()` and emits `EditorEvent::Edited`. `save(project)` asks `session.begin_save(&text)`; on `Ok((document, ticket))` it runs `ProjectRoot::save_document` on the background executor and applies the result through a weak handle: success -> `session.save_succeeded(&saved, ticket)`; failure -> `session.save_failed(&error)` and `EditorEvent::Conflict` when the code is `Conflict`, `SaveFailed` otherwise. Errors from `read_document` (not UTF-8, over 2 MiB, not a file) set the status message and leave the open file untouched. `show_diff(text)` puts the diff into a second `EditorState` with `set_disabled(true)` and a diff highlighter that classifies lines (`+`, `-`, `@@`, headers) without syntect.

- [ ] **Step 3: Run, lint, commit (OWNER GO-AHEAD)** — `feat: add the editor view with revision-checked saves and a diff view`

---

### Task 5: `FileTreeView`

**Files:**
- Create: `crates/rwk-ui/src/file_tree/{mod,view}.rs`, `crates/rwk-ui/tests/tree_headless.rs`

**Interfaces:**
- Consumes: `ProjectRoot::list`, `FileEntry`, `TreeState`/`Tree`/`TreeItem`/`TreeEvent` (Task 2).
- Produces: `FileTreeView`, `FileTreeEvent::OpenFile(String)`.

- [ ] **Step 1: Failing tests** in `tree_headless.rs` (temp project with `src/lib.rs`, `src/bin/main.rs`, `README.md`, a symlink pointing outside the project, a file named `.hidden`):

```text
the_root_lists_directories_first_then_files_case_insensitively
expanding_a_directory_lists_its_children_lazily
a_symlink_that_leaves_the_project_is_not_listed_or_opened
selecting_a_file_asks_the_guard_before_opening
a_listing_from_the_previous_project_is_dropped
refresh_keeps_expanded_folders_and_the_selection
```

- [ ] **Step 2: Implement.** `set_project` bumps a project generation, clears the items and starts the root listing on the background executor. Each `TreeEvent::Expanded(id)` starts one listing for that folder; the result is applied only if its generation still equals the current one and the folder is still expanded. Items are built from `FileEntry`; the item id is the relative path. Selection emits `FileTreeEvent::OpenFile(path)` only for files; the workbench decides whether the guard lets it through. `refresh` re-lists the root and every expanded folder and restores expansion and selection by path.

- [ ] **Step 3: Run, lint, commit (OWNER GO-AHEAD)** — `feat: add the lazy file tree`

---

### Task 6: `Workbench` shell, dock and terminals

**Files:**
- Create: `crates/rwk-ui/src/workbench/{mod,layout,dock,state}.rs`, `crates/rwk-ui/tests/workbench_headless.rs`

**Interfaces:**
- Consumes: Task 1 logic; `EditorView`, `FileTreeView`, `TerminalView`, `PtyTerminalHost`, `SessionRegistry` (`rwk_core::sessions`, max 4), `wake_channel`/`FrameGate` (Plan 4b's registry pattern from `rwk-ui-bench/src/child.rs`).
- Produces: `Workbench`, `Workbench::run`, `Workbench::state`.

- [ ] **Step 1: Failing tests:**

```text
the_command_dispatch_path_is_shared_by_shortcut_button_and_palette
a_disabled_command_does_nothing_and_reports_its_reason
the_fifth_terminal_is_refused_with_a_reason
hiding_the_dock_keeps_the_terminals_alive
layout_presets_set_the_three_pane_proportions
switching_files_with_unsaved_edits_asks_first
closing_with_unsaved_edits_asks_first
closing_the_window_stops_owned_sessions
```

- [ ] **Step 2: Implement.** `Workbench` owns the panes (files / editor / dock in an `h_resizable` group, decision E4), the `SessionRegistry`, a `PtyTerminalHost` whose wake closure routes to per-session `FrameGate`s (the `Registry` pattern from `rwk-ui-bench`), the `DocumentSession`-aware `EditorView`, the selected terminal index and the dock visibility. `run(CommandId, window, cx)` is the only place commands take effect: it first calls `available`, shows the reason in a toast when disabled, and otherwise dispatches (open project, save, diff, new terminal, focus moves, layout presets 1/2/3, toggle dock, copy/paste/clear/stop on the selected terminal, refresh, settings, shortcuts). Layout presets are three fixed proportion triples applied to the resizable state; the divider moves with arrow keys when focused. Agent, Handoff and Memory are dispatched to a "not in this version" toast (decision E10).

- [ ] **Step 3: Run, lint, commit (OWNER GO-AHEAD)** — `feat: add the workbench shell with the terminal dock`

---

### Task 7: Command palette, dialogs and toasts

**Files:**
- Create: `crates/rwk-ui/src/workbench/{palette,dialogs}.rs`
- Modify: `crates/rwk-ui/tests/workbench_headless.rs`

- [ ] **Step 1: Failing tests:**

```text
the_palette_filters_commands_by_label_and_shows_why_one_is_unavailable
the_palette_runs_the_selected_command_through_the_shared_path
only_the_palette_shortcut_works_while_the_palette_input_has_focus
the_unsaved_changes_dialog_offers_save_discard_and_cancel
cancelling_the_dialog_changes_nothing
no_shortcut_runs_while_a_dialog_is_open
stopping_a_session_asks_for_confirmation
```

- [ ] **Step 2: Implement** on `gpui_base::dialog` / `toast` hosted by the root pinned in Task 2 (a plain overlay entity if the components cannot be used without `gpui-component`'s theme). The palette is an input plus a filtered list over `catalog()` with `available` reasons. Keyboard routing: a `KeyboardContext` is derived from what has focus (terminal view, text input, dialog) and passed to `resolve_shortcut`; reserved combinations run their command and stop propagation, everything else falls through to the focused widget (spec section 7).

- [ ] **Step 3: Run, lint, commit (OWNER GO-AHEAD)** — `feat: add the command palette, dialogs and toasts`

---

### Task 8: `rwk-app`

**Files:**
- Create: `crates/rwk-app/src/{main,persistence}.rs`
- Modify: `crates/rwk-app/Cargo.toml`

- [ ] **Step 1: Failing tests** (pure, in `persistence.rs`): `the_appearance_file_lives_under_the_xdg_config_home`, `a_missing_unreadable_or_invalid_file_gives_the_default_appearance`, `saving_creates_the_directory_with_owner_only_permissions`, `a_failed_save_is_reported_but_never_blocks_the_app`.

- [ ] **Step 2: Implement.** `persistence.rs` reads and writes `$XDG_CONFIG_HOME/raios-workspace-native/appearance.json` (fallback `~/.config`), 0700 directory and 0600 file, through `parse_appearance`/`serialize_appearance`. `main` builds the application (`gpui_platform::application()`), runs the initialization sequence pinned in Task 2, opens one window hosting `Workbench`, wires the folder picker (`prompt_for_paths`, directories only) to `Workbench::run(CommandId::Project)` and applies the stored appearance. Closing the window goes through `request_leave` and stops owned sessions first. The binary never starts the daemon; "daemon unreachable" is shown as state in P4.

- [ ] **Step 3: Smoke run on a machine with a display:** `cargo run --release -p rwk-app`. Record in the commit body whether the window painted more than one frame (the executor's machine for Plan 4b did not).

- [ ] **Step 4: Commit (OWNER GO-AHEAD)** — `feat: add the rwk-app binary`

---

### Task 9: The P3 gates in `ui-bench`

**Files:**
- Modify: `crates/rwk-ui-bench/src/{gates,child,main}.rs`

- [ ] **Step 1: Pure tests first** in `gates.rs`: the four new gates exist with the limits of the table above (E1 350 ms, E2 700 ms, A1 300 ms, A2 150 MB, all at-most); `ui-bench editor-open|app-cold-start|app-idle` parse; the NOT MEASURED path for A1/A2 reuses the frame-delivery probe of Plan 4b.
- [ ] **Step 2: Implement.** `editor-open`: a child opens a window with an `EditorState`, builds 30,000 lines (typical Rust-like content, ~40 bytes per line) and, in a second run, 500,000 lines, and times `set_value` to the first painted frame with highlighting enabled for E1 and disabled for E2 (E2 is a capability gate; say so in its report line). `app-cold-start`: spawn `rwk-app --exit-after-first-frame`, parent times spawn to the printed `FIRST_FRAME`. `app-idle`: `rwk-app` with a temp project, one file open and four terminals running `sleep`, the same frame-delivery probe, 10 s settle, PSS from `smaps_rollup`.
- [ ] **Step 3: Run the pure tests and a smoke run** (`ui-bench editor-open --runs 1`); a machine that delivers no frames must report NOT MEASURED, never PASS.
- [ ] **Step 4: Commit (OWNER GO-AHEAD)** — `feat: add the editor-open and application gates`

---

### Task 10: Verification gate and documentation

- [ ] **Step 1:** `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` for the whole native workspace, `cargo audit` on the lockfile, the supply-chain script, the `rwk-core` dependency scan. All green; report new advisories.
- [ ] **Step 2: Documentation.** `CHANGELOG.md` (what P3 delivers and its known limits: no icons, LCARS skin renders as standard, agent/handoff disabled, diff view without side-by-side), the `tools/raios-workspace-native/README.md` crate table and test/gate commands, `memory.md` under the executor's identity, `sigmap`, `raios pre-flight`.
- [ ] **Step 3: Commit (OWNER GO-AHEAD)** — `docs: record the native editor and workbench`

---

### Task 11: Authoritative gate run and the P3 decision (planner or owner, reference machine)

Not for an unattended executor.

- [ ] **Step 1:** Run Plan 4b's Task 10 first if it has not been run (U1-U5 and the visual checks). Close heavy background work; load average below 1.5.
- [ ] **Step 2:** `ui-bench editor-open|app-cold-start|app-idle --enforce --runs 5` three times; raw output into `docs/superpowers/reports/2026-10-workbench-gates.md` with the stated exclusions (painted not presented frames; E2 is a capability gate).
- [ ] **Step 3:** Use the app for real: open this repository, edit and save a file, force a conflict from another shell, switch files with unsaved edits, open four terminals, close the window with edits pending. Record every surprise in the report.
- [ ] **Step 4: Decision.** E1, E2, A1, A2 pass and Step 3 shows no lost edit: P4 may start. A gate failure by less than 20%: the owner decides whether to accept. A lost edit or a crash: fix before anything else, no phase starts.

### Task 12: Stop here

Do not start P4 inside this plan.

---

## Out of scope

Inspector, handoff composer, consent dialog, daemon connection state (P4); memory view, LCARS skin, Breeze icons, file-type classifier, parity checklist and removal of the Tauri version (P5); LSP, search-in-files, multi-cursor beyond what `EditorState` offers, split editors, side-by-side diff, session restoration, installer and signing.

## Self-review

- **Spec coverage (section 7):** editor with soft wrap off, line numbers, folding, revision-checked saves, unsaved-change guards, read-only diff view, syntect highlighting (Tasks 3, 4); keyboard and command system ported 1:1 with available/reason, one dispatch path, exact modifiers, terminal key contexts (Tasks 1, 6, 7); layout with draggable divider, Ctrl/Cmd+1/2/3 presets, up to four terminals (Task 6); file tree with arrow navigation (Task 5; icons and classifier deferred to P5 by the spec's own phase list); theme tone switching without recreating editors or PTYs (Tasks 6, 8, decision E7; LCARS and fonts in P5). Section 9 gates: editor open (E1, E2), cold start (A1), idle PSS (A2). Section 10: P3 = this plan.
- **Placeholders:** Task 1 contains the full verified code. Tasks 3-9 give exact test names, interfaces, behaviours and the library signatures pinned in Task 2, but not code blocks for the GPUI-bound parts, for the reason Plan 4b stated: the `gpui-base` and `gpui-pre` signatures are only trustworthy after Task 2's probe compiles. The planner expands Tasks 3-9 into full code after Task 2 has run, as was done for Plan 4b.
- **Type consistency:** `CommandId`, `KeyboardContext`, `WorkbenchState`, `DocumentSession`, `SaveTicket`, `Appearance` are defined once in Task 1 and consumed by name in Tasks 4-8. `FileTreeEvent::OpenFile` carries the relative path that `DocumentSession::request_open` and `ProjectRoot::read_document` take.
- **Open items for the owner:** decisions E1-E10; whether Plan 4b's Task 10 must pass before Task 5 begins (this draft allows starting at once, accepting the rework risk stated above); whether `rwk-app` should already contain the P4 daemon connection banner (this draft leaves it to P4).

---

## Execution notes (2026-10-11, inline, uncommitted)

Where the executed work differs from the text above (each is a `Ruling:` line in the ledger):

- Task 1: `workbench/mod.rs` also re-exports `SaveTicket` (the editor needs it) and a fifth module, `palette` (Task 7's pure content), was added.
- Task 2: `gpui-base` re-exports `Tree`, `TreeState`, `TreeItem`, `TreeEvent`, `h_resizable`, `resizable_panel`, `ResizableState` and `Root` at the crate root (the modules are private). `DockArea` was not used.
- Task 3: **highlighting is asynchronous.** Measured in release, syntect with `fancy-regex` parses about 3K lines per second (30K lines: 15.5 s; an edit near the top re-parses everything after it). The single synchronous adapter of the plan was replaced by a background worker that publishes 100-line chunks, serves the parsed prefix plus the previous spans shifted across the edit, and never parses on the UI thread. `syntect`'s `regex-onig` (C dependency) remains the fallback if the reference-machine run shows the background latency is too high.
- Task 4: no soft-wrap getter exists, so that check became `opening_a_file_shows_its_text_and_is_clean`; the diff view's read-only state is `!is_editable()`; the git test fixture ignores the user's global git configuration.
- Task 5: Enter and click on a tree row are handled by the view itself (the tree's Confirm action only toggles folders).
- Task 6: the workbench found a real bug: project switching must stop sessions one by one, because `SessionRegistry::stop_all` closes the registry for good.
- Task 7: dialogs, palette and the toast line are plain overlays, not `gpui-base` components; `Files` ("Dosya ara") is a toast, file-name search is not built.
- Task 8: `rwk-app --exit-after-first-frame` waits for a compositor frame callback after the first render (the first render happens inside window creation and would pass trivially).
- Task 9: `ui-bench` U1 and A1 use the same definition; the first-frame wait is bounded to 20 s.
- Final self-review found and fixed two data-loss paths: reopening the open file with unsaved edits, and switching project while sessions run.
