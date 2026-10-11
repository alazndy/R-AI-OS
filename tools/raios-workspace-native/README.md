# RAIOS Workspace Native

Nested Cargo workspace for the native (GPUI) RAIOS workspace. Design: `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`.

| Crate | Role | GPUI |
|---|---|---|
| `rwk-core` | project capability, documents, sessions, control plane client, handoff state machine, memory projection, terminal engine and its pure helpers (`palette`, `keys`, `mouse`, `paint`) | no |
| `rwk-ui` | terminal view (`terminal`), code editor and highlighter (`editor`), file tree (`file_tree`), workbench shell, palette and dialogs (`workbench`) | yes |
| `rwk-app` | the application binary: window, folder picker, appearance persistence | yes |
| `rwk-bench` | headless terminal gates G1-G8 (`term-bench`) | no |
| `rwk-ui-bench` | UI-bound gates U1-U5, E1-E2, A1-A2 (`ui-bench`) | yes |

## Running the application

```bash
cargo run --release -p rwk-app -- /path/to/project
```

`Ctrl+O` picks a folder, `Ctrl+Shift+P` opens the command palette (it lists every command with its shortcut and, for unavailable ones, why). `Ctrl+S` saves, `Ctrl+Shift+T` opens a terminal (up to four), `Ctrl+1/2/3` switch the layout. While a terminal has focus only the shortcuts that carry Shift or Alt are taken by the workbench: plain Ctrl keys (`Ctrl+C`, `Ctrl+K`, `Ctrl+L`, ...) reach the shell. Unsaved edits are never discarded without a choice (Save / Discard / Cancel).

Known limits: no file-type icons, the LCARS skin renders as standard, agent/handoff/memory commands are disabled until P4/P5, no file-name search, no side-by-side diff, no IME pre-edit.

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

## Supply chain

GPUI is pinned (`gpui-pre =0.3.8`). Re-run `scripts/verify_gpui_supply_chain.py` against the lockfile on every GPUI change (see `docs/security/gpui-supply-chain-2026-10-06.md`).
