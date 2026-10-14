# Native workspace: parity checklist against Workspace 0.1 and the owner's requests (2026-10-14)

Evidence kinds: **T** automated test (names in `tools/raios-workspace-native/crates/*/tests`), **R** seen in the real window
on this machine (X11/XWayland, screenshot of the window itself), **D** verified against a real `aiosd`
(**D-iso** a daemon built from this repo in a throwaway user+network namespace, **D-live** the owner's own
running daemon after the update), **CI** green on PR #45/#46 (Ubuntu, Arch, Windows, macOS).

| Requirement | Evidence |
|---|---|
| Open a project (folder picker, Ctrl+O), lazy file tree | T tree_headless, R |
| File tree: Breeze icons by type, tones per skin/tone, folder open/closed glyph, header, hover, keyboard (arrows, Enter, left/right) | T assets/file_types/tree_headless, R |
| Editor: open, edit, save with revision check, dirty guards (file/project/window switch), 2 MiB limit | T editor_headless, workbench_headless, R (typed, Ctrl+S saved to disk) |
| Syntax highlighting (tree-sitter), no colour drift, large-file cut-off | T highlight tests, R |
| Git diff, unified and side by side (aligned columns, coloured) | T editor_headless, side_diff, R |
| Terminal: PTY shell, 16/256/24-bit colour, selection/copy, bracketed paste with multiline preview and 16 KiB cap, resize, mouse reporting, scrollback, IME commit and pre-edit text | T view_headless, term-bench, R (command ran, output shown) |
| Up to four sessions, third pane switches to the agent layout, stop asks first, window close stops sessions | T workbench_headless |
| Sessions without a project offer the folder picker | T |
| Command palette (every command, shortcut, reason when unavailable), file quick open, toast auto-dismiss | T, R |
| Shortcuts: palette, new terminal, terminal toggle, focus editor/files, agent, handoff, inspector (Ctrl+Shift+D), layouts 1/2/3, settings, dock resize | T commands, R (Ctrl+Shift+P/T/A/H/D/J/E) |
| Dock divider: drag and keyboard | T |
| Skins standard and GT-LCARS × dark and light, Antonio font, terminal and editor follow the theme, persisted | T theme/skin, R (all four appearances) |
| Inspector: tasks, runs, pending approvals (read-only) of the open project by exact path, connection banner | T inspector, R, D-live |
| Memory view: memory.md outline with checklist progress and search, RAIOS records with type/layer filters, credentials redacted, inert links/HTML | T memory_model/workbench_headless, R, D-live (reads the live database read-only) |
| Policy-guarded daemon commands (deny / allow / confirm with exact payload, Enter never answers, decline/expire sends nothing) | T consent/workbench_headless |
| Project registration (auto for unknown projects, remembered once confirmed) | T, R + D-live (dialog → Onayla → daemon "Proje zaten kayıtlı") |
| Handoff: composer validation, queue, receipt tracking, receiver start only on a fresh pending receipt | T, D-iso (queue, receipt pending, NOT_FOUND), R + D-live (composer → dialog → Onayla → daemon held the approval; removed afterwards with `raios handoffs remove`) |
| Agent launch: chooser, policy, validation, terminal running `raios run <agent>` | T, D-iso (validation Ok), R + D-live (Antigravity: dialog → Onayla → terminal with the wrapper's output; stopped on exit, no prompt sent) |
| Daemon token handling (owner-only file, 8 h, re-read per call), fail-closed with a banner when unreachable or stale | T, R (banner shown against a wrong token), D-live |
| Performance gates U1–U5, E1–E2, A1–A2 | `docs/superpowers/reports/2026-10-13-workspace-native-ui-gates.md`: all pass on a quiet machine; A1 is tight (297.7 ms vs 300) |
| CI for the native workspace (Ubuntu and Arch, headless), supply-chain check, `cargo audit` | CI |
| Tauri 0.1 removed from the working tree | moved (not deleted) to `~/dev/archive/raios-workspace-tauri-0.1/` |

## What was not verified, and why

- An agent session *doing work* (a prompt sent, a model answering): it spends the owner's agent quota. The launch path up to and including the running wrapper terminal was exercised live.
- Wayland-native windows: the checks ran under XWayland.
- Windows and macOS: only the headless suites run there (CI); the application targets Linux.
- Approving or rejecting approvals from the workspace: out of scope by design (read-only until the contract carries an expected revision).
- The installed `raios` CLI binary was not rebuilt; only `aiosd` was updated (backup `~/.local/bin/aiosd.bak-20261014`).
