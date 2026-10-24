# RAIOS Workspace Native — Terminal Orchestration Design Spec

Date: 2026-10-24
Status: draft for owner review
Builds on: `2026-10-05-raios-workspace-native-design.md` (sections 4, 8 and the Plan 7/8 delivery)

## 1. Intent

Owner request: terminals can be arranged by dragging, can be given names, work fully with Claude Code, Codex and OpenCode, and take part in a task system in which tasks are handed from one terminal to another.

Owner decisions so far:

1. Tasks live in the existing daemon task records (`cp_tasks`), so `raios task`, `raios task-update`, the TUI, the VS Code extension and the workspace show the same data.
2. This document is written and reviewed before any code.

## 2. What exists today

| Capability | State | Where |
|---|---|---|
| Session kinds Shell, Claude, Codex, Opencode | done | `rwk-core/src/sessions.rs` |
| Agent launch `raios run <agent> --project <root> [prompt]`, resume `raios resume <agent:native-id>` | done | `rwk-core/src/terminal/command.rs` |
| `raios run --name <NAME> --work <TEXT> --handoff <MODE>` | CLI only; the workspace passes none of them | `raios run --help` |
| Up to 12 terminals, tabs, session restore, resume offers | done | Plan 7 Phase E, Plan 8 Phase A |
| Layout | three fixed presets (`Ctrl+1/2/3`) and one divider; the dock is a flat list | `rwk-core/src/workbench/layout.rs` |
| Handoff composer, receipts, receiver start | done, per project and per agent kind | `handoff_flow.rs`, `handoff_state.rs` |
| Daemon `Command::CreateTask { title, project_path, priority }`, `UpdateTaskStatus { task_id, status }` | exist in the contract; `ControlPlane` has no method for them | `raios-contracts/src/command.rs` |
| Inspector task list | read-only, `TaskRow { id, title, status, assignee }` | `rwk-core/src/inspector.rs` |

Not present: terminal names, drag and drop of any kind, task create/assign/status in the workspace, a link between a task and a terminal, a handoff addressed to a terminal rather than to an agent kind.

## 3. Non-goals

Daemon-hosted PTYs, a new task store, task dependency graphs, multi-project task boards, cross-machine terminals, and driving an agent's internal state (the workspace never parses agent output to decide what an agent is doing).

## 4. Terminal identity and names

A session gains a `name: Option<String>`, at most 120 characters (the limit `raios run --name` already enforces), no control characters, no credential-looking text (the handoff input validator is reused). Without a name the tab shows the kind and an ordinal ("claude 2").

- **Rename:** double-click the tab or palette "Terminali yeniden adlandır" (also `F2` when the dock has focus, but never while the terminal itself has focus, so the key still reaches the shell). Enter commits, Esc cancels.
- **At launch:** an agent started with a name runs `raios run <agent> --project <root> --name <name> [prompt]`, so the daemon's run record carries the same name and the inspector shows it.
- **After launch:** the daemon cannot rename a running wrapper. A rename of a running agent is a local label; the inspector keeps showing the launch name. The UI says so in the rename dialog instead of hiding the difference.
- **Persistence:** the name is part of the saved workspace terminal entry and of a resume offer, so a restored terminal keeps it.
- **Uniqueness:** names are not required to be unique; handoff and drag use the internal `SessionId`.

## 5. Dock layout and drag and drop

### 5.1 Model

The flat terminal list becomes a pure layout tree in `rwk-core` (no GPUI), generic over how a pane is identified (live `SessionId`s while running, positions in the saved terminal list on disk):

```
DockLayout<T> { root: Option<Node<T>> }
Node = Tabs  { sessions: Vec<T>, active: usize }
     | Split { axis: Horizontal | Vertical, children: Vec<{ weight: f32, node: Node }> }   // at least two children, weights sum to 1
```

