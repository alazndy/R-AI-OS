# RAIOS Workspace Native

Nested Cargo workspace for the native (GPUI) RAIOS workspace. Design: `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`.

| Crate | Role | GPUI |
|---|---|---|
| `rwk-core` | project capability, documents, sessions, control plane client, consent gate, guarded commands, handoff state machine and receipt tracking, input validation, inspector and memory models, theme tokens, file-type classifier, terminal engine and its pure helpers | no |
| `rwk-ui` | terminal view (`terminal`), code editor and highlighter (`editor`), file tree (`file_tree`), inspector, handoff composer, memory view, bundled assets (Breeze icons, Antonio), workbench shell, palette and dialogs (`workbench`) | yes |
| `rwk-app` | the application binary: window, folder picker, appearance persistence, daemon token source | yes |
| `rwk-bench` | headless terminal gates G1-G8 (`term-bench`) | no |
| `rwk-ui-bench` | UI-bound gates U1-U5, E1-E2, A1-A2, D1-D2 (`ui-bench`; D1 = dock rearrangement with 12 terminals, D2 = PTY resizes per rearrangement) | yes |

## Running the application

```bash
cargo run --release -p rwk-app -- /path/to/project
```

`Ctrl+O` picks a folder, `Ctrl+Shift+P` opens the command palette (it lists every command with its shortcut and, for unavailable ones, why). `Ctrl+S` saves, `Ctrl+Shift+T` opens a terminal (up to twelve), `Ctrl+1/2/3` switch the layout. While a terminal has focus only the shortcuts that carry Shift or Alt are taken by the workbench: plain Ctrl keys (`Ctrl+C`, `Ctrl+K`, `Ctrl+L`, ...) reach the shell. Unsaved edits are never discarded without a choice (Save / Discard / Cancel).

Every terminal has a title strip: its name, or the kind with an ordinal ("shell 2", "claude 1"). Double-click the strip or use the palette's "Terminali yeniden adlandır…" to name it (up to 120 characters, no control characters, no leading `-`, nothing credential-looking; an empty name clears it). The name is saved with the project's terminals and comes back on restore. A fresh agent started with a name runs `raios run <agent> --name <name>`; renaming an agent that is already running only changes the local label, because the daemon keeps the name a run started with.

The terminals sit in a dock layout tree (`rwk-core::workbench::dock`): groups of tabs and splits with weighted shares, at most four levels deep and twelve panes. Every terminal starts in a group of its own, stacked in a column with equal heights. Arrange them by dragging: drag a terminal's title onto another title to make it a tab there, onto the middle of a pane to join that pane's group, or onto the edge band of a pane (the outer quarter, highlighted while you drag) to split it off on that side; drag the thin divider between panes to resize. Every gesture has a palette command for the keyboard: "Terminali öncekiyle / sonrakiyle yer değiştir", "Terminali komşusunun sağına / altına böl", "Terminali öncekinin sekmesi yap", "Terminali sekme grubundan ayır", "Terminali büyüt / küçült". A refused move (a lone pane cannot be split from itself, nesting too deep) changes nothing and says why. Moving a terminal never touches its process. The arrangement is saved with the project and restored for the shells that come back; agent terminals rejoin at the end as their resume offers are taken.

Ending a terminal has three levels. "Durdur" ends the process and keeps the pane so the output stays readable. "Kapat" (Ctrl Shift W, the × on a title, a middle click on a title) ends it and removes the pane; a running process is asked about first, a pane whose process already ended is removed without a question. "Zorla öldür…" ends every process of the terminal at once with SIGKILL, without the grace period an agent's wrapper otherwise gets to close its run in the daemon, and says so in its question. Ending a process happens off the UI thread; the pane is gone from the layout immediately.

"SSH ile bağlan…" (Ctrl Shift S, the SSH toolbar button) opens a picker over the hosts in `~/.ssh/config` (aliases with their `HostName`, `User` and `Port`) and the plain names in `~/.ssh/known_hosts`. Typing filters them; anything typed that is a valid `[user@]host[:port]` (or `[ipv6]:port`) can be connected to as well. The target is validated and given to `ssh` as separate arguments after `--` (`ssh [-p port] -- [user@]host`), never as command text; option-looking text, whitespace and control characters are refused. The terminal is named after the target. A remote shell is not part of the saved workspace: reopening a project never connects anywhere by itself.

A toolbar above the panes holds the everyday commands (project, file and text search, save, terminals, split and resize, tasks and handoff, layouts, panels, palette). Every button is a catalog command: it is dimmed when the command cannot run, its tooltip gives the label and shortcut or the reason, a click on a dimmed button states the reason, and the dock and inspector buttons show a pressed state.

