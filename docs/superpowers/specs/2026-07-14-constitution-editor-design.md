# Constitution Editor & Creator — TUI Design

## Problem

The TUI's "Rules" panel (`ui/panels/rules.rs`, menu item, backed by `app/state.rs::system_rules()`)
renders a hand-typed, hardcoded Turkish rule summary that has drifted from the real
`AGENT_CONSTITUTION.md` (currently v5.1, 4-Agent Matrix). It reads nothing from disk and cannot
be edited. Separately, the generic file browser/editor (`EditorState`, `ui/filebrowser.rs`)
already knows how to view and raw-edit arbitrary files, but the master constitution file is
marked `.readonly()` in `get_master_rule_files()`, so it cannot be edited even through that path.

Users want an interactive way to both **view/edit** the real constitution content and
**create** new project-specific constitution files, from inside the TUI, without hand-editing
markdown blind or risking a bad edit to the single file every agent reads.

## Scope

- Replaces the existing "Rules" menu panel with a new **Constitution** panel (same menu slot).
- Covers the global `AGENT_CONSTITUTION.md` (path from `AiosConfig::master_md_path`) and, when a
  project is open, that project's own `CLAUDE.md`/`AGENTS.md`/`GEMINI.md` file(s).
- Does not touch `raios-policy.toml`, `.claude/settings.json`, or other non-constitution config
  files — those remain under the existing Policies/Rules file lists untouched.

## Modes

The Constitution panel has three modes, all operating on whichever file is the active
`ConstitutionTarget` (see Scope of Editing below):

### 1. Outline mode (default)

Parses the active file's real `##`/`###` markdown headers into a navigable tree, instead of
showing static hardcoded text. List items under a section (numbered `1. **Bold:** ...` or
bulleted `* ...`) are flattened as individually-addressable rows under their parent section.

- `j`/`k` or `↑`/`↓` — move between sections/items in the flattened outline
- `→`/`←` — expand/collapse a section
- `Enter` on a section header — jump into raw edit mode with the cursor placed at that
  section's starting line
- `i` on an item row — begin inline edit of that single line (see Inline item editing)
- `n` under a section — insert a new item (opens an empty inline input)
- `d` on an item row — delete that line
- `r` — switch to raw edit mode for the whole file
- `c` — open Creator mode
- `Tab` / `1` / `2` — switch between open tabs (Global / project file), see Scope of Editing
- `Esc` — cancel inline edit, or leave the panel if not editing

### 2. Raw edit mode

Reuses the existing `EditorState`/`Editor` widget and `handle_file_edit_key` keybindings
verbatim (`Ctrl+S` save, `Esc` back to outline, all existing line-editing behavior) — no new
text-editing code. The only change required outside this module is removing the `.readonly()`
flag from the constitution file's `FileEntry` in `get_master_rule_files()`/wherever the target
file entry is constructed, so `e`/`Enter`-to-edit is actually reachable.

### 3. Creator mode

Reachable from either tab via `c`. Two entry points:

- **Project-specific file** (common path): reads the *global* `AGENT_CONSTITUTION.md`'s real
  section titles and presents them as a checklist — "inherit via `@/home/alaz/AGENT_CONSTITUTION.md`"
  (default: all checked, matching the user's existing real-world convention of a single include
  line) vs "override/add for this project". If the user only wants to add project-specific notes
  (the common case), a single free-text field is offered, appended as a new `## Project-Specific
  Rules` section. A preview screen shows the exact markdown that will be written before
  confirming.
- **Global file, from scratch** (rare path, requires an extra "are you sure?" confirmation
  since this is the single file every agent reads): same free-form section-by-section flow,
  writing directly to `master_md_path`.

Both paths funnel through the same save-safety mechanism (below) before anything touches disk —
a brand-new file just shows a diff where every line is an addition.

## Scope of Editing (multi-file)

```rust
pub enum ConstitutionTarget {
    Global,                                          // AiosConfig::master_md_path
    ProjectFile { path: PathBuf, kind: ProjectFileKind }, // active project's CLAUDE.md/AGENTS.md/GEMINI.md
}
```