A split is n-ary rather than binary, so twelve terminals stacked in a column are one split of twelve, not eleven nested pairs, and the depth limit stays meaningful (an earlier draft of this document used binary splits with a ratio; that could not hold twelve stacked panes within the depth limit).

Invariants, checked by `validate()` that every operation ends with: each pane appears exactly once; no empty group; a split has at least two children and its weights are positive and sum to one; a split never sits directly inside a split of the same axis (it is merged); active tab in range; depth at most 4; at most 12 panes. Every operation works on a copy, normalizes it and validates it before replacing the layout, so a refused operation changes nothing.

Operations: `push_default`, `remove`, `retain`, `activate`, `move_tab(pane, into_group_of, index)`, `split(pane, target, edge)`, `resize(path, boundary, delta)`, `map`, `filter_map`, `sanitized`. The three presets keep working: they set the pane proportions of files, editor and dock, and the dock tree is independent of them.

### 5.2 Gestures

- Drag a tab within its strip to reorder.
- Drag a tab onto another strip to move it there.
- Drag a tab onto the edge of a terminal pane (the edge zone highlights) to split in that direction.
- Drag to empty dock space to create a new group; dropping outside the window does nothing.
- Keyboard equivalent for every drag (palette: "Terminali sağa/sola/yukarı/aşağı taşı", "Terminali böl"), so arranging never requires the mouse.
- A drag never closes, restarts or resizes the PTY's process; only its pane is re-parented. Terminal size is re-sent after the drop settles (one resize per drop, not per mouse move).

### 5.3 Persistence

`DockLayout<usize>` is stored in the per-project saved workspace file next to the existing tab list (`dock`, each pane a position in the saved `terminals` list). A file without it loads with the default arrangement (every terminal in its own group, stacked equally), and a tree that points past the terminals, repeats a pane or nests too deeply is repaired or dropped, never turned into an error that loses the terminals. On restore the arrangement covers the shells that come back; agent terminals rejoin the dock at the end, one by one, as their resume offers are taken.

### 5.4 Risk

GPUI's drag APIs (`on_drag`, `on_drop`, drag-over styling) are used in a pinned `gpui-pre =0.3.8` and their behaviour on the headless test platform is not yet established. Step one of the drag phase is a spike that proves a tab drag in `rwk-ui` headless tests; if the test platform cannot drive it, the model operations are still fully covered by core tests and the gesture layer gets a display test behind `RWK_UI_DISPLAY_TESTS=1`, like the existing window tests.

## 6. Agent compatibility: Claude Code, Codex, OpenCode

"Fully compatible" is defined as this matrix, each cell verified, not assumed:

| Capability | Check |
|---|---|
| Launch with and without a prompt and a name | `raios run` argv built correctly; process starts in the project root |
| Terminal behaviour: colours, cursor shapes, alt screen, bracketed paste, mouse reporting, resize, IME | per-agent manual checklist on the reference machine; engine behaviour already covered by `terminal_engine.rs` |
| Resume | `raios resume <agent:native-id>` for each agent; the recovery catalog lists each agent's conversations |
| Handoff as sender | the agent identity used in `CreateHandoff` is the one the daemon expects (`claude_kaira` in the contract, `claude-kaira` in the CLI): one mapping function, tested for all three agents |
| Handoff as receiver | `raios run <agent> --handoff auto` consumes a pending handoff and the receipt moves to Consumed |
| Clean stop | stop gives the wrapper up to 5 s to finalise its run, for each agent |

Automated tests use a fake agent executable that mimics `raios run` behaviour (prints, reads stdin, exits with codes, honours `--name` and `--handoff`) so no subscribed model is ever run in tests (existing rule). The real-binary checklist is run by hand and its result is recorded in the phase report. Where an agent needs a flag the others do not, it is added to the per-agent launch description in `rwk-core`, not scattered through the UI.

Open fact to verify in the compatibility phase: whether `raios run opencode` is complete on the CLI side (its `--handoff` and resume paths), since the workspace only shells out to it.

