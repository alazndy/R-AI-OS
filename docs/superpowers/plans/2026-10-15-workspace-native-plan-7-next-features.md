# Workspace native, Plan 7: live run stream, project search, tabs and session restore, in-app approvals

Status: **plan for review**; no code is written before approval (skeleton-first, RIPER-5).
Owner asked (2026-10-15) for all four. Order below is by dependency, not by size.

## 0. Findings from the investigation

| Area | Finding | Consequence |
|---|---|---|
| Live stream | `GET /api/stream` broadcasts whatever the daemon pushes into one channel; only the sentinel and the factory push (loose JSON). The typed `Event` contract (`AgentRunStateChanged`, `ApprovalRequested`, `ApprovalResolved`, `SnapshotUpdated`, …) is **defined but never emitted**; only the TUI reducer consumes it. | The daemon must emit the typed events first; polling cannot be replaced by subscribing today. |
| Task paths | `service.rs` fills `UnifiedTaskDto.project_path` from `t.project_name` (a name, not an absolute path). All 144 tasks on this machine therefore show `None`/a name, and the inspector (exact path match) lists nothing. | Daemon-side fix (join the project root) is a prerequisite for a useful inspector. |
| WebSocket client | `rwk-core` talks HTTP with `ureq`, which has no WebSocket. A sync client needs one new crate (`tungstenite`, plain `ws://127.0.0.1`, no TLS). | New dependency: supply-chain script and `cargo audit` gate; alternative is long-poll on the existing query. |
| Search | `raios locate <pattern> --dir <root> -j` already gives `{path,line,text}` JSON over a trigram index (~15 ms warm). | Native can shell out to it (no new dependency, same engine as the CLI), with a bounded result count and a timeout. |
| Approvals | `Command::ApproveHandoff`/`RejectHandoff` exist without `expected_revision`. `ApproveHandoff` sets the run to `handover_delivered` and the task to `completed` even when a human (not the target agent) approved, while the receipt (`ResolvedElsewhere`) correctly says "not delivery". | Spec correction C3 is real: approve needs different run/task semantics before a button can exist. Reject is safe to ship first. |
| Tabs/restore | Editor is single-document; `DocumentSession` is per file. Terminals are owned by the app; `raios resume` (merged in #53) can relaunch saved agent conversations by exact ID. | Restore = app-local state file plus `raios resume --dry-run`-style planning; no daemon change. |

## 1. Phases (each ends with tests, clippy, a real-window check and its own PR)

### Phase A, foundation in the daemon (raios-runtime / raios-contracts)
1. Tasks carry the project root: `UnifiedTaskDto.project_path` = the registered project's absolute path (join on `project_id`), name stays in a new `project_name` field (additive, `#[serde(default)]`).
2. Emit typed events from the real transition points: run created / ended (`AgentRunStateChanged`), handoff queued (`ApprovalRequested`), resolved (`ApprovalResolved`). One helper `emit(Event)` over the existing broadcast sender; no behaviour change for current listeners.
3. Tests against the real HTTP/WS handler with a temp DB (no live daemon).

### Phase B, live inspector (rwk-core / rwk-ui)
```rust
// rwk-core/src/control/stream.rs (skeleton)
pub trait EventSource: Send { fn next_event(&mut self, timeout: Duration) -> Result<Option<ControlEvent>>; }  // exists
pub struct WebSocketEvents { /* tungstenite client, reconnect with backoff */ }
pub enum ControlEvent { /* existing */ RunChanged{run_id, agent, status}, ApprovalChanged{approval_id} }
```
- The inspector keeps its 3 s poll as the fallback (daemon down or stream missing) and refreshes immediately on an event.
- Connection banner reflects stream state as well as query state.

### Phase C, in-app approvals
- Contract: `expected_revision` on `ApproveHandoff`/`RejectHandoff` (additive, optional for old callers); the daemon refuses a stale revision (`CONFLICT`).
- Semantics (needs your decision, see §3): a human approval resolves the approval as `approved` with `resolved_by = local_control_owner`, the run exits with `handover_approved_by_owner` (not `handover_delivered`) and the task is **not** completed; only the target's own wrapper marks delivery.
- UI: Approve / Reject buttons per approval in the inspector, both behind the existing consent gate (policy command names `approve_handoff`, `reject_handoff`), reject asks for a reason.
- Reject ships first (no semantic change needed).

### Phase D, project search
```rust
// rwk-core/src/search.rs (skeleton)
pub struct SearchHit { pub path: String /* relative */, pub line: usize, pub text: String }
pub fn locate(root: &Path, pattern: &str, ignore_case: bool, limit: usize) -> Result<SearchResult>;  // runs `raios locate -j`, bounded output, 5 s timeout
```
- Palette command "Projede ara" (`Ctrl+Shift+F`), results list with preview, Enter opens the file at the line (needs `EditorView::open_at(path, line)`).
- Failure modes: `raios` missing, timeout, invalid pattern → message in the panel, never a panic.

### Phase E, tabs and session restore
```rust
// rwk-core/src/workbench/layout_state.rs (skeleton)
pub struct SavedWorkspace { pub project: PathBuf, pub tabs: Vec<SavedTab>, pub active: usize, pub layout: LayoutPreset, pub terminals: Vec<SavedTerminal> }
pub struct SavedTab { pub path: String, pub line: usize }
pub enum SavedTerminal { Shell, Agent { kind: SessionKind, native_session: Option<String> } }
```
- Editor holds several `DocumentSession`s (tab strip, dirty marker per tab, close asks per tab); the existing guards (unsaved edits on switch/close) apply per tab.
- Save on change (debounced) to `$XDG_CONFIG_HOME/raios-workspace-native/workspaces/<project-hash>.json`, owner-only, atomic; restore on project open: tabs and layout immediately, shells restart, agent terminals are *offered* ("Önceki ajan oturumunu sürdür") through `raios resume` planning, never auto-launched.

## 2. Order, size, risks
A (daemon, ~4 commits) → B (client, ~3) → D (search, ~3, independent, can run in parallel with B) → C (contract + UI, ~5) → E (largest UI change, ~6).
Risks: new dependency `tungstenite` (verify owner and checksum with `verify_gpui_supply_chain.py`-style provenance); changing run/task semantics for approvals touches the wrapper and TUI (they read `handover_delivered`); the daemon must be reinstalled to test A/B/C live (same procedure as before, with backup).

## 3. Decisions needed
1. **Events transport**: add `tungstenite` for a real WebSocket subscription (recommended), or stay with faster polling only?
2. **Approval semantics (C3)**: human approval ends the handoff as `handover_approved_by_owner` and leaves the task open (recommended), or keep today's "completed + delivered" behaviour and only add the buttons?
3. **Search scope**: shell out to `raios locate` (recommended; requires `raios` on PATH, which the app already assumes for agents) or implement an in-process searcher?
4. **Restore of agent terminals**: offer-only via `raios resume` (recommended) or auto-resume the last session?
