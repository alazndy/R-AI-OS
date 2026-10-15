# Changelog

## Unreleased

- **Workspace-native Plan 7, Phase E (tabs and session restore):** what a project had open now survives a restart, without ever doing work the user did not ask for. The editor holds several documents as tabs: opening a file never asks — it adds a tab and leaves the other buffers alone, the guard sits on what would lose work instead (closing a dirty tab asks Save/Discard/Cancel per tab; a project switch or window close takes one answer and then saves every dirty tab in tab order through the workbench's save queue), a dirty tab carries its marker, reopening an open dirty file only brings its tab forward, and the strip is capped at `MAX_SAVED_TABS` (32) with a named refusal (`CommandId::CloseTab` is the new Ctrl+W; catalog 36). Per project the workbench stores one owner-only atomic state file (`$XDG_CONFIG_HOME/raios-workspace-native/workspaces/<sha256(project)>.json`) holding the tab list with cursor lines, the active tab, the layout preset and the terminals; every change notes the state behind a 500 ms debounce (`WorkbenchEvent::WorkspaceChanged`, the same hand-off `rwk-app`'s `FsWorkspaceStore` persists) and a project switch or window close flushes immediately. Restoring applies tabs and layout right away and restarts shells, but never auto-launches an agent: saved agent terminals become offer rows in the dock, matched against `raios resume --list --json --project <root>` (read off the main thread under a generation guard, so an answer for a changed project is dropped). A remembered conversation id takes exactly that session back, otherwise the newest resumable session of the same agent that no other offer claimed, and an id that has left the catalog is never silently swapped for a different conversation. A row offers Sürdür/Vazgeç (an unmatched row only Vazgeç, an in-flight one is disabled), and taking one bypasses nothing: the continuation goes through the same launch guard as a fresh agent (`Guarded::ResumeAgent`, the `launch_agent` policy key — the daemon records and audits the launch first), a denied, cancelled or failed resume leaves the offer in place for another try, and only a validated launch starts a pane running `raios resume <agent:native-id>` with the key as one validated argument (`SessionLaunch::{Fresh,Resume}` is the new launch parameter threaded through the session registry and command builder; native ids are allowlisted — alphanumeric or `_` start, alphanumerics, `-` and `_` inside, 128 bytes at most — and a key whose agent is not the pane's kind is refused). Offers are part of the saved state: a taken offer becomes its pane's conversation id, an untaken one is offered again next time with the id it was matched to, and a dismissed one is gone. Covered in the nested native workspace by rwk-core's lib (170) and all integration targets (resume-key validation and the refusals, `raios resume` command building against the argv allowlist, catalog parsing and offer planning — exact-id match, the newest-unclaimed heuristic, a left-the-catalog id left alone), and by `workbench_headless` (70) + `editor_headless` (15) + `rwk-app` (15): the test rig's scripted `raios` answers the catalog from a `.catalog.json` file and logs every argv, so the offers tests prove the offer appears after a restore, that Sürdür reaches the daemon as a `launch_agent`-keyed launch whose pane runs `raios resume <exact key>`, that the next saved state carries the conversation id, that a denied resume keeps the offer with no process started, and that a dismissed or unmatched offer is not offered again. Known limit unchanged: the real-window check cannot run on this machine (an agent-launched GPUI window receives no frame callbacks), so the headless suite stands in.

- **Workspace-native Plan 7, Phase C (in-app approvals):** approvals become decisions. `ApproveHandoff` now records the control-plane owner's consent, not delivery: the approval resolves as `approved` with the owner as `resolved_by`, the waiting run leaves the queue as `handover_approved_by_owner`, and the task stays `awaiting_approval` until the addressed agent's own wrapper consumes the handoff through the new assignee-bound `cp_mark_handoff_delivered` — only that second step applies the artifact, completes the task, writes the delivery audit event and flips a receipt's `delivered_by_target` (an owner approval alone never claims delivery). A wrapper meeting an owner-approved handoff now delivers it without asking again; its "already completed" answer marks delivery directly instead of borrowing `ApproveHandoff` as consent; and a newer handoff supersedes an owner-approved one that was never delivered, while a handoff the user closed is never touched. Every status transition bumps a new `cp_approvals.revision` column; `ApproveHandoff`/`RejectHandoff` accept an optional `expected_revision` (all `#[serde(default)]`, so existing callers keep working — the TUI still passes none) and refuse a stale one with `CONFLICT` instead of silently acting on state that changed since it was read. Both pending-approval queries and the now snapshot now resolve the approval's project root (registered path, else the newest run-contract workspace, else the legacy slot — the Phase A resolution reused), the requesting run's agent and the task's assignee, and expose the revision, so the native inspector can present and decide them: handoff rows gain Approve/Reject behind the existing policy and consent gate, Reject asks for a reason in a small modal that refuses empty and credential-shaped text before anything is sent, a `CONFLICT` answer surfaces as an error instead of a phantom decision, and both decisions only report consent (the toast says so, no process is started). Covered in the main workspace by raios-core 544 + raios-runtime 517 tests (revision contract and `CONFLICT`, owner-approval vs delivery split with audit-chain assertions, supersede rules, snapshot contents) and in the nested native workspace by the client wire tests (ApproveHandoff/RejectHandoff payloads including `expected_revision`, `CONFLICT` mapping, wrong-status refusal, local refusal of a reasonless rejection) plus headless workbench tests (allow and confirm paths, reason trimming, empty/secret refusal, Escape).

- **Workspace-native Plan 7, Phase D (project search):** the workbench gains "Projede ara" (`Ctrl Shift F`, command palette) — a search overlay over the RAIOS CLI's trigram index (`raios locate -j`), typed in the new `rwk-core::search`. `SearchSource` is the seam (the app wires `CliSearch`, tests script a fake); the CLI path is resolved per call so a `PATH` change is picked up, and the query is validated before any process starts (empty refused — the CLI matches every line on `""` — a leading `-` refused against flag injection, NUL or over 512 bytes refused). The process boundary is bounded: stdin closed, stdout and stderr drained on their own threads, a 15 s budget (the plan's 5 s was measured impossible: a warm daemon answers in 25–47 ms, but the first cold index build took 9.8 s and a daemon-down fallback 4–6 s), and a hung CLI is killed. Failure messages drop the CLI's `locate failed: ` prefix, whitespace-only stdout is an empty result, paths are made project-relative with `/` (canonicalize retry when the literal prefix fails), and results are truncated client-side at 200 hits with a `truncated` flag (the CLI has no limit flag). The overlay (`rwk-ui::search`) runs each search on the background executor under a generation guard, so an answer for a superseded, closed or project-changed search is dropped. Enter runs the query, and once that query's results are on screen Enter opens the highlighted hit — a property the tests forced: the input emits an extra `Change` on Enter, so the first design's dirty flag made the second Enter re-run instead of open; the text is now compared against the last submitted query. Arrows move the highlight, Escape closes, and the case toggle re-runs. A hit opens the file through the same unsaved-edits guard as the file tree and palette (`PendingAction::OpenFile { path, line }`); the one branch that used to be a no-op — the same clean file — now reopens it to move the cursor, because a hit with a line is not a no-op. `EditorView::open_path_at` applies the 1-based line right after `set_value` (converted to a 0-based position; a failed read clears it) and `cursor_line` exposes the cursor for tests. The command catalog grows to 35, available exactly when a project is open. Covered by 7 core unit tests, 8 scripted-CLI integration tests (arguments, relative paths, limit, empty/garbage/failing/hung/missing program; the suite serializes its spawns because parallel `fork` while another test's script sat between `write` and `exec` reproduced ETXTBSY "Text file busy"), 5 headless workbench tests (run-on-Enter then open-at-line with the cursor asserted, Escape, case-toggle re-run, stale-answer drop, project switch closes the overlay) and 2 catalog tests.
- **Workspace-native Plan 7, Phase B (live inspector):** the inspector no longer waits up to three seconds to show a change — it subscribes to the daemon's typed event stream. New `rwk-core::control::stream::WebSocketEvents` opens `GET /api/stream` with the same bearer token the HTTP client uses (re-read on every reconnect, because the daemon rotates it), maps the typed `Event` frames — `AgentRunStateChanged`, `ApprovalRequested`/`ApprovalResolved`, `SnapshotUpdated` — into control events, and reports `Connected`/`Unreachable` on every transition; loose `{"event": …}` frames and unknown event types are skipped, because the inspector reacts to the typed contract only. The stream is an accelerator, never a requirement: a refused port, an unreadable token or a dropped socket becomes an `Unreachable` transition and a backoff retry (250 ms doubling to 10 s) while the three-second poll stays on exactly as before. On a typed event the panel refreshes immediately (a burst — a handoff publishes several — is one refresh). The visible banner folds the two states without touching the model: the query state always wins (a stale list matters more than a missing accelerator) and a stream-only failure shows as a note — "Canlı akış yok (yoklama sürüyor): …" — never as an alarm, because the poll still runs; `Workbench::state().daemon_reachable` keeps reading the query state only. New dependency: `tungstenite 0.24` (default `handshake` feature only, no TLS) — the same crate family as the daemon's already-vetted `tokio-tungstenite 0.24`; tokio and TLS stay out of `rwk-core` (the dependency-boundary script is green and the native lock's `cargo audit` is identical to baseline). Covered by five socket tests against a real local WebSocket server (the upgrade must carry `Authorization: Bearer` and `Host`, loose and unknown frames produce nothing, a dropped socket is reported and reconnected, a refused port backs off, a missing token fails closed with `Unauthorized`), banner/mapping/monitor unit tests in `rwk-ui`, and a headless workbench test in which a gated event source must refresh the model to a snapshot that is only reachable after a fetch, within 2.5 s — before the three-second poll could have fired. `rwk-app` gains a read-only `examples/stream_probe` to tail a running daemon (live evidence: connected in 2 ms against the installed daemon, which predates the typed events). Known limits: the real-window tests cannot run on this machine (a GPUI window receives no frame callbacks in an agent-launched process — Plan 4b Task 10), so the window check remains for the reference machine.
- **Workspace-native Plan 7, Phase A (daemon foundation):** `UnifiedTaskDto.project_path` now resolves the task's real absolute project root — the registered `projects.path` when `project_id` resolves, otherwise the newest run-contract workspace root recorded for the task's runs (wrapper and handoff tasks are launched against directories that were never registered; a live check found only 35 of 146 active tasks reachable through `project_id` alone and 145 of 146 through the run contract), with the legacy list-item slot kept as the last fallback for pre-registration personal tasks. The display name moves to the new additive `project_name` field (`#[serde(default)]`), so older payloads keep deserializing. The daemon now actually emits the typed event contract it previously only defined: handoff dispatches publish `ApprovalRequested`, `ApprovalResolved` and `AgentRunStateChanged` on the same broadcast bus every transport already subscribes to, strictly after the dispatch transaction commits and always with the status read back from the database — never an assumed constant — while an idempotent replay emits nothing a second time. No existing listener changes behaviour: the log ring buffer still keys on `v["event"]` and no loose payload format was touched. Covered by four new `raios-core` query tests and an in-crate end-to-end test that serves the real HTTP/WebSocket router on an ephemeral port with a temporary database: a real WebSocket client receives the create/approve/reject events for queued handoffs, and a real snapshot query resolves both project-root branches. `tokio-tungstenite` is added only as a dev-dependency (0.24.0 was already in the lockfile through axum's `ws` feature, so the shipped dependency surface is unchanged).
- **ANKA on Windows:** the ANKA suite now states its platform instead of failing on it. The path model (asserted project paths, the policy home, directory `fsync`) is POSIX-only by design, so on Windows `AnkaPolicy::init` fails closed with "ANKA cannot normalize the home path …" and every publish would report its durability as unconfirmed; the tests that assert those semantics are `#[cfg(unix)]` and no longer run on Windows. This restores the Windows CI job, which #53 had left red (same failing job on master's merge commit).

- **Native workspace application (P3):** new binary `rwk-app` opens one window with the Workspace 0.1 workbench: a native folder picker, a lazily listed file tree (directories first, symlinks and build directories skipped, listings dropped when the project changed meanwhile), a code editor on `gpui-base` (soft wrap off, line numbers, folding, revision-checked atomic saves, conflict detection that keeps the edits, a read-only Git diff view, files over 1 MiB open without highlighting), up to four terminal sessions in a dock, a typed command catalog ported one to one from 0.1 (28 commands, exact-modifier shortcuts resolved before the focused terminal sees the key, reasons for unavailable commands, command palette), and guards that ask before unsaved edits are lost when switching file or project or closing the window. Syntax highlighting runs on tree-sitter (Rust, JSON, Markdown, JavaScript, TypeScript): parsing is incremental and the highlight query covers only the visible range (30K lines parse in 0.38 s, a visible-range query takes 0.4 ms). A first implementation on `syntect` parsed only about 3K lines/s (15.5 s for 30K lines) and was replaced. Pure logic lives in `rwk-core::workbench` (commands, shortcuts, palette, `DocumentSession`, appearance) and is tested without a window. The appearance (`standard` skin, dark or light tone) is stored under `$XDG_CONFIG_HOME/raios-workspace-native/appearance.json`, owner-only. `ui-bench` gained the editor-open (E1, E2), application cold-start (A1) and application idle-memory (A2) gates and reports NOT MEASURED when the compositor delivers no frame callbacks. Known limits: no file-type icons, the LCARS skin renders as standard, agent, handoff and memory commands answer "Bu sürümde henüz yok", no file-name search, no side-by-side diff. New third-party dependencies: `tree-sitter 0.27` and the Rust, JSON, Markdown, JavaScript and TypeScript grammars (all MIT, C parsers built by the crates' build scripts); `cargo audit` reports no vulnerability.

- **Native workspace terminal view (P2b):** new crate `rwk-ui` renders a terminal session with GPUI (`gpui-pre =0.3.8`, supply-chain verified): batched background quads and one shaped text run per style run, cell metrics from the font, a resize that reaches the engine and the PTY only when the whole-cell grid changes, keyboard input (xterm key table with application-cursor mode, Alt prefix, modified arrows, function keys), committed IME/AltGr text, SGR mouse reporting only when the program asked, selection with copy, wheel scrollback, and paste with a multiline preview confirmation and a 16 KiB cap. The pure parts live in `rwk-core::terminal` and are tested without a window: `palette` (the 0.1 ANSI palettes, xterm cube and greys), `keys`, `mouse` and `paint` (a toolkit-neutral paint list). The new crate `rwk-ui-bench` (`ui-bench`) measures the UI-bound gates U1-U5 (cold start, frame rate under one and four floods, keystroke echo latency, idle PSS) and reports INCONCLUSIVE, never PASSED, when the compositor delivers no frames or the machine was busy. Known limits: no IME pre-edit rendering, SGR mouse only, no box-drawing special casing, fixed font. Headless view tests run on GPUI's test platform with real text shaping; window tests need `RWK_UI_DISPLAY_TESTS=1` and a compositor that delivers frame callbacks.

- **Native workspace terminal engine (P2a):** `rwk-core::terminal` now runs terminal sessions: a PTY host with Workspace 0.1's process ownership (pidfd-based stop, agent wrappers get 5 s to close their run), an engine with a bounded reader/parser pipeline (memory stays bounded however fast a program prints), 8 KiB parse slices so a snapshot never waits behind a long parse, style-merged drawable snapshots, scrollback, selection, answered terminal queries (cursor position, attributes, theme colors, text area size), OSC 52 clipboard access disabled, bracketed paste that strips escape characters, and a bounded input queue so a child that stops reading its input cannot freeze the caller. New direct dependencies: `alacritty_terminal =0.25.1`, `portable-pty` 0.9, `libc` 0.2. The new crate `rwk-bench` (`term-bench`) measures the headless performance gates G1-G8 and reports INCONCLUSIVE instead of FAILED when other processes were using the machine. 162 tests in the native workspace.

- **Daemon command `ValidateAgentLaunch` (P0b):** validates, authorizes and audits an agent launch without starting any process, for clients that run the agent in their own PTY. It shares `LaunchAgent`'s validation (known agent, registered project, bounded prompt without credentials) and is governed by the existing `launch_agent` policy rule, while the audit chain and the idempotency table record it as `validate_agent_launch`. `LaunchAgent` is unchanged and still opens an external terminal. The terminal launcher and the policy loader are now injected into `dispatch_with_seams`, so tests can prove which commands start a process and apply a policy without touching process-wide state. `rwk-core`'s `DaemonClient::launch_agent` sends only the new command, accepts only the result status `validated` and fails closed against a daemon that does not know it (HTTP 422). The native workspace core now has 106 tests.

- **Native workspace core logic (P1b):** `rwk-core` now implements the project capability (listing, 2 MiB UTF-8 documents with SHA-256 revisions, conflict-checked atomic saves, symlink-safe paths, `git diff`), the four-session registry, the handoff lifecycle state machine with its nine executable specifications, UI-local consent, the read-only memory projection with an injectable secret redactor, a scriptable fake control plane and the daemon HTTP client. Every daemon failure fails closed. 103 tests, no stub left. `DaemonClient::launch_agent` deliberately fails closed because the daemon's `LaunchAgent` opens an external terminal window; a non-spawning `ValidateAgentLaunch` command is the next plan.

- **Control-plane handoff and registration contract:** added `Command::CreateHandoff` (queues a `BLOCKER` handoff inside the dispatcher's transaction, so idempotency, owner binding and the hash-chained audit entry apply), `Command::RegisterProject` (canonicalizes the path, never overwrites an existing registry row), `Query::GetHandoffReceipt` (project-scoped; `delivered_by_target` is true only when the addressed agent's own wrapper consumed the handoff, never for a control-plane approval) and `Query::GetCommandPolicy` (read-only view of the policy action a command would receive). `create_handoff_workflow_in_tx` lets callers that already hold a transaction reuse the workflow without a nested `BEGIN`.

- **Native workspace core skeleton (P1a):** new nested workspace `tools/raios-workspace-native` with the UI-independent `rwk-core` crate: typed project, document, session, control-plane, handoff-lifecycle, memory and consent skeletons with `unimplemented!("P1b")` bodies and ten executable specifications, plus a recorded supply-chain verification of the GPUI crates (`docs/security/gpui-supply-chain-2026-10-06.md`, `tools/raios-workspace-native/scripts/verify_gpui_supply_chain.py`).

## v3.9.0 — 2026-08-18

- **VS Code daemon ownership:** Linux startup now delegates to `aiosd.service` and requires an authenticated TCP handshake before reporting readiness, preventing detached auto-spawn races with the systemd user service during desktop login.
- **Audit-ledger concurrency integrity:** standalone audit appends now acquire SQLite's writer slot with `BEGIN IMMEDIATE` before reading the predecessor hash, while callers already inside a larger control-plane or Product Factory transaction upgrade that transaction without opening a nested one. This prevents concurrent connections from selecting the same predecessor and forking the supposedly linear hash chain. A file-backed eight-writer regression test reproduces the old fork and verifies one linear chain; a separate test preserves transactional audit-call compatibility.
- **HTTP/2 denial-of-service advisory:** updated transitive `h2` from 0.4.15 to 0.4.16, resolving `RUSTSEC-2026-0258` (unbounded empty DATA frames). The lockfile change is intentionally limited to the patched crate and checksum; `cargo audit` reports zero known vulnerabilities.
- **Tray graphical-session startup:** the Linux user service is now enabled by `graphical-session.target` instead of `default.target`. `After=` alone did not pull the graphical target into the same boot transaction, so Qt could start several seconds before the Wayland socket existed, abort once, and recover only through `Restart=on-failure`. The unit remains ordered after and part of the graphical session, while a CI-backed service contract prevents regression to early boot ownership.
- **Detached child lifecycle:** the daemon's scheduler no longer drops spawned `Child` handles without waiting for them. A single process-wide reaper thread now polls all detached agent and extension children and consumes their exit status, preventing completed cron children from accumulating as zombies under `aiosd`; reaper startup/channel failure terminates and waits for the untracked child instead of leaking it.
- **SQLite maintenance and recovery:** added `raios db check [--full]`, `raios db checkpoint [--truncate]`, and `raios db backup [--keep 1..=10]`. Online snapshots are source- and destination-checked, SHA-256-recorded, owner-only on Unix, fsynced before success, and pruned to a bounded retention set without touching unrelated files. The recovery guide keeps restore offline and operator-controlled because `workspace.db` is shared by every agent and control-plane surface.
- **3.9.0 release:** moved all six Rust crates to one inherited `[workspace.package]` version, prepared the CLI/daemon as 3.9.0 and the VS Code extension as 0.9.0, and finalized the signed release metadata.

## v3.8.0 — 2026-08-02

- **`raios bootstrap` safety & genericization:** the command previously ran entirely without confirmation — silently installing global npm packages, adding unverified third-party GitHub repos as Claude Code plugin marketplaces, cloning and copying "rule" files from those repos into other AI agents' global config directories, and writing a hardcoded personal-identity `MASTER.md` to a hardcoded path. Now: the action list (npm tools, marketplaces/plugins, rule-sync repos) is config-driven under a new `[bootstrap]` section in `config.toml`, empty by default (an unconfigured install is a safe no-op); a pure `build_plan()` produces an inspectable, described action list before anything runs; `execute()` reports `Ok`/`Skipped`/`Failed` per action instead of silently swallowing errors, and is idempotent (already-added marketplaces, already-installed plugins, already-enabled plugins all report `Skipped`, not `Failed`, so re-running an already-bootstrapped machine exits 0); the CLI requires explicit confirmation (default No) before executing anything, with new `--dry-run` and `--yes`/`-y` flags; the hardcoded personal-identity file write is deleted, not relocated. Also fixes two issues found during review: the rule-sync git clone/pull target moved off a predictable `/tmp` path (a local hijack risk) to `~/.cache/raios/bootstrap/`; and the TUI setup wizard no longer silently wipes the whole `config.toml` (including hand-authored `[bootstrap]`/`[daemon]`/`[factory]` sections) on every run — it now loads the existing config and only overwrites its own fields.
- **Obsidian vault sync:** new `raios obsidian-sync [--vault <path>] [--dry-run] [--json]` command regenerates an Obsidian vault at `~/Obsidian` (one note per raios project, a `<category>-MOC.md` per category, and a root `Proje Atlası.md` index) from every project `raios_core::entities::discover_all_entities` finds on disk (a pure filesystem/git scan — deliberately not the DB-backed `load_entities` that `health`/`stats`/`commit` use, since the DB's `waiting`/`beklemede` lifecycle status was hiding 57 of 68 real projects) + each project's `memory.md`. `raios new` now runs the same sync automatically after scaffolding a project (unless `--no-vault`), replacing the previously dead `update_vault_atlas()` (silently no-op'd since the Linux migration — it only checked a hardcoded Windows path). Every category's MOC — including empty ones — is rewritten on every non-dry-run sync so a category doesn't go stale-and-orphaned when its last project disappears; the `tags:` frontmatter line is generated through a dedicated slugging helper so a category/status value containing a quote or other YAML-unsafe character can't break the note's frontmatter block; the CLI exits non-zero when a sync completes with errors.
- **Panic-vector hardening:** a full manual audit of every file flagged by `raios refactor` (79 files, HIGH+MED severity) found and fixed 4 real production panic vectors: `daemon/handlers.rs`'s WS connection handler mixed a safe `if let Ok(serialized) = serde_json::to_string(...)` pattern with 7 raw `.unwrap()` call sites doing the identical operation, unified on the safe pattern; `hub.rs`'s `probe_port()` round-tripped a Tailscale `IpAddr` through `format!`+`parse`+`unwrap` to build a `SocketAddr`, which would panic on an IPv6 address (no bracket notation) — now built directly via `SocketAddr::new`; `hub.rs`'s `cmd_logs` follow mode `.expect()`'d on spawning `tail`, now degrades to an error message if `tail` isn't on `PATH`; `intelligence/instinct.rs`'s `InstinctEngine::init()` `.expect()`'d on `dirs::home_dir()`, reachable from every `raios run`/`raios task` invocation via `agent_runner.rs`. Every other flagged file was read and confirmed to be either already-hardened code, test-only `.unwrap()`s, or type-level-infallible calls — not mechanically "refactored" for cosmetic nesting-depth scores alone.
- **Coverage floor raised 40% → 42%:** with tests added across CLI, TUI, MCP, and a new `new_project::create` integration test.
- **`raios-tray` audit:** a full first-time audit of `tools/raios-tray/raios-tray.py` (1909 lines, previously untouched by any tooling) found and fixed: a shell-quoting design flaw in `open_terminal()` where callers pre-quoted commands with POSIX `shlex.quote` semantics that don't apply once embedded into AppleScript or PowerShell — quoting responsibility is now centralized per target shell inside `open_terminal` itself; a live systemd service (`raios-tray.service`) that had drifted to reference a deprecated pre-consolidation `.venv` path instead of the canonical one; no pinned/reproducible dependency install (added `requirements.lock.txt`, a `pip freeze` snapshot — the one dependency surface in the repo without a lockfile, unlike `Cargo.lock`/`pnpm-lock.yaml`); and 9 silent `except Exception: pass`/`return ...` sites that swallowed network/auth/parse/OS failures with no logging, now logged at debug level to `~/.config/raios/tray.log`.
- **VS Code extension dependency fix:** `brace-expansion@5.0.7` (transitive via `@vscode/vsce` → `glob` → `minimatch`) was flagged HIGH severity by `pnpm audit` (GHSA-mh99-v99m-4gvg, DoS via unbounded expansion length) — pinned to the patched `5.0.8` via a `pnpm-workspace.yaml` override.
- **Systemd hub status accuracy:** `raios hub start/status` now resolves the process that owns port 42069 when a systemd-managed daemon has no legacy `aiosd.pid` file, reporting the real PID instead of incorrectly labelling the healthy service as orphaned. Covered by a Linux listener-owner regression test.
- **Health-worker SQLite contention:** project security scans remain three-way parallel, while their short activity-state transactions are serialized through a dedicated semaphore. Security-finding synchronization now starts with `BEGIN IMMEDIATE`, acquiring SQLite's writer slot before reading prior state so a competing writer is handled by the configured busy timeout instead of an immediate deferred-transaction lock-upgrade failure. A two-connection regression test reproduces the old `DatabaseBusy` failure and verifies the wait-and-succeed behavior.

## v3.7.1 — 2026-07-24

- **Factory existing-repository onboarding:** Factory can now attach an existing absolute local Git worktree without scaffolding, canonicalizing the path and verifying the repository root, `origin` remote, and full `HEAD` SHA before persisting owner-bound provenance. The typed CLI, MCP, and TUI surfaces share the canonical idempotent/audited dispatcher; HTTP(S) remotes with embedded credentials are rejected.
- **Release integrity:** the public release now ships pinned, cross-platform archives for `raios` and `aiosd`, SHA-256 checksums, GitHub build provenance attestations, and AGPL licensing material for the extension package.
- **Release gates:** CI now enforces Rust formatting, zero-warning clippy, a 42% line-coverage floor, Rust/Node dependency audits, VSIX packaging with a license, and staged installer smoke tests on Linux, macOS, and Windows.
- **Daemon safety coverage:** lifecycle status decisions are extracted into a pure, boundary-tested function; the compliance scanner now has regression tests for secret detection, language rules, package-manager policy, and score formatting.

## v3.7.0 — 2026-07-21
### Added
- **Product Factory (Phases 0-10)**: a full owner-bound, audit-logged product lifecycle layer — disabled-by-default typed config, workspace/product/intake/Charter drafting, Charter-backed requirement drafts, change-control and planning with approval-gated lifecycle cycles, evidence-gated stage completion, verified-release readiness, cycle pause/resume/cancel, dependency-aware evidence staleness, and a content-addressed (SHA-256) artifact store kept outside SQLite. Exposed via a bounded local TUI transport (`/factory workspace|product|intake|answer|charter`), new `raios factory overview` / `raios factory execute` CLI commands, and a policy-scoped MCP surface (`factory_overview` read-only, `factory_execute` blocking all human-only approval/cancellation/release actions).
- **React Native/Expo Factory support**: managed/prebuild/bare project detection, an idempotent closed-testing quality profile (TypeScript, Expo config, web export, dependency audit, Android/iOS device evidence), and `quick`/`governed` intake operating modes.
- **ANKA (Agent Narrative Knowledge Archive)**: a separate, owner-only, redacted, rebuildable transcript cache (`raios anka status|index|search|blame|forget`) exposed read-only to agents as untrusted historical evidence via MCP `anka_recall`. Never writes to `workspace.db`, auto-injects context, or promotes records to curated memory.
- **`raios wrapper-note "<text>"`**: lets a live `raios run` child (or a hook) record one explicit, project-bound follow-up note mid-session, over a loopback-TCP IPC path from the sandboxed child to the wrapper (an unguessable per-run UUID is the actual access-control boundary, not the transport). The wrapper validates run ownership, project match, length (≤500 chars), and secret-likeness before persisting to a new `cp_wrapper_events` table and the existing L0→L3 memory pipeline. Covered by a true end-to-end test over the real transport with two projects running in parallel, proving neither the note content nor the run-ID ownership check leaks across projects. A Unix domain socket was tried first and live-tested as unreachable from inside Codex's Landlock sandbox regardless of directory allow-listing; loopback TCP is reachable through that same sandbox once `sandbox_workspace_write.network_access=true` is set, so `raios run codex` (and the detached task runner) now always passes that override to every `codex` child it spawns — inert when Codex's effective sandbox is `read-only` or `danger-full-access`, but a deliberate, explicit widening of network egress for every shell command Codex's own agentic loop runs while wrapped, not just for `wrapper-note`.
- **Wrapper launch-prompt capture**: after a successful `codex`/`opencode` wrapper run, one explicit bounded positional launch prompt is persisted through the existing memory pipeline; flags, multi-argument invocations, oversized input, and secret-like text are rejected before capture.
- **TUI numbered route tabs**: the left-hand vertical route list is now a horizontal tab bar (`1 NOW  2 WORK  3 EXPLORE  4 GOVERN`) with direct number-key jumps; the header shows a two-line control-plane status band on taller terminals.
### Changed
- **Every `codex` child `raios` spawns now enables `sandbox_workspace_write.network_access`** (via a single `codex_command()` builder shared by `raios run` and the detached task runner), specifically so `wrapper-note`'s loopback-TCP channel is reachable without the caller needing to know Codex's sandbox internals. This is a real, security-relevant behavior change — it widens network egress for every shell command Codex's own agentic loop runs while wrapped, not just for `wrapper-note` — made deliberately after the transport-only fix (loopback TCP with default sandbox config) was live-verified to still fail identically to the original Unix-socket bug.
- **`RAIOS_DB_PATH` environment override** added to `raios-core`'s database path resolution, so integration tests can run against a real isolated SQLite file instead of either mocking the DB or touching the live `~/.config/raios/workspace.db`.
- Wrapper memory import scope narrowed: only Claude (project-bound) and AGY (workspace-bound) transcript histories auto-create project facts; Codex's and OpenCode's globally-scoped histories are intentionally skipped until their upstream formats carry trustworthy project/session identity.
- Periodic wrapper memory-sync shutdown now polls once per second instead of blocking session finalization for up to 90 seconds; informational `--help`/`--version` invocations no longer consume a pending handoff meant for an interactive session.
### Fixed
- `db_budget` false-positive query match, VS Code extension `CommandBridge` HTTPS/loopback URL validation, and a sidebar DOM/`textContent` refactor (workspace and extension security hardening pass).
- Clippy `single-char insert_str` lint in `anka.rs` — full workspace `cargo clippy --all-targets --all-features -- -D warnings` is clean again.
- `workspace.db` maintenance under explicit approval: backed up (2.9GB), rebuildable trigram/BM25 search indexes purged and VACUUMed to 66MB (~98% reduction), with verified post-VACUUM integrity and 100% preservation of `cp_*`/`mem_items`/`audit_log` data.
### Verification
- `cargo test --workspace --lib`: 732 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo audit`: no vulnerabilities; 3 pre-existing unmaintained/unsound advisories (`number_prefix`, `paste`, `lru`), none new or blocking.
- The wrapper-note E2E test was mutation-checked twice (once per transport): confirmed to fail with the expected assertion when the run-ID ownership guard was temporarily removed, then verified green again after reverting, for both the Unix-socket and loopback-TCP versions.
- Live-tested `raios wrapper-note` from real `codex` children across five configurations to pin down the exact fix: no flag (Unix socket, rejected) → `--sandbox workspace-write` (TCP, still rejected) → `--sandbox workspace-write -c sandbox_workspace_write.network_access=true` (TCP, succeeded) → `--sandbox danger-full-access` (succeeded) → finally, the ordinary `codex "<prompt>"` invocation with **zero explicit flags**, the exact form a real user types, succeeding end-to-end once the network-access override was made automatic.

## v3.6.0 — 2026-07-17
### Added
- **Typed TUI control plane:** introduced the serialization-only `raios-contracts` crate, coherent daemon snapshots, typed commands/events, idempotency caching, transactional audit logging, and four attention-first routes: Now, Work, Explore, and Govern.
- **Windows installation:** added `install-system.ps1` for locked release builds, `%APPDATA%\raios` configuration/policy setup, user `PATH` registration, and the `RAIOS_Daemon` Scheduled Task.
- **Windows runtime parity:** added native Scheduled Task daemon management, `netstat.exe` PID lookup, PowerShell lifecycle hooks, PowerShell agent wrappers, and portable tray startup paths.
- **Cross-platform CI:** Windows, macOS, and Linux now build and test the full Cargo workspace with the lockfile enforced.
### Fixed
- Removed clippy blockers in control-plane, search, and preflight test paths; full workspace clippy now passes with `-D warnings`.
- Corrected Windows installation documentation, config/token paths, and stale README version/agent references.
### Verification
- `cargo test --workspace`: **678 passed, 2 ignored, 0 failed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `pnpm audit --audit-level=high`: no known vulnerabilities.

## v3.5.0 — 2026-07-11
### Changed
- **`raios grep` renamed to `raios locate`** (MCP `grep_search` → `locate_search`), including the core engine (`trigram::grep` → `trigram::locate`, `GrepMatch` → `LocateMatch`) — the command name no longer collides with "grep" as a concept.
### Fixed
- **MCP `semantic_search` now delegates to the resident Cortex daemon**, same as the CLI's `raios search` since v3.4.0. It previously paid a full in-process `Cortex::init()` + `index_project()` + HNSW rebuild on *every single call* — measured >60s per call, never completing within a 60s timeout. Now: ~1s warm, ~30-60s one-time cost only on the first call after an `aiosd` restart.
- **Duplicate search results from stale git worktrees.** `SKIP_DIRS` never listed `worktrees`, so any repo carrying a Claude Code isolated-worktree checkout under `.claude/worktrees/<id>/` reported every match twice (once from the real source, once from the stale copy) — confirmed on GT-Launcher (56MB stale checkout).
- **`tool_pin` (MCP tool-manifest tamper detection) was blocking all `tools/call` requests** after `grep_search`/`semantic_search` were added to the tool list without re-pinning. Verified the drift was legitimate (this repo's own commits), re-pinned.
### Added
- **Dart/Flutter support**: `dart` added to `INDEXED_EXTS`, verified on two real Flutter projects (NEXUS, esp32flutter) with exact-parity results via both CLI and MCP.
- `.fastembed_cache/` added to `SKIP_DIRS` (was being walked as source content).
- `raios-policy.toml` explicitly allow-lists `locate_search`/`semantic_search` and loopback (`127.0.0.1`/`localhost`) domains — the latter needed for the resident-daemon TCP client introduced in v3.4.0, consistent with this codebase's existing SSRF-hardening design (loopback was already deliberately excluded from the metadata-IP blocklist for this exact reason, see 2026-07-02 in memory.md).
### Notes
- Verified end-to-end by two agents independently: Antigravity Kaira implemented and live-tested the Dart support + MCP tool_pin fix; Claude Kaira independently re-verified that work (diff content, test counts) before merging, then found and fixed the `semantic_search` daemon-delegation gap and the worktrees duplicate-match bug through its own live dogfooding — both previously undocumented. Combined suite: **635 tests, 0 failures**.

## v3.4.0 — 2026-07-10
### Added
- **`raios grep <pattern>` — trigram-indexed exact/regex search** (`[--dir <path>] [-i] [--reindex]`). Every file's content is indexed as lowercased 3-character windows in the shared `workspace.db`; queries extract the literal substrings a pattern requires, intersect candidate files in SQL, then regex-verify only those candidates. Exhaustive within scope (grep semantics, not top-k), **measured at 0.015s warm** on this repo, with proven set-identical output to `grep -rn` over the same scope. Patterns yielding no usable ≥3-char literal (alternations, short wildcards) fall back to a full scoped scan — always correct, just slower. New MCP tool **`grep_search`** exposes the same engine to agents (200-match response cap).
- **Resident Cortex in `aiosd` — semantic search is now sub-second.** A dedicated worker thread owns ONE long-lived Cortex (embedding model + HNSW), serving requests over an mpsc/oneshot channel with lazy dirty-flag rebuilds (file-change events mark dirty; the next search rebuilds once). `raios search` transparently delegates its vector half to the daemon (300ms connect timeout, `AUTH` handshake) and **silently falls back to the full in-process path when the daemon is unreachable** — measured: ~1.0s daemon-warm vs ~4-6s fallback, identical results. New daemon command `CortexReindex`; `VectorSearch` responses gain an additive `vector_hits` field (existing `results` shape untouched for TUI compatibility).
### Fixed
- Daemon's `VectorSearch` handler previously ran `Cortex::init()` — full model load + HNSW rebuild — **on every single request**; the file-watcher worker did the same per changed file, and its incremental `index_file` writes never triggered an HNSW rebuild at all (silently useless indexing). All three replaced by the resident-worker design above.
### Notes
- Built as two parallel agent worktrees (Codex: trigram; Antigravity: daemon residency) with an explicitly partitioned file surface; merged sequentially with a rebase. Combined suite: **634 tests, 0 failures** (619 → +10 trigram, +5 daemon).

## v3.3.0 — 2026-07-09
### Added
- **Layered memory (L0→L3), ported from TencentDB-Agent-Memory's semantic pyramid**: `mem_nodes` (immutable evidence: raw transcript lines, archived body revisions) + `mem_lineage` (derived-from/revision edges) give `mem_items` real traceability for the first time. `mem_items.layer` discriminates L1 atomic facts (deterministic hash-slugged, deduped), L2 daily scene digests (cumulative same-day merge with `[[slug]]` backlinks), and L3 a rolling persona (background + working rules, rebuilt from the newest L1 facts). All distillation is local/deterministic — no LLM calls.
- **`raios mem history <slug>`** and **`raios mem list --layer <n>`** — inspect a memory item's revision chain and filter by pyramid layer.
- **`raios sessions --canvas <session_id>`** — folds a session's `session_events` stream into a compact Mermaid flowchart; consecutive same-type events collapse into one node with a `se:<id>` back-reference to the full, untruncated payload — compression is never irreversible.
- **`raios usage` now reports live Claude Pro/Max quota remaining.** The statusLine script caches `rate_limits.five_hour`/`seven_day` usage percentages (from Claude Code's own stdin JSON) to `~/.claude/raios-usage-cache.json`; `raios usage` reads that cache (with a 24h staleness cutoff) and shows `5h:XX% 7d:YY% remaining` plus formatted reset times instead of a hardcoded "unknown".
### Fixed
- **`mem_items.body` unbounded growth**: `mem_upsert` previously concatenated every write onto the same row forever. It now replaces the body and archives the previous version as an immutable `mem_nodes` revision — the full history is still recoverable via `raios mem history`, but the live row stays bounded.
- **A second instance of the same bug, caught only by a whole-branch review**: the 90-second periodic memory-sync thread re-scans the entire session transcript on every tick (fixed start timestamp), and was inserting a fresh, undeduplicated `mem_nodes`/`mem_lineage` row per matched fact on every pass — silently recreating unbounded growth one layer down. Fixed with content-addressed dedup on `(project_key, kind, content)` for `l0_raw` nodes (revision nodes are correctly exempt — each is a genuinely distinct snapshot).
- `mem_upsert`'s archive-then-replace sequence (revision node + lineage edge + item update) now runs inside a single SQLite transaction instead of three unguarded autocommit statements.
- `raios security` / `raios refactor` output now discloses its own limitation inline ("pattern-based scan — a clean result is not proof of absence") instead of only in internal docs — both are regex/heuristic scanners, not semantic analysis.
- `gen-context.config.json` used an unrecognized `customOutput` key that `sigmap` silently ignored, so it never wrote `SIGMAP.md` (it was defaulting to `.github/copilot-instructions.md` instead). Corrected to the real `output` key.
### Changed
- `session_memory.rs` (974 lines after the memory-layering work) split into a `session_memory/` directory module — `transcript_io.rs`, `heuristics.rs`, `distillation.rs`, plus a thin `mod.rs` orchestrator. Pure move, no behavior change; full external call surface (`auto_sync_agent_memory`, `collect_transcript`, `decision_lines_from_transcript`, etc.) preserved.

## v3.2.0 — 2026-07-07
### Changed
- **Workspace split**: monolithic library physically split into a Cargo workspace of 5 crates (`raios-core`, `raios-runtime`, `raios-surface-cli`, `raios-surface-mcp`, `raios-surface-tui`) — the actual reason for the version jump from v3.0.0; no v3.1.0 was ever tagged.
- **Schema consolidation**: `task_graph_nodes`/`swarm_tasks` control-plane link columns moved into the single central migration (`db/schema.rs`); both stores previously carried their own duplicate `CREATE TABLE`/`ALTER TABLE` that had silently drifted out of sync.
### Fixed — security hardening pass (2026-07-07 session)
- `.ipc_token` (legacy daemon auth file) removed entirely — it duplicated `.session_token`'s secret without matching its owner-only (0600) permissions, defeating that hardening for any local reader. All clients now read `.session_token` only.
- Daemon AUTH handshake now uses a constant-time comparison (was a plain `!=`).
- Session tokens and the hub API key now come from a direct OS CSPRNG draw instead of a hand-rolled `SHA256(uuid‖pid‖time)` construction.
- `[server.hub] trusted_proxy` config added — a same-host reverse proxy no longer silently downgrades remote requests onto the localhost auth path.
- Windows: token files now get an owner-only ACL on write (`icacls`) **and** are verified on read, not just written — previously Windows had no permission enforcement at all.
- `raios hub api-key show` masks the key by default now (`--reveal` for the full value) instead of always printing it in full.
### Note
This entry summarizes the ~90 commits between v3.0.0 and this tag rather than itemizing each — see git log for full detail. Full findings/fix trail for the security pass: `docs/adversarial-review-2026-07-07.md`.

## v3.0.0 — 2026-06-25
### Added
- **4-agent identity matrix**: Claude Kaira, Codex Kaira, OpenCode Kaira, Antigravity Kaira — Gemini CLI fully retired (Google shut it down).
- **`raios handoff`** — atomic agent-to-agent handoff built entirely on the existing control plane (`cp_tasks` / `cp_agent_runs` / `cp_artifacts` / `cp_approvals`), no separate state file. `--to`/`--status` are clap-validated; `--msg` is heuristically scanned for secrets (AWS/Anthropic/OpenAI/GitHub keys, PEM blocks) and refused before it ever touches the DB. Auto-attaches `git diff --stat HEAD` so the receiving agent sees what changed without being told. A new handoff to the same `(agent, project)` atomically supersedes any still-pending one instead of letting stale handoffs pile up.
- **Real prompt delivery, not a dead env var** — `raios run`/`raios task` deliver the pending `[HANDOVER CONTEXT]` via each CLI's actual prompt-injection surface: `claude --append-system-prompt`, `codex <positional prompt>`, `opencode --prompt`, `agy --prompt-interactive`. Delivered exactly once (consumed only after the child process actually spawns).
- **`agy` (Antigravity CLI) support** — `raios run agy`/`raios run antigravity` now spawn the real `agy` binary instead of erroring "Unsupported agent".
- **TUI Inbox panel** — new 14th menu screen showing pending approvals (handoffs included), active agent runs, and blocked tasks straight from the control plane; previously this data was only reachable via the `get_inbox` MCP tool, invisible at the terminal.
### Fixed
- `create_handoff_workflow` now runs inside a real SQLite transaction (`unchecked_transaction`) instead of unguarded sequential inserts.
- `run_agent` only marks a handoff "consumed" after `cmd.spawn()` actually succeeds, not before.
- `cp_list_personal_tasks` was leaking workflow tasks (file-change-approval, handoff) into the personal-task checklist sidebar; now excludes any task with an attached `cp_approvals` row.
- Superseding a stale pending handoff now also cancels its `cp_agent_runs` row — previously it lingered as `awaiting_approval` forever, cluttering the Inbox panel.
- A stale, never-committed `lock_manager.rs` test left broken by the gemini→claude rename was asserting against itself.

## v1.5.1 — 2026-05-21
- (no commits since last tag)

All notable changes to the **R-AI-OS** project will be documented in this file.

## [1.5.0] - 2026-05-21 (Intelligence & Architecture Edition)

### Added

**Phase 5 — Agent Swarm Mesh:**
- `SwarmStore` — SQLite-backed worktree lifecycle management.
- 5 TCP commands: `CreateSwarmTask`, `GetSwarmTask`, `ListSwarmTasks`, `ApproveSwarmTask`, `RejectSwarmTask`.
- `raios swarm start|list|approve|reject` CLI subcommand.
- 3 MCP tools: `create_swarm_task`, `list_swarm_tasks`, `approve_swarm_task`.

**Phase 6 — Edge Intelligence:**
- `EdgeRouter` — cosine-similarity semantic routing of natural-language queries to capability names.
- `raios route "<query>"` CLI command.
- `route_capability` MCP tool.

**Phase 7 — Evolutionary Intelligence:**
- `CandidateStore` — learns instinct candidates from job success/failure outcomes.
- `start_evolution_worker` subscribes to daemon broadcast and auto-generates instinct candidates.
- TCP commands: `ListEvolutionCandidates`, `PromoteEvolutionCandidate`, `PruneExpiredCandidates`.
- `raios evolve list|promote|prune` CLI subcommand.
- MCP tools: `list_evolution_candidates`, `promote_evolution_candidate`.

**Phase 8 — Recursive Reasoning (Task DAG):**
- `TaskGraph` module — directed acyclic graph (DAG) of dependent shell commands.
- SQLite persistence with cycle detection, depth validation (max 50 nodes), 10-min timeout.
- TCP commands: `CreateTaskGraph`, `ExecuteTaskGraph`, `GetTaskGraph`.
- `execute_graph_async` — parallel execution of independent nodes via Factory Mode.
- 5 unit tests: create, ready_nodes, max_limit, mark_complete, cycle detection.

### Refactored

**Codebase Architecture (3-phase refactor):**
- `src/cli.rs` (3001 lines) split into `src/cli/` — 11 submodules, max 329 lines each (`dev`, `git`, `health`, `instinct`, `new`, `search`, `security`, `swarm`, `version`, `workspace`).
- `src/mcp_server.rs` (1667 lines) split into `src/mcp/` — 7 submodules: `mod`, `resources`, `tools`, `tools_workspace`, `tools_dev`, `tools_git`, `tools_swarm`.
- `hybrid_search.rs` + `indexer.rs` → `src/search/` module with backwards-compat re-exports.
- `edge.rs` + `evolution.rs` + `instinct.rs` + `router.rs` → `src/intelligence/` module.

**Code Quality:**
- 33 clippy warnings → 0 (`sort_by_key`, `&Path`, `strip_prefix`, `while_let`, dead fields, `flatten`, identical blocks, manual division).
- Risky `unwrap()` calls replaced in production paths (`daemon/server.rs`, `cli/search.rs`, `cli/health.rs`, `cli/new.rs`).

### Changed
- `lib.rs`: 42 top-level modules → logical groupings with backwards-compatible `pub use` aliases.
- All previously scattered `crate::hybrid_search`, `crate::indexer`, `crate::edge`, `crate::evolution`, `crate::instinct`, `crate::router`, `crate::mcp_server` paths continue to work unchanged.

## [1.4.0] - 2026-05-20 (Universal Kernel Edition)

### Added

**Universal Agent Kernel (R-AI-OS 2.0):**
- **Tri-Protocol Interface:** Daemon TCP (:42069), MCP-over-TCP (:42070) ve CLI aynı anda çalışıyor. Claude, Gemini, Codex ve Antigravity aynı event bus'ı paylaşıyor.
- **Lock Manager:** Dosya ve task bazlı kilit yöneticisi — öncelik sistemi (User > Agent > Automation), 30s timeout, re-entrant kilitleme, deadlock önleme. 5 unit test.
- **Radar Whisper Stream:** Compile hataları, security açıkları ve mimari ihlaller bağlı tüm ajanlara gerçek zamanlı `RadarWhisper` eventi olarak push ediliyor.
- **Factory Mode:** Ağır görevler (refactor, test generation, build) arka planda kuyruklanıyor. Anında `job_id` dönüyor, tamamlanınca SQLite inbox + webhook notification. 4 unit test.
- **Universal Proxy-Store:** Yeteneği isteyen ajan ile arka taraftaki backend'i (Rust internal / Python skill / Shell / MCP bridge) soyutlayan proxy katmanı. 5 unit test.

**Storage Overhaul:**
- **Cortex SQLite Store:** Vector embeddings `cortex_store.json`'dan SQLite BLOB'a taşındı. 384×f32 = 1536 byte/chunk little-endian BLOB. Upsert artık transaction-safe (split-brain fix). Legacy JSON otomatik siliniyor.
- **BM25 Index Persistence:** Inverted index SQLite'a persist ediliyor. Restart'ta dosya mtime'larına göre sadece değişen dosyalar yeniden indexleniyor — büyük workspace'lerde soğuk başlangıç eliminasyonu.
- **Session Memory System:** Her agent TCP bağlantısı otomatik session açıyor. Olaylar (`file_change_request`, `handover`, `note`) SQLite'a yazılıyor. Disconnect'te `memory.md`'ye özet satırı ekleniyor.

**MCP Enhancements:**
- `raios://session/current` — aktif session'ı events ile birlikte döndüren MCP resource.
- `raios://session/recent` — son 10 tamamlanmış session.
- `session_note` tool — agent'ın bir kararı veya tamamlanan görevi session memory'ye kaydetmesi için.

### Fixed
- `instinct.rs`: `ProjectHealth` struct'ında eksik `ci_status` / `ci_url` alanları düzeltildi.

### Changed
- `aiosd` daemon artık `Kernel::run()` üzerinden başlıyor — tüm protokoller paylaşılan broadcast channel üzerinde.
- `Server::run()` → `run_inner(tx)` refactoru ile dışarıdan broadcast kanalı alabilir hale getirildi.

---

## [1.3.0] - 2026-05-17 (AI Intelligence Layer)

### Added
- **Official Documentation Suite:** Kapsamlı 7 bölümlük Wiki ve "AI OS Kernel" odaklı yeni README.md.
- **Hybrid Memory Search:** `raios memory --query` ile tüm projelerde semantic + BM25 arama desteği.
- **Sentinel Guard Watch:** `raios security --watch` ile gerçek zamanlı OWASP güvenlik taraması ve Windows bildirimleri.
- **Instinct Automation:** `raios instinct suggest` ile projelere otomatik kodlama kuralları önerisi.
- **CI Status Tracking:** `raios ci` ile GitHub Actions iş akışlarının TUI üzerinden canlı izlenmesi.
- **Aura Hardened IPC:** UUID tabanlı güvenli daemon/client haberleşme protokolü.
- **Diff Inbox Pattern:** Kod değişiklikleri için asenkron onay kuyruğu.

### Fixed
- Cortex Engine'de bellek kullanımı ve indeksleme performansı iyileştirildi.
- TUI üzerindeki gecikme (lag) sorunları SQLite önbelleği ile giderildi.
- Daemon bağlantı kopma ve sonsuz döngü hataları düzeltildi.

## [1.2.0] - 2026-05-14 (AI OS Kernel)

### Added
- **Core Toolkit:** Git, Build, Deps, Env, Version, Process ve Disk yönetimi için 7 yeni modül.
- **AgentShield Guard:** Tehlikeli komut engelleme ve veri sızıntısı koruması.
- **Aggregate MCP Tools:** `project_info` ve `portfolio_status` ile ajanlar için optimize edilmiş veri akışı.
- **Modular App State:** Uygulama durumu namespaced struct'lara (ui, system, projects vb.) bölündü.
- **TUI Enhancements:** Zenginleştirilmiş proje detay paneli ve sağlık raporları.

## [1.1.0] - 2026-05-10

### Added
- **Zero-Setup Wizard:** 8 adımlı otomatik kurulum sihirbazı.
- **OWASP Security Scanner:** Proje bazlı güvenlik skorlaması.
- **Mempalace Scanner:** Derinlemesine proje envanter taraması.

## [1.0.0] - 2026-05-01 (Genesis)

### Added
- **Full CLI Power Tools:** Temel R-AI-OS komut setinin tamamlanması.
- **TUI Improvements:** Stabil ve hızlı Terminal UI arayüzü.

## [0.9.0] - 2026-04-25

### Added
- **GitHub Remote Support:** Uzak depo URL desteği ve workspace optimizasyonu.

## [0.8.0] - 2026-04-15 (Foundations)

### Added
- **Modular TUI:** Bileşen tabanlı arayüz mimarisi.
- **aiosd Daemon:** Arka plan servis altyapısı.
- **Agent Security Proxy:** Ajan etkileşimleri için güvenlik katmanı.

---
*Generated by R-AI-OS Automator*
