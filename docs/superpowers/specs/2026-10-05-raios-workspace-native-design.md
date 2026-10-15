# RAIOS Workspace Native — Design Spec

Date: 2026-10-05
Status: draft for owner review (design sections 1-6 approved in conversation; this document is the written spec awaiting review)
Replaces: `tools/raios-workspace/` (Tauri 2 + WebView) once feature parity is reached.

## 1. Intent

Owner goal: the fastest, most robust and most stable version of the workspace. Owner decisions, in order:

1. Full native Rust UI; drop Tauri/WebView.
2. Linux-first personal tool (KDE Wayland is the reference environment). macOS/Windows are out of scope.
3. First native release is feature-equivalent to Workspace 0.1.
4. GPUI toolkit, gated by a spike (done, see section 2).
5. Control plane access goes through `aiosd`; the UI authenticates as a local human actor (option "a").

Success is defined by measured budgets (section 9), not by impressions.

## 2. Spike findings that constrain the design

Measured on the reference machine (Intel UHD 620, 8 cores, KDE Wayland). Spike code lives only in the session scratchpad and is not part of this repository.

| Measurement | Tauri 0.1 (debug build) | GPUI spike (release) |
|---|---|---|
| First buffer attach | ~715 ms | 250-310 ms (editor, 5K lines) |
| Idle PSS | ~272 MB (3 processes) | ~86 MB (1 process) |

Constraints discovered:

- **Soft wrap must stay off.** `gpui-base` editor with soft wrap on takes ~1.4 s for a 5K-line file (about 0.26 ms/line, 50K lines unusable). With `soft_wrap(false)`: 30K lines 272 ms, 100K 328 ms, 500K 579 ms.
- **Ecosystem is fragmented.** `gpui-component`/`gpui-kit` 0.7.1 pins `gpui-pre =0.3.8` (published 2026-10-05, snapshots ship daily). `gpui-terminal` targets the older `gpui 0.2.2`. Porting it required 4 mechanical fixes.
- **`gpui-terminal` is demo grade and must not be used as-is:** unbounded reader queue (no backpressure; 430-540 MB under flood), parsing on the UI executor without yielding (1 frame in 6 s during a flood), `Event::PtyWrite` ignored (terminal queries such as cursor position and device attributes are never answered). A 4 ms parse slice restored ~20 fps but dropped throughput to Konsole/xterm level (1M plain lines 1.3 s, 300K colored lines 2.9 s).
- Reference drain times (1M plain lines / 300K colored lines): Ghostty 0.68 s / 2.1 s, Konsole 1.3 s / 2.4 s, xterm 1.2 s / 1.8 s.
- Licenses: `gpui-component` Apache-2.0, `gpui-terminal` MIT OR Apache-2.0; both compatible with this repository's AGPL-3.0.

Not measured: keystroke-to-photon latency, Tauri/xterm.js terminal throughput, Wayland IME and clipboard behavior, real 4-pane rendering in one window.

## 3. Non-goals

Daemon-hosted PTYs, LSP, macOS/Windows support, installer and signing. Two items once listed here are delivered and described in section 8: actionable approvals (Plan 7 Phase C) and session restoration (Plan 7 Phase E).

## 4. Process model and control-plane boundary

Processes: one native GPUI application, the existing `aiosd` daemon, and PTY children owned by the application (shell, `raios run <agent>`). The terminal layer sits behind a trait so PTY hosting can move to the daemon later.

**Rule: reads may bypass the daemon; every mutation goes through the `aiosd` contract**, so policy and the hash-chained audit ledger apply in one place and the second policy enforcement point in 0.1 disappears.