## 7. Task system

### 7.1 Data

Tasks are `cp_tasks` rows. The workspace needs the contract to grow in two small, daemon-owned ways:

| Addition | Why | Layer |
|---|---|---|
| `ControlPlane::create_task`, `update_task_status` | wrappers over the existing `CreateTask` and `UpdateTaskStatus` commands | `rwk-core` only |
| `Command::AssignTask { task_id, assignee, expected_revision, idempotency_key }` | there is no assign command today; `expected_revision` follows the approvals pattern, so two windows cannot silently overwrite each other | `raios-contracts`, `raios-core`, `raios-runtime` |
| `TaskRow` gains `revision` and `priority` | needed for conflict-safe edits and ordering | `raios-contracts` DTO, `rwk-core` |

Everything that mutates goes through the existing policy and consent gate (`Guarded`), with new policy keys `create_task`, `update_task_status`, `assign_task`. Reads keep using `GetWorkSnapshot` and the stream, filtered by exact `project_path`.

### 7.2 Task panel

A dock panel (tab beside the inspector) lists the open project's tasks, grouped by status (pending, in progress, completed, cancelled). Actions: new task (title, priority), change status, assign. All are palette commands as well. Completed and cancelled tasks are collapsed by default.

### 7.3 Binding a task to a terminal

