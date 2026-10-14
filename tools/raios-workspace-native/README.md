# RAIOS Workspace Native

Nested Cargo workspace for the native (GPUI) RAIOS workspace. Design: `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`.

| Crate | Role | GPUI |
|---|---|---|
| `rwk-core` | project capability, documents, sessions, control plane client, consent gate, guarded commands, handoff state machine and receipt tracking, input validation, inspector and memory models, theme tokens, file-type classifier, terminal engine and its pure helpers | no |
| `rwk-ui` | terminal view (`terminal`), code editor and highlighter (`editor`), file tree (`file_tree`), inspector, handoff composer, memory view, bundled assets (Breeze icons, Antonio), workbench shell, palette and dialogs (`workbench`) | yes |
| `rwk-app` | the application binary: window, folder picker, appearance persistence, daemon token source | yes |
| `rwk-bench` | headless terminal gates G1-G8 (`term-bench`) | no |
| `rwk-ui-bench` | UI-bound gates U1-U5, E1-E2, A1-A2 (`ui-bench`) | yes |

## Running the application

```bash
cargo run --release -p rwk-app -- /path/to/project
```

`Ctrl+O` picks a folder, `Ctrl+Shift+P` opens the command palette (it lists every command with its shortcut and, for unavailable ones, why). `Ctrl+S` saves, `Ctrl+Shift+T` opens a terminal (up to four), `Ctrl+1/2/3` switch the layout. While a terminal has focus only the shortcuts that carry Shift or Alt are taken by the workbench: plain Ctrl keys (`Ctrl+C`, `Ctrl+K`, `Ctrl+L`, ...) reach the shell. Unsaved edits are never discarded without a choice (Save / Discard / Cancel).

`Ctrl+Shift+D` (or the palette) shows the inspector above the terminals, `Ctrl+Shift+A` starts an agent, `Ctrl+Shift+H` writes a handoff, the palette's "Proje hafızasını göster" opens the memory view and "GT-LCARS görünümünü aç / kapat" switches the skin; `Ctrl+,` switches dark and light, `Ctrl+Alt+.` / `Ctrl+Alt+,` move the dock divider.

## Daemon-facing commands

Everything that changes daemon state (start an agent, queue a handoff, register a project) first reads the policy for that command: `deny` refuses, `allow` sends, `confirm` shows a dialog whose text is derived from the exact payload and sends only after a click (Enter never answers it). Declining, closing or letting it expire sends nothing. Opening a project the daemon does not know registers it through the same guard. The daemon token is read from `<config>/raios/.session_token` (owner-only, at most eight hours old) on every call; without the daemon, files, editor, terminals and memory keep working and the daemon commands say why they are unavailable.

The inspector lists the open project's tasks, running agents and pending approvals (read-only) by exact project path; records without a path are only counted. The handoff composer validates the note (size, option-looking text, control characters, credential-looking text) before anything is sent; the receipt is polled and the receiver can be started only while the freshly read receipt is pending and no owned receiver runs.

## Memory view

Shows `memory.md` as a heading outline with checklist progress and search, and the structured RAIOS records (type and layer filters) from a read-only connection to `workspace.db`. Links, images and HTML are shown as inert text; credentials in stored records are replaced by `[gizli]`. Budgets: 64,000 characters, 300 headings, 2,000 lines per section, 32 table columns.

## Appearance

Skin (standard or GT-LCARS) and tone (dark or light) are independent and persisted; switching repaints every pane, the editor and the terminal surface without recreating sessions. The 28 KDE Breeze file icons are bundled unmodified (checksums in `crates/rwk-ui/assets/file-icons/provenance.json`, licenses in `crates/rwk-ui/assets/licenses`) and tinted by file type; Antonio (OFL) is the LCARS display font.

Known limits: no side-by-side diff, no IME pre-edit text, approvals cannot be approved or rejected here (read-only until the contract carries an expected revision).

## Terminal view

`TerminalView` takes an `Arc<TerminalEngine>` and an `Arc<dyn TerminalHost>` and paints the engine's snapshots: backgrounds as merged quads, text as one shaped line per style run positioned at its own column. The parser thread's wake-up goes through `FrameGate` (at most one pending notification per frame) and an async task that calls `cx.notify()`.

Implemented: 16/256/24-bit colour with the 0.1 dark and light ANSI palettes, cursor shapes, selection and copy (`Ctrl+Shift+C`), paste (`Ctrl+Shift+V`; multiline pastes show a preview and wait for Enter/Esc; over 16 KiB is refused), keyboard encoding, committed IME text, SGR mouse reporting, wheel scrollback, resize.

Known limits: no IME pre-edit text, SGR (1006) mouse encoding only, no special box-drawing glyphs, one fixed monospace font, no accessibility tree.

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