Right click opens a context menu on a terminal title, a terminal screen (unless the program inside asked for mouse reports; Shift takes the click back), the empty dock, an editor tab, a file-tree row and a task row. The click selects what it hit first, and the lines are the same catalog commands as the palette, with the same shortcuts shown and the same reasons when unavailable. Arrow keys, Home/End, Enter and Escape drive the menu; a click elsewhere only closes it; shortcuts do not fire behind it.

Tasks are the daemon's own operator tasks (shared with `raios task`, the TUI and VS Code). The inspector lists them; "Yeni görev" (palette) adds one, buttons change status or assign it to an agent kind (claude, codex, opencode, agy), and a task can be dragged onto an agent terminal's title or body to assign it to that agent. Every change goes through the consent guard and the daemon's audit ledger; an assignment carries the revision the user saw, so a task changed elsewhere is refused with "bu arada değişti" instead of overwritten. A shell terminal is never an assignee. "Devret…" on a task row opens the handoff composer seeded with the sending terminal's name and the task title. The task is attached to that handoff and moves to the receiving agent only after the receipt shows the receiver's own wrapper consumed it; an expired or rejected handoff leaves the task where it was. The receiver is started in a new terminal ("Alıcıyı başlat"); pasting into an already running agent is deliberately not offered.

`Ctrl+Shift+D` (or the palette) shows the inspector above the terminals, `Ctrl+Shift+A` starts an agent, `Ctrl+Shift+H` writes a handoff, the palette's "Proje hafızasını göster" opens the memory view and "GT-LCARS görünümünü aç / kapat" switches the skin; `Ctrl+,` switches dark and light, `Ctrl+Alt+.` / `Ctrl+Alt+,` move the dock divider.

The bottom bar shows toasts and, on its right, the quota signal read from `raios usage --json`; it refreshes every ten minutes and never blocks the window.

## Daemon-facing commands

Everything that changes daemon state (start an agent, queue a handoff, register a project) first reads the policy for that command: `deny` refuses, `allow` sends, `confirm` shows a dialog whose text is derived from the exact payload and sends only after a click (Enter never answers it). Declining, closing or letting it expire sends nothing. Opening a project the daemon does not know registers it through the same guard. The daemon token is read from `<config>/raios/.session_token` (owner-only, at most eight hours old) on every call; without the daemon, files, editor, terminals and memory keep working and the daemon commands say why they are unavailable.

The inspector lists the open project's tasks, running agents and pending approvals (read-only) by exact project path; records without a path are only counted. The handoff composer validates the note (size, option-looking text, control characters, credential-looking text) before anything is sent; the receipt is polled and the receiver can be started only while the freshly read receipt is pending and no owned receiver runs.

## Memory view

Shows `memory.md` as a heading outline with checklist progress and search, and the structured RAIOS records (type and layer filters) from a read-only connection to `workspace.db`. Links, images and HTML are shown as inert text; credentials in stored records are replaced by `[gizli]`. Budgets: 64,000 characters, 300 headings, 2,000 lines per section, 32 table columns.

## Editing, language servers, git and tasks

- **Language servers** start lazily per language and only from programs found on `PATH`
  (`rust-analyzer`, `typescript-language-server`, `pyright-langserver`, `gopls`, `clangd`). Add your
  own in `~/.config/raios-workspace-native/language-servers.json`, never in a project:
  `{"servers":[{"name":"zig","program":"zls","args":[],"extensions":["zig"],"language":"zig"}]}`.
  The program is a bare name, not a path. Without a server there are no problems, hover,
  completion, rename or formatting for that language; the footer says which server is missing.
- **Tasks** come from `~/.config/raios-workspace-native/tasks.json` (program and arguments, never
  a shell string) and run in a terminal.
- **Git** (source control view): stage, unstage, hunks, discard, commit, branches, stash, pull
  (fast-forward only) and push (asks first, never forces). Changed files carry a mark in the tree
  and on tabs.
- **Project search and replace** use `raios locate`; the pattern is a regular expression and
  replacing shows a preview before it writes. Files with unsaved edits are skipped.
- **Settings** (`Ctrl ,`): sections for appearance (skin, tone), editor (font size, tab width, word
  wrap), editing (auto-save, format on save) and the toolbar (four presets: tabbed, compact, flat,
  minimal; captions, shortcuts, per-category switches). Everything is remembered between runs.
- **Editor commands**: go to line (`Ctrl G`, `line` or `line:column`), comment or uncomment the
  selected lines (`Ctrl /`), zoom editor and terminal text (`Ctrl +`, `Ctrl -`, `Ctrl 0`), word wrap
  toggle, "who last changed this line" (palette: git blame), close other / all tabs (tab menu).
  Multi-cursor (`Ctrl Alt Up/Down`), code actions (`Ctrl .`) and gutter folding come from the
  editor widget.