A terminal may carry a `task_id: Option<String>`. Setting it is an explicit act: drag a task row onto a terminal, or the palette "Görevi terminale ata". It does three things, in order, and stops at the first failure: `AssignTask` (assignee = that terminal's agent identity), `UpdateTaskStatus -> in_progress`, and, for an agent terminal whose process is waiting for input, nothing else. **The workspace does not type the task into the agent by itself.** Delivering the task text to the agent is the handoff in section 8, a separate, visible step.

The binding is saved with the terminal. When the terminal's process exits, the task is not changed automatically; the task row shows "terminal kapandı" and the owner decides (reopen, complete, reassign). Status changes made by the agent through `raios task-update` appear through the stream and win over the UI's last known value.

## 8. Terminal-to-terminal handoff

Handoffs in the daemon are agent-identity to agent-identity (`from_agent`, `to_agent`), scoped to a project. The workspace adds the terminal as a UI concept on top without changing that:

1. The user starts a handoff from a terminal (its tab menu, `Ctrl+Shift+H`) or by dragging a task onto another terminal and choosing "Devret".
2. The composer opens prefilled: sender = that terminal's agent, receiver = the target terminal's agent, summary seeded with the task title, the sender's terminal name and a marker line the user can edit. Existing validation applies (size, option-looking text, control characters, credential-looking text).
3. A same-kind handoff (Claude to Claude) is refused by the daemon contract (`from` must differ from `to`). The composer states that and offers a note to the task instead.
4. After the policy and consent gate, `CreateHandoff` queues it and the existing receipt state machine tracks it.
5. If a task is attached, it moves to the receiver (`AssignTask`) only after the receipt is `Delivered` or `Consumed`, so a handoff that never arrives does not strand the task on an agent that never saw it.

### 8.1 How the receiver gets it

The daemon delivers a handoff when the receiving wrapper starts (`raios run <agent> --handoff auto`). A terminal that is already running an agent session does not pick one up mid-conversation, and `raios steer` only reaches daemon-spawned sessions, not the PTYs this application owns. Two ways, both explicit:

- **Default, "Yeni terminalde başlat":** the existing receiver start. A new terminal of the receiver's kind opens and consumes the handoff on startup. Reliable, auditable, uses only existing surfaces.
- **Optional, "Mevcut terminale yapıştır":** the handoff text is pasted into the target terminal's PTY through the existing multiline paste preview, which waits for Enter. It works for any agent that reads a prompt, but the workspace cannot tell whether the agent is mid-turn, and pasted text is untrusted input to that agent. It is therefore never automatic, always shows the exact text, and is not recorded as a delivered handoff (no receipt exists for it).

Recommendation: ship the default first; decide on the paste path after using it (open question 2).

## 9. Delivery phases

Each phase ends with a typed review of the core changes before UI work, as in the existing plans. Plan numbers continue from Plan 8.

- **Plan 9 Phase A, names:** `name` on sessions, rename gesture, `--name` at launch, persistence, fake-agent argv tests.
- **Phase B, agent matrix:** the fake agent, the identity mapping, per-agent launch descriptions, the manual checklist for the three real agents. Done before drag so the matrix is not hidden by UI churn.
- **Phase C, dock layout model:** `DockLayout`, operations, invariants, persistence with fallback; core tests only, no UI change visible yet beyond the flat list rendering from the tree.
- **Phase D, drag and drop:** the spike first, then reorder, move, split, keyboard equivalents.
- **Phase E, tasks (done: assignee is the short agent name; `updated_at` is the optimistic-concurrency token; task list lives in the inspector):** `ControlPlane` task methods, the contract additions, the task panel, assign by drag, policy keys, conflict handling.
- **Phase F, handoff between terminals (done: composer seed, task rides the handoff and moves on delivery, new-terminal start; paste path and task↔terminal persistence deferred, see open question 2):** composer prefill, task move after delivery, "Yeni terminalde başlat" wiring, optional paste.
- **Phase G, gates:** extend `ui-bench` with a 12-terminal drag scenario (no frame above budget, no PTY resize storm), run the three-agent checklist, update the README and `SIGMAP.md`.

## 10. Testing

- `rwk-core`: `DockLayout` operations and invariants (including property-style tests: any sequence of operations keeps the invariants), saved-file fallback, name validation, identity mapping, task and handoff flows against the fake `ControlPlane`, assign conflict (`CONFLICT` on a stale revision).
- Daemon side (Phase E): `AssignTask` policy, audit entry, idempotency, revision conflict, with the existing runtime test harness and an isolated `RAIOS_DB_PATH`.
- `rwk-ui`: headless workbench tests for rename, keyboard moves and the task panel; drag tests as far as the test platform allows (section 5.4); real-window tests behind `RWK_UI_DISPLAY_TESTS=1`.
- Security: a task title, terminal name or handoff summary containing option-looking text, control characters or credential-looking text is refused or redacted before it reaches `argv`, the daemon or the audit log. Terminal content never enters the activity log (unchanged rule).
- No subscribed-model runs and no live handoff consumption in automated tests.

## 11. Risks

1. **GPUI drag support** on the pinned version (section 5.4); mitigated by the model/gesture split and the keyboard equivalents.
2. **Layout tree complexity:** nested splits with twelve live PTYs can cause a resize storm; one resize per settled drop and a depth limit of 4.
3. **Contract growth:** `AssignTask` is a new daemon mutation; it follows the approvals pattern (policy key, audit, `expected_revision`) and ships with its own daemon tests before any UI uses it.
4. **Agent drift:** Claude Code, Codex and OpenCode change their CLIs often. The matrix is a checklist run per release, not a one-time claim, and the fake agent keeps CI independent of them.
5. **Handoff expectations:** a user may expect the running agent in the target terminal to receive the handoff immediately. Section 8.1 states what actually happens; the UI labels both buttons with their real behaviour.

## 12. Open questions for the owner

1. Is "assign" to an agent identity (claude / codex / opencode) enough, or must a task be assigned to one specific terminal even when two terminals run the same agent?
2. Should "Mevcut terminale yapıştır" exist at all, given that pasted handoff text is untrusted input to a running agent and leaves no receipt?
3. Is the 12-terminal ceiling still right for split layouts, or should splits be limited to fewer visible panes with the rest in tabs?
4. Should renaming a running agent terminal also try to update the daemon's run record, which would need a new daemon command, or is the local-label behaviour in section 4 acceptable?