- If a project is active (`ProjectState.active`), the panel shows two tabs: `[1] Global
  Constitution` and `[2] <project>/CLAUDE.md` (or whichever file `discover_all_agent_rules`-style
  lookup finds for that project). No active project → only the Global tab.
- If the project's file is include-only (just `@/home/alaz/AGENT_CONSTITUTION.md`, matching the
  user's real setup), outline mode shows a short notice — "↳ includes: AGENT_CONSTITUTION.md —
  press [1] to edit the real content" — rather than trying to parse an empty file.

## Save Safety (backup + diff confirmation)

New `save_constitution_file()` in `raios-runtime`'s filebrowser module (separate from the
generic `save_file_content` used by other panels, which is untouched):

1. Before writing, copy the current file to `<file>.bak.<unix_timestamp>` next to it.
2. After copying, prune old backups for that file, keeping only the 5 most recent
   (`<file>.bak.*` glob, sorted by timestamp suffix, delete the rest).
3. Compute a simple line-level diff (added/removed line counts, plus the actual changed lines)
   between old and new content, store it as a pending confirmation, and render an overlay:
   "N lines added, M lines removed — save? [Enter] confirm / [Esc] cancel".
4. Only on `Enter` does the real write happen. A brand-new file (Creator mode, no prior content)
   shows every line as an addition.

This applies uniformly to raw edit mode's `Ctrl+S`, inline item add/edit/delete in outline mode,
and Creator mode's final confirm — all funnel through the same function so there's exactly one
place that knows how to safely write a constitution file.

## Data Model

New `crates/raios-runtime/src/constitution.rs`:

```rust
pub struct ConstitutionSection {
    pub level: u8,             // 1 = "##", 2 = "###"
    pub title: String,
    pub line_start: usize,     // source line range, used to jump into raw edit mode
    pub line_end: usize,
    pub items: Vec<String>,    // numbered/bulleted list lines under this section
    pub children: Vec<ConstitutionSection>,
}

pub fn parse_sections(content: &str) -> Vec<ConstitutionSection>
```

Only `##`/`###` headers and the numbered/bulleted lines immediately under them are recognized;
plain paragraphs (e.g. free-form Persona/Attitude prose) become a single item. Files that don't
parse into any sections (unexpected structure, or an include-only stub) yield an empty
`Vec` — outline mode then shows the include-notice or falls back silently to raw edit, never an
error.

New app state, following the existing `EditorState`/`SetupState` idiom (`editing: bool` +
`input: String` for the field currently being typed, same pattern already used by the setup
wizard and extension-name editing):

```rust
pub struct ConstitutionState {
    pub target: ConstitutionTarget,
    pub tabs: Vec<ConstitutionTarget>,      // 1 or 2 entries depending on active project
    pub sections: Vec<ConstitutionSection>,
    pub outline_cursor: usize,              // index into the flattened section+item list
    pub mode: ConstitutionMode,              // Outline | RawEdit | Creator
    pub item_editing: bool,
    pub item_input: String,
    pub pending_save: Option<PendingSave>,   // set by save_constitution_file() until confirmed
}
```

## Panel Wiring

- `ui/panels/rules.rs` is replaced by `ui/panels/constitution.rs` (rendering outline/raw/creator
  based on `ConstitutionState.mode`); the menu label and index stay as-is, only the panel content
  changes — no menu restructuring elsewhere in `content.rs`/`menu.rs`.
- `app/state.rs::system_rules()` and the `RuleCategory`/`SystemRules` hardcoded data are deleted
  once nothing references them.
- Keyboard handling gets a new `handle_constitution_key` following the same per-panel dispatch
  pattern as `handle_file_view_key`/`handle_file_edit_key`.

## Out of Scope

- No changes to `raios-policy.toml`, `.claude/settings.json`, or the existing generic file
  browser/editor's behavior for non-constitution files.
- No new external diff crate — a minimal line-comparison is sufficient for the confirmation
  overlay; this is not meant to be a full merge/diff tool.
- No automatic propagation of edits to other agents' live sessions — saving the file is enough;
  picking up the change is each agent's existing read-on-start behavior.