- **Parameter hint** (`Ctrl Shift Space`) shows the signature of the call under the cursor with the
  current parameter between « », in the footer.
- **Project symbols** (`Ctrl T`) searches symbols of the whole project through the language server
  of the open file. **Recent projects** (`Ctrl Alt O`) lists the folders opened before.
- **Changes on disk**: open files and the file tree notice edits made elsewhere (checked every
  two seconds). A clean file reloads; one with unsaved edits is flagged instead of overwritten.
- **Terminal search** (`Ctrl Alt F`) searches a terminal's output, scrollback included.
- **Regex replace in the open file** (palette: "Dosyada regex ile değiştir"): live match count,
  `$1` inserts a group, one undoable edit. `Ctrl F` / `Ctrl H` stay plain text.
- `Ctrl`-click a `path:line` or a web address in terminal output to open it.

## Appearance

Skin (standard, GT-LCARS or Aperture, the black and orange KDE theme: its editor syntax colours and Konsole terminal colours included) and tone (dark or light) are independent and persisted; switching repaints every pane, the editor and the terminal surface without recreating sessions. The 28 KDE Breeze file icons are bundled unmodified (checksums in `crates/rwk-ui/assets/file-icons/provenance.json`, licenses in `crates/rwk-ui/assets/licenses`) and tinted by file type; Antonio (OFL) is the LCARS display font.

The palette's "Diff: yan yana / birleşik" switches the Git diff between one unified column and two aligned columns (old and new); the terminal shows IME pre-edit text at the cursor until it is committed.

File tree rows have a right-click menu to create a file or folder, rename and delete. Every operation goes through the project capability, never replaces an existing entry, refuses names with separators or control characters, deletes folders only when empty, and refuses to rename or delete anything open in an editor tab.

Known limits: approvals cannot be approved or rejected here (read-only until the contract carries an expected revision).

## Terminal view

`TerminalView` takes an `Arc<TerminalEngine>` and an `Arc<dyn TerminalHost>` and paints the engine's snapshots: backgrounds as merged quads, text as one shaped line per style run positioned at its own column. The parser thread's wake-up goes through `FrameGate` (at most one pending notification per frame) and an async task that calls `cx.notify()`.

Implemented: 16/256/24-bit colour with the 0.1 dark and light ANSI palettes, cursor shapes, selection and copy (`Ctrl+Shift+C`), paste (`Ctrl+Shift+V`; multiline pastes show a preview and wait for Enter/Esc; over 16 KiB is refused), keyboard encoding, committed IME text, SGR mouse reporting, wheel scrollback, resize.

Known limits: SGR (1006) mouse encoding only, no special box-drawing glyphs, one fixed monospace font, no accessibility tree.

## Tests

```bash
cargo test -q --manifest-path tools/raios-workspace-native/Cargo.toml          # everything that needs no display
RWK_UI_DISPLAY_TESTS=1 cargo test -q -p rwk-ui --test view_window              # real windows (reference machine)
cargo test -q -p rwk-ui --test editor_headless --test tree_headless --test workbench_headless   # editor, tree and workbench without a display
```

`rwk-ui/tests/view_headless.rs` drives the view on GPUI's test platform with real text shaping and real engine threads. The test harness forwards the parser thread's wake to the production wake closure from the test thread, because GPUI's `TestDispatcher` does not reliably schedule tasks woken from a foreign thread.

## Performance gates

```bash
cargo run --release --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-bench --bin term-bench -- --enforce --runs 5
cargo build --release --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-app -p rwk-ui-bench
tools/raios-workspace-native/target/release/ui-bench all --enforce --runs 5   # ui-bench finds rwk-app next to itself
```

Exit status 0 passed, 1 failed on a quiet machine, 3 inconclusive (busy machine, or the compositor delivered no frames to the window), 2 usage error. Frames are painted frames, not presented ones; keystroke latency excludes compositor and display.

## Live daemon check

`scripts/live_daemon_probe.sh` runs `DaemonClient` against a real `aiosd` built from this repository inside a throwaway user+network namespace (temporary config and database, so the real daemon, its ports and its token are untouched) and makes the real calls: policy, registration, handoff, receipt, launch validation. An `aiosd` installed before `GetCommandPolicy`/`GetHandoffReceipt` existed answers those queries with HTTP 422; the workspace then fails closed ("the daemon answered HTTP 422") until the daemon is rebuilt from this repository.

## Supply chain

GPUI is pinned (`gpui-pre =0.3.8`). Re-run `scripts/verify_gpui_supply_chain.py` against the lockfile on every GPUI change (see `docs/security/gpui-supply-chain-2026-10-06.md`).
