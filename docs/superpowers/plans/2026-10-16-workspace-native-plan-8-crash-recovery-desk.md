# Native workspace Plan 8: a desk that survives a power cut

Status: **proposal, waiting for approval** (RIPER-5 phase 3, skeleton only; no behaviour code yet).
Date: 2026-10-16. Owner: Göktuğ. Author: Claude Kaira.

## 1. Why (the user's own words, 2026-10-16)

> Today I open 5 terminal tabs and split each in two. The power goes out, the system dies,
> and it is hard to track the sessions and manage the memory.
> The workspace must do this. The editor is only for when it is needed. The terminals must be
> fast, solid and clean.

So the product goal is not "an editor". It is:

1. **A desk of about 10 terminals** (5 tabs × 2 panes) that opens fast and stays smooth.
2. **After a crash or power cut, one action puts the whole desk back** where it was: same
   projects, same folders, same agent conversations.
3. **One place that shows every session and its memory**, so nothing is lost track of.
4. The editor stays as it is: usable when needed, no more investment.
5. **Every terminal says who it is**: a name strip (agent name plus the user's label) and a
   colour per agent or per pane, so ten terminals can be told apart at a glance.
6. **A menu bar at the top** (Dosya, Düzenle, Görünüm, Oturum, Ayarlar, Yardım) so every
   command and every setting can be reached by mouse, without remembering shortcuts.

## 2. What exists today (measured from the code on master `696e22b`)

| Need | Today | Gap |
| --- | --- | --- |
| Number of terminals | `MAX_SESSIONS = 4` (`rwk-core/src/sessions.rs:13`) | The desk needs ~10. Hard blocker. |
| Shape of the desk | One window = one project; layout presets of 1/2/3 panes | No "tabs of projects, each with 2 panes". |
| Saved state | One file per project (`SavedWorkspace`, atomic, owner-only), saved on every change; terminals capped at 4 | No whole-desk file, no crash marker, no fsync guarantee stated. |
| Agent continuation | `raios resume --list --json` catalog; saved agent terminals are *offered* one by one | No "restore everything" action; shells are restarted but their folder is not restored per pane. |
| Session overview | `raios sessions` (CLI), workspace inspector (tasks, runs, approvals of the open project) | No cross-project list of agent conversations with their last work. |
| Terminal output after restart | none | The user sees an empty terminal and does not remember what it was doing. |

## 3. Decisions to approve before any code

1. **Desk model.** One window holds a *desk* = ordered **desk tabs**; each desk tab has a
   project root and **1–2 panes** (more later). Recommended: keep "1–2 panes per tab" now.
2. **Session cap.** Raise `MAX_SESSIONS` from 4 to **12** (one constant, backend and UI), and
   add a performance gate that runs 10 live terminals (see §6). Recommended: 12.
3. **Where the desk is stored.** One file `desk.json` next to today's per-project files,
   written atomically and **fsynced** (file and directory) on every structural change;
   unchanged per-project files keep working. Recommended: yes.
4. **Crash detection.** A `clean_exit` marker written on a normal quit and removed at start.
   Marker missing at start ⇒ the previous run ended abnormally ⇒ show the recovery screen.
   Recommended: yes.
5. **Restore policy.** Recovery screen lists every pane with a checkbox, default all selected,
   and **one button "Hepsini geri yükle"**. Shell panes restart in their folder.
   Agent panes continue their conversation through `raios resume <key>` (the existing
   planner re-checks each one); nothing runs without the user pressing the button.
   This replaces the earlier "offer one by one" behaviour. Recommended: yes.
6. **Scrollback tail.** Store the last **200 lines** of each terminal (secrets redacted with
   the existing redactor, owner-only file) and show them read-only above the restored
   terminal. Recommended: yes, as a last phase; it can be dropped without touching the rest.

## 4. Skeleton (types and contracts only)

```rust
// rwk-core/src/desk.rs  (new, pure data + sanitize rules, like layout_state.rs)
pub const MAX_DESK_TABS: usize = 8;
pub const MAX_PANES_PER_TAB: usize = 2;

pub struct SavedDesk {
    pub version: u32,
    pub tabs: Vec<SavedDeskTab>,
    pub active: usize,
}

pub struct SavedDeskTab {
    pub project: PathBuf,              // absolute project root
    pub panes: Vec<SavedPane>,         // 1..=MAX_PANES_PER_TAB
    pub layout: PaneSplit,             // Single | Vertical | Horizontal
    pub editor_tabs: Vec<SavedTab>,    // existing type, unchanged
}

pub struct SavedPane {
    pub terminal: SavedTerminal,       // existing: Shell | Agent { kind, native_session }
    pub cwd: PathBuf,                  // absolute, validated inside or equal to project
    pub tail: Option<Vec<String>>,     // last lines, redacted; None when disabled
    pub label: Option<String>,         // user's name for the pane, <= 40 chars, no control chars
    pub tint: Option<PaneTint>,        // None = the agent's default colour
}

/// A fixed palette, never free hex: contrast is checked once per theme (see tests).
pub enum PaneTint { Amber, Blue, Green, Violet, Rose, Teal, Slate }

// rwk-core/src/desk_recovery.rs  (new)
pub enum StartState { Fresh, CleanRestart(SavedDesk), CrashedWith(SavedDesk) }
pub struct RecoveryPlan { pub items: Vec<RecoveryItem> }
pub struct RecoveryItem { pub tab: usize, pub pane: usize, pub action: RecoveryAction }
pub enum RecoveryAction {
    RestartShell { cwd: PathBuf },
    ResumeAgent { key: ResumeKey },        // from the `raios resume` catalog
    OfferNewAgent { kind: SessionKind },   // conversation no longer resumable
}
pub fn plan_recovery(desk: &SavedDesk, catalog: &SessionCatalog) -> RecoveryPlan; // pure, no I/O

pub trait DeskStore { // implemented by rwk-app over files; in-memory stand-in for tests
    fn load(&self) -> StartState;
    fn save(&self, desk: &SavedDesk) -> Result<()>;   // atomic + fsync
    fn mark_running(&self) -> Result<()>;             // removes clean_exit
    fn mark_clean_exit(&self) -> Result<()>;
}
```

UI contracts (empty at first): `RecoveryView` (the list with checkboxes and the one button),
`DeskTabBar` (project tabs above the dock), `SessionsPanel` (cross-project list, §5 phase F).

## 5. Phases (each ends with tests and a real-window check; one PR each)

- **A. Terminal ceiling and quality.** `MAX_SESSIONS` 4 → 12; 10-terminal gate (§6);
  fix whatever the gate finds. No new UI.
- **B. Desk model.** `SavedDesk`, sanitize rules, migration from today's per-project state,
  desk tabs and a 1–2 pane split in the workbench.
- **C. Durable store and crash detection.** Atomic + fsync writes, `clean_exit` marker,
  `StartState`. Tests simulate truncated files and a missing marker.
- **D. Recovery screen.** `plan_recovery`, `RecoveryView`, "Hepsini geri yükle", per-item
  skip, restored agents through `raios resume`.
- **E. Sessions panel.** Every agent conversation across projects (from the resume catalog and
  the control plane): project, status, last work description, link to the project's memory.
- **F. Scrollback tail** (optional, droppable).
- **H. Pane identity.** Each terminal gets a header strip: coloured dot, agent name
  (`Claude`, `Codex`, `OpenCode`, `Antigravity`, `Kabuk`), the user's label and, when known,
  the project folder name. The terminal body gets a *very light* tint overlay (about 6 %
  alpha) plus a coloured left edge, strong enough to recognise across the room and weak
  enough not to touch text contrast. Default colour per agent kind (Claude amber, Codex
  blue, OpenCode green, Antigravity violet, shell slate); a pane can override it from its
  header or the menu (*Oturum → Bölme rengi / Bölme adı*). Colour and label are saved in the
  desk and restored. The existing 0.1 contrast gates run for every tint on all four
  appearances; a tint that fails a gate is not offered. Pairs with the status marker from
  phase E so the strip also shows running / waiting / finished / failed.
- **G. Menu bar and settings.** A menu bar over the existing `CommandId` registry (the same
  commands the palette lists, with their shortcuts and disabled reasons), plus a settings
  view. Menus: *Dosya* (open project, quick open, save, close tab, quit), *Düzenle*
  (search in project, copy/paste), *Görünüm* (terminal, inspector, layouts, skin and tone,
  dock size), *Oturum* (new terminal, start agent, handoff, restore desk, sessions panel,
  project memory), *Ayarlar* (appearance, font size, terminal limit, restore policy,
  scrollback tail on/off), *Yardım* (shortcut list). Settings persist next to
  `appearance.json`. No new behaviour: every item runs an existing command.

## 6. How we will know it works (acceptance)

1. **Power-cut drill**, repeatable by script: start the app with 5 desk tabs × 2 panes
   (shells plus at least one real agent), `kill -9` the app, start again ⇒ recovery screen
   lists all 10 panes, one click restores all, shells are in their folders, the agent
   conversation continues. Run also with `desk.json` truncated to half: the app must still
   start and fall back to the per-project files, never crash.
2. **Terminal gates** with 10 live terminals: input-to-echo latency, a 50 MB/s output flood
   without freezing the UI, memory per terminal, start time with a restored desk. Numbers
   recorded next to the existing gates in `docs/superpowers/reports/`.
3. **Safety:** state files owner-only, paths validated as today (`..`, NUL, absolute rules),
   nothing is executed at start without the button, tails redacted. `cargo clippy -D warnings`
   and the existing 170+ tests stay green.

## 7. Out of scope

Editor features (LSP, git gutter, multi-cursor); Windows and macOS; moving PTY hosting into
the daemon (the `TerminalHost` seam stays for later); restoring unsaved editor text after a
crash (open tabs and cursor lines are restored, unsaved edits are not).

## 8. Known risks

- Agents that are mid-answer when the power goes out lose that answer; `raios resume` only
  continues from the last saved turn. The screen will say so.
- `raios resume` coverage differs per agent (Antigravity has no verified continuation); those
  panes get `OfferNewAgent`.
- Twelve live PTYs plus the inspector's event stream must stay inside the start-up and frame
  gates; phase A measures this before anything else is built on it.