| Operation | Path | Contract status |
|---|---|---|
| Task/approval snapshot and live updates | `Query::GetWorkSnapshot`, `Query::GetGovernSnapshot`, `/api/stream` events; the client filters by exact `project_path` (see correction C2) | exists |
| Launch agent | a non-spawning daemon command `ValidateAgentLaunch` (validate, policy, audit under the name `launch_agent`), then the UI spawns `raios run` in its own PTY. **Correction C5:** the existing `Command::LaunchAgent` makes the daemon open an external terminal window and must not be used | implemented in Plan 3b (`feat/workspace-native-p1c`: daemon command `ValidateAgentLaunch`, policy under `launch_agent`, audited as `validate_agent_launch`) |
| Create handoff | `Command::CreateHandoff` | **missing** |
| Handoff delivery state | `Query::GetHandoffReceipt` | **missing** |
| Register an unknown project | `Command::RegisterProject` or an existing CLI path (to verify) | **missing / to verify** |
| memory.md and RAIOS records view | direct `SQLITE_OPEN_READ_ONLY` projection, as in 0.1 | no contract; acceptable because it never writes |
| Files, git diff, PTYs | local (`cap-std`, `portable-pty`) | local |

Daemon unreachable: mutations fail closed with a visible "daemon unreachable" state; files, editor, terminals and the memory view keep working. The UI never starts the daemon silently; it offers the `raios hub start` action.

## 5. Crate layout

New nested workspace at `tools/raios-workspace-native/`. The Tauri version stays until parity, then it and `vendor/glib-0.18.5` are removed (the GTK3/WebKitGTK dependency disappears).

| Crate | Contents | GPUI |
|---|---|---|
| `rwk-core` | `project` (cap-std capability, opaque IDs), `documents` (2 MiB, UTF-8, SHA-256 revision, git diff), `control` (`ControlPlane` trait, `DaemonClient` over HTTP/WS, fake for tests), `memory` (read-only projection), `terminal` (section 6), `sessions` (max 4, owned process lifecycle) | no |
| `rwk-ui` | workbench, file tree, editor, terminal view, inspector, memory view, command palette, dialogs, command catalog and shortcut resolver, themes, 28 Breeze icons, file-type classifier | yes |
| `rwk-app` | `main`, window, wiring, appearance persistence | yes |

Dependency direction: `rwk-ui -> rwk-core -> raios-contracts`. `rwk-core` never imports GPUI, so it is testable headless.

Ported from 0.1 (logic reuse): process ownership and pidfd-based safe stop (`terminal.rs`, 463 lines), `files.rs`, `memory.rs`, `contracts.rs`. Rewritten: the TypeScript side (`workbench.ts`, `file-types.ts`, `memory-model.ts` are ~150 lines combined; the 800-line `main.ts` is replaced by GPUI entities and events).

To verify: (1) the legacy path-derived memory key may exist only in `raios-core`; if so, depend on it with a minimal feature set. (2) Whether `gpui-base`'s `test-support` feature suffices for view-level tests.

## 6. Terminal engine

Per session: PTY -> reader thread (64 KiB reads) -> **bounded channel** -> parser thread owning `alacritty_terminal::Term` behind a `FairMutex` -> UI.

- Backpressure: a full channel blocks the reader, the PTY fills, the producer slows. Memory stays bounded.
- Query replies: `PtyWrite` events are written back to the PTY. A cursor-position round-trip test verifies this.
- The UI thread does not parse. The parser thread signals at most once per frame; the UI takes a short lock and snapshots visible cells.
- Rendering: coalesce adjacent cells of identical style into one shaped run per row (the spike's `gpui-terminal` shaped every character separately), quads for backgrounds, cached shaped lines.
- Vendored with attribution (Apache-2.0/MIT): `colors.rs`, `box_drawing.rs`, `input.rs` (key to bytes), `mouse.rs` if needed. Everything else is rewritten.
- Parity features: 16/256/24-bit color with the existing dark/light ANSI palettes, `TERM=xterm-256color`, `COLORTERM=truecolor`, 2,000-line scrollback, bracketed paste with multiline preview confirmation and a 16 KiB cap, resize, selection and copy, SGR mouse reporting, IME and AltGr.
- Fallback if budgets fail: the 4 ms parse-slice approach proven in the spike.

## 7. Editor, workbench, theme and views

**Editor** (`gpui-base` `EditorState`): soft wrap always off; line numbers and folding on. Saves require a matching SHA-256 revision; 2 MiB limit; UTF-8 regular files only; unsaved changes block project/file/window switches. Git diff renders in a read-only editor view with diff colors. Syntax highlighting through `syntect` (pure-Rust `fancy-regex` feature, lazily loaded language sets) via the editor's `highlighter_factory`; 0.1 covered JS, JSON, Markdown and Rust, all supported. To verify: whether the highlighter interface is incremental and its cost on large files.

**Keyboard and commands:** the 0.1 `workbench.ts` catalog is ported 1:1: a typed `Command` enum, `available(&state) -> Result<(), Reason>` (palette shows why an action is unavailable), one dispatch path shared by buttons, shortcuts and the palette. GPUI `actions!` plus key contexts: reserved combinations are caught as actions before reaching the PTY; unreserved keys (Ctrl+C/K/S/L, Tab, arrows) go to the shell. To verify: Wayland IME/AltGr and the equivalent of 0.1's held-key repeat cancellation.

**Layout:** dock with draggable divider (drag and arrow keys), Ctrl/Cmd+1/2/3 balanced/editor/agent layouts, up to 4 terminal panes. Evaluate `gpui_base::dock::DockArea` first; if unsuitable, a flex split with `ResizeHandle`.

**Theme:** token based; skins standard and GT-LCARS times tones dark and light; `{skin, tone}` persisted independently and validated. Switching does not recreate editors or PTYs (editor styles via `set_editor_style`, terminal palette swap). Antonio font bundled with its OFL license. The existing contrast gates become pure unit tests: file icons >= 4.94:1, filenames >= 12.52:1 across all four appearances.

**File tree and icons:** the 28 Breeze SVGs with hashes and license texts move unchanged; GPUI `svg()` tints them. The file-type classifier is ported 1:1 (exact filename rules first, then case-insensitive extensions). Arrow navigation, Home/End, Enter and an accessible expanded/collapsed label are preserved.

**Memory view:** heading hierarchy (max 300), checklist progress, safe Markdown subset; raw HTML, links and images remain inert text. Budgets preserved: 64,000 characters, 2,000 lines, 32 table columns, plus an element-count budget replacing the ~5,000 DOM-node budget. `gpui-base` ships a Markdown text view; if it cannot disable links, images and HTML, a ~300-line renderer over the `markdown` crate's AST is written instead.

## 8. State, inspector, handoff and consent

**State:** a single `WorkspaceState` entity (project, documents, sessions, daemon connection state, snapshot). Async results apply through weak entity handles. Stale-result guards (project, view, query sequence) remain only for project switches and handoff observations.

**Saved workspace (Plan 7 Phase E):** the editor holds several documents as tabs, and what a project had open is stored per project — one owner-only atomic file at `$XDG_CONFIG_HOME/raios-workspace-native/workspaces/<sha256(project)>.json` — holding the tab list with cursor lines, the active tab, the layout preset and the terminals (shells, and agents with the conversation id when one is known). Saves are debounced by 500 ms on every change (tab opened, closed or activated, cursor moved, layout picked, session started or stopped) and flushed on project switch and window close, the moments the file has to be right. Opening a file never asks — it adds a tab and leaves the other buffers alone; the unsaved-edits guard sits on what would lose work instead: closing a dirty tab (Save/Discard/Cancel per tab), and a project switch or window close (one answer, then every dirty tab is saved in tab order). The tab strip is capped at `MAX_SAVED_TABS` (32).

**Session restore and offers (Plan 7 Phase E):** restore applies tabs and layout immediately and restarts shells, but never auto-launches an agent: each saved agent terminal becomes an offer row in the dock, matched against the recovery catalog (`raios resume --list --json --project <root>`, read off the main thread under a generation guard, so an answer arriving after a project switch is dropped). A remembered conversation id takes exactly that session back; without one, the offer takes the newest resumable session of the same agent that no other offer claimed; an id that has left the catalog is never silently replaced with a different conversation. Taking an offer bypasses nothing: it goes through the same launch guard as a fresh agent (`Guarded::ResumeAgent`, the `launch_agent` policy key), so the daemon records and audits the launch first, a denied, cancelled or failed resume leaves the offer in place for another try, and only a validated launch starts a pane running `raios resume <agent:native-id>` — the key is one validated argument, never command text. Offers are part of the saved state: a taken one becomes its pane's conversation id, an untaken one is offered again next time with the id it was matched to, and a dismissed one is gone.

**Inspector:** `Query::GetWorkSnapshot` and `Query::GetGovernSnapshot` plus `/api/stream` events (`ApprovalRequested`, `ApprovalResolved`, `AgentRunStateChanged`, `SnapshotUpdated`), filtered client-side by exact `project_path` equality (`UnifiedTaskDto` and `ScoredApprovalDto` already carry it). Records with no path, and ambiguous or global records, are omitted. A banner shows connection state (connected, degraded, unreachable). **Approvals are actionable since Plan 7 Phase C** (correction C3 resolved): `Command::ApproveHandoff` now records the control-plane owner's consent — the approval resolves as `approved` with the owner as `resolved_by`, and the addressed agent's own run later delivers through the assignee-bound `cp_mark_handoff_delivered`, the only path that applies the artifact, completes the task and flips a receipt's `delivered_by_target` — so an Approve button never reports delivery. `ApproveHandoff`/`RejectHandoff` carry the optional `expected_revision` the row was displayed at (`cp_approvals.revision` bumps on every transition), and a stale decision is refused with `CONFLICT` instead of being applied silently. Both decisions go through the policy + consent gate in the workspace; reject asks for a reason.

**Handoff composer:** queue receipt, receiver launch and wrapper consumption stay distinct. The composer records a `BLOCKER` continuation, not a task-success report. Client-side prompt validation (option injection, obvious secrets) plus daemon-side secret scanning. The 0.1 flag logic becomes a pure state machine in `rwk-core`: `Queued -> Launching -> Pending -> {Delivered, Consumed, Expired, Rejected, Missing, Unreadable}` plus owned-receiver liveness, as `fn next(state, observation) -> state`.
Rules: **Restart receiver** is enabled only when a freshly read receipt is Pending and no owned receiver runs; every retry re-reads the receipt (`GetHandoffReceipt`) first. Approved, expired or rejected receipts are terminal for that ID; later missing or stale reads cannot undo them. Only the target wrapper's delivery counts as delivered. Known limit inherited from 0.1: reading the receipt and launching are not atomic.

**Policy consent (corrected, C1):** the control-plane path already treats an authenticated command as the human confirmation for a `confirm` rule and records it as `tool_confirm` in the audit ledger (`dispatch_control_command`). The earlier `CONFIRMATION_REQUIRED` + nonce design is withdrawn: a process holding the token could fetch the nonce and resend, so it proved nothing about human presence.
1. Before sending a mutating command, the UI calls the read-only `Query::GetCommandPolicy { command_name }`, which returns `allow`, `confirm` or `deny` (no policy file means `confirm`, as in the dispatcher).
2. On `confirm`, the UI shows an in-workspace dialog whose operation text is derived from the exact payload it is about to send. Cancel, close or shutdown send nothing. 0.1's 120-second single-use request semantics stay local to the UI as a UX safeguard.
3. The daemon still enforces `deny` and a malformed policy file, and audits the command. Human presence is not provable by the daemon; the token remains the real boundary, as for the TUI and the VS Code extension.

**Errors and observability:** daemon `Problem` codes map to stable UI codes; transient conditions show toasts, persistent ones a banner. Terminal and clipboard content never enters the activity log. Closing the window prompts for unsaved edits and stops owned sessions; agent stop gives the wrapper up to 5 s to finalize its run.

## 9. Testing and performance gates

**Tests:** `rwk-core` headless: the 15 native and 8 pure tests from 0.1, plus the handoff state machine (all transitions), `DaemonClient` against a fake daemon built on contract types, the terminal engine over an in-memory PTY pair (cursor-position round trip, bracketed paste, resize), documents (revision conflict, symlink escape, 2 MiB), and the memory projection (a write attempt must fail; registered path-key collisions rejected). `rwk-ui` pure tests: contrast tokens, file-type classifier, command catalog, exact-modifier shortcut resolver, Markdown budgets. Integration against a real `aiosd` with an isolated `RAIOS_DB_PATH`, never the live database; whether the daemon accepts alternate ports for this is to be verified. Security tests: path traversal, symlink escape, PTY argv allowlist, prompt injection and secrets, nonce single use and expiry. No subscribed-model runs or live handoff consumption in tests. `cargo audit` is mandatory; `pnpm audit` no longer applies.

**Performance gates** (spike harnesses kept under `bench/`; targets are hypotheses, the first three held in the spike):

| Gate | Target | Spike reference |
|---|---|---|
| Cold start to first frame | <= 300 ms (A1, the whole application: <= 350 ms since 2026-10-14, see the gate report) | 250-310 ms |
| Editor open, 30K / 500K lines | <= 350 / <= 700 ms | 272 / 579 ms |
| 1M plain lines fully parsed | <= 0.7 s | 0.55 s (raw, no UI) |
| 300K colored lines | <= 1.0 s | 0.9 s (raw) |
| Frame rate during a 4M-line flood | >= 45 fps | ~20 fps (time-sliced) |
| Peak RSS during flood | <= 256 MB | 430-540 MB (unbounded) |
| 4 concurrent floods | >= 30 fps | ~13 fps (4-window proxy) |
| Idle PSS (project + 4 terminals) | <= 150 MB | 86 MB (single editor) |

Keystroke-to-photon <= 16 ms is measured in-app as event to presented frame; this excludes compositor and display and the report must say so. Gates run locally; CI is not assumed to have a GPU or Wayland.

## 10. Delivery phases

Each phase starts with a typed skeleton (types, traits, empty mocks) reviewed before logic is written.

- **P0** Contract and daemon prerequisites: `CreateHandoff`, `GetHandoffReceipt`, project registration, and the read-only `GetCommandPolicy` query. Independent of the UI, reviewed on its own. Detailed in `docs/superpowers/plans/2026-10-05-workspace-native-plan-1-daemon-contract.md`.
- **P1** `rwk-core` skeleton, ported logic, `ControlPlane` trait, `DaemonClient`, fake. First task: verify owners and checksums of the `gpui-pre*` crates.
- **P2** Terminal engine and performance gate. **Stop criterion:** if the gates fail, reassess before starting P3-P5.
- **P3** Editor, file tree, workbench shell, commands, keyboard.
- **P4** Inspector, handoff, consent dialog.
- **P5** Memory view, LCARS theme, icons, parity checklist against the 0.1 README; then remove the Tauri version and `vendor/glib-0.18.5`.

## 11. Changes outside the new workspace

| Addition | Layer |
|---|---|
| `Command::CreateHandoff`, `Query::GetHandoffReceipt` | `raios-contracts`, `raios-runtime` |
| Project registration command (or verified CLI path) | to be verified |
| `Query::GetCommandPolicy` (read-only; replaces the withdrawn nonce flow, C1) | contracts, runtime |
| `expected_revision` on `ApproveHandoff`/`RejectHandoff`, with `cp_approvals.revision` (Plan 7 Phase C, correction C3) | contracts, core, runtime |
| Session offers consume the existing `raios resume --list --json` / `raios resume <agent:native-id>` CLI surface; Phase E adds no daemon or contract surface | `raios-surface-cli` (unchanged) |

## 12. Risks

1. **GPUI churn:** exact version pin and a committed `Cargo.lock`; upgrades only through a deliberate checklist.
2. **Supply chain:** the owners and checksums of `gpui-pre*` crates are not yet verified (P1 gate).
3. **Wayland IME, clipboard and accessibility:** unverified; `accesskit` appears in the dependency tree but its usefulness is unknown.
4. **Repository hygiene:** the whole Tauri 0.1 tree (`tools/raios-workspace/`) and `db/workspace.rs` are uncommitted, and a half-finished `version/formats` refactor is in flight. Native work must start from a clean baseline commit on a separate branch, or P0 changes will mix with that refactor.
5. **Effort:** terminal engine ~2-3K lines, UI ~6-8K lines (estimates). Measured in weeks, not days; P2 is the first real decision point.
