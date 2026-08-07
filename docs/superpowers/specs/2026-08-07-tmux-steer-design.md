# tmux Steer — Design Spec

**Date:** 2026-08-07
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira

## Context

While researching `omnigent-ai/omnigent` (a direct competitor to raios's multi-CLI
wrapper strategy), we found their live "steer" capability: for native-wrapped
harnesses (`claude-native`, `codex-native`), a follow-up message is delivered into
a *currently running* session by pasting it into the session's live terminal pane
(`tmux send-keys` for claude-native; a real `turn/steer` RPC for codex-native,
which Codex exposes natively). Both are documented as live-verified in
`docs/QUEUE_STEER_DESIGN.md` of that project.

raios has no equivalent today. `ExecutionProxy::spawn_agent`
(`crates/raios-runtime/src/daemon/proxy.rs`) is fire-and-forget: it spawns the
agent CLI via `Command::spawn`, captures stdout into `AgentProcess.logs`, and
enforces a timeout ("Death Timer"). `raios handoff`
(`crates/raios-core/src/db/wf_handoff.rs`) only reassigns `cp_tasks.assignee_id`
in the database — delivery happens whenever the target agent is next spawned and
reads its inbox, not while it's running. There is no way today to inject a new
instruction into an agent that is *currently* mid-session.

This spec covers a new, narrow, additive capability: **`raios steer`**, letting a
human or another agent inject a message into a currently-running,
daemon-spawned agent session.

## Scope (explicitly decided)

- **Target sessions**: only agents spawned headlessly via
  `ExecutionProxy::spawn_agent` (daemon-managed, e.g. via cron/handoff-triggered
  launches). The human-interactive `raios run <agent>` path
  (`crates/raios-runtime/src/agent_runner.rs`, inherits the caller's own
  terminal) is explicitly **out of scope** for this iteration.
- **Senders**: both a human (new `raios steer <agent> "<message>"` CLI command)
  and other agents (new MCP tool `steer_agent`). Both route through the same
  internal function.
- **Relationship to `raios handoff`**: purely additive. `raios handoff` and its
  DB-reassignment semantics are untouched — they remain the mechanism for "pick
  this up whenever you're next spawned." `raios steer` is a new, separate,
  best-effort mechanism for "inject this into what's running right now."
- **No busy/idle detection.** We deliberately do not build precise
  turn-boundary detection (no equivalent of Claude Code's
  `UserPromptSubmit`/`Stop` hooks). Framing follows Omnigent's own documented
  honesty on this point: steer is "send now; the agent folds it into current
  work if it can" — a best-effort delivery, not a guaranteed mid-turn
  interrupt. This keeps the initial scope small and avoids promising a
  precision we can't verify across all four wrapped CLIs.

## Architecture

`ExecutionProxy::spawn_agent` launches the agent inside a named tmux session
instead of a bare `Command::spawn`:

```
tmux new-session -d -s raios-agent-<AgentProcess.id> '<agent-cmd>'
```

The session name is deterministic from the existing `AgentProcess.id: Uuid`
(`crates/raios-runtime/src/daemon/proxy.rs`) — no new identifier scheme, no
new field, no migration.

Steering a live session shells out:

```
tmux send-keys -t raios-agent-<id> "<message>" Enter
```

This mirrors Omnigent's live-verified `claude-native` mechanism exactly: the
message lands in the pane as if the user typed it, and the wrapped CLI's own
TUI decides how to fold it into current work.

## Components

| Component | Change |
|---|---|
| `daemon/proxy.rs::spawn_agent` | Launch via `tmux new-session -d` instead of direct `Command::spawn`. |
| Output capture | Immediately after `new-session`: `tmux pipe-pane -o -t <session> 'cat >> <logfile>'`. `AgentProcess.logs` is now populated by tailing that logfile instead of reading the child's stdout handle directly. This is the one existing code path this change touches. |
| `daemon/proxy.rs::steer_agent` (new fn) | `steer_agent(agent_id: Uuid, message: &str, sender: &str) -> Result<SteerOutcome>`. Validates the target against `DaemonState.active_agents`, confirms liveness with `tmux has-session`, then `send-keys`. |
| `raios-surface-cli/src/cli/steer.rs` (new) | `raios steer <agent> "<message>"`, same skeleton as `cli/handoff.rs`. |
| MCP tool `steer_agent` | Agent-to-agent steering, same underlying function as the CLI path. |
| `raios-policy.toml` | New explicit `[[tools.rules]]` entry for `steer_agent` with `action = "confirm"` (the existing `default_action = "confirm"` already covers it; an explicit rule documents the intent, matching the precedent set for `Handover`). |
| Audit | Every steer call writes an `audit_log` row (sender identity, target agent id, message, timestamp, outcome) and calls the existing `push_event` broadcast so a live TUI/dashboard reflects it immediately. This does **not** touch `cp_tasks` or `cp_approvals` — steer stays isolated from the handoff/task graph. |
| `raios doctor` | New check: `tmux` binary present. Surfaced at `raios doctor` time, not as a runtime surprise the first time `spawn_agent` runs. |

## Data Flow

1. `raios steer <agent> "<msg>"` (CLI) or `steer_agent` (MCP) call arrives.
   Both call `ExecutionProxy::steer_agent`.
2. **Policy check first.** Consult `raios-policy.toml`'s resolved action for
   `steer_agent`:
   - `Deny` → reject immediately with the same error shape used by other
     policy-denied calls elsewhere in the codebase.
   - `Confirm` → follow the existing `Handover` precedent exactly: a pending
     row is filed through the existing approval flow (`cp_approvals` +
     `raios sessions`/inbox — the same mechanism already used for handoff
     approvals). The CLI/MCP call returns "queued for approval," not a
     synchronous block. The real `send-keys` fires only once approved.
   - `Allow` → proceed immediately.
3. Look up `agent_id` in `DaemonState.active_agents`. Not found, or status
   isn't "running" → clear error, no tmux call attempted.
4. Defensive liveness check: `tmux has-session -t <session>`. If the session
   is gone (agent crashed or exited without state catching up yet), return a
   clear "target session not found — agent may have finished or crashed"
   error, and opportunistically flip that `AgentProcess` to inactive in
   `DaemonState` so the next lookup doesn't repeat the same dead hit.
5. `tmux send-keys -t <session> "<message>" Enter`.
6. On success: write the `audit_log` row, fire `push_event`, return `"sent"`
   to the caller. We do not return an Omnigent-style `delivered`/`queued`
   distinction, since we don't track busy/idle state — "sent" only confirms
   the keystrokes were injected, not that the agent has acted on them yet.

## Error Handling

| Failure | Behavior |
|---|---|
| `tmux` binary missing | `spawn_agent` fails fast at spawn time with a clear error. No silent fallback to the old `Command::spawn` path — one code path, not two. |
| `tmux has-session` fails | Steer returns a clear "session not found" error; matching `AgentProcess` is marked inactive. |
| Policy `Deny` | Same error shape as existing policy-denied paths. |
| Policy `Confirm` | Routed through the existing approval mechanism (see Data Flow step 2) — not a new bespoke confirmation UI. |

## Testing

- **Unit**: `steer_agent`'s validation branches (agent not found, policy deny)
  — mockable without a real tmux process.
- **Integration**: spawn a harmless real command (e.g.
  `bash -c 'read line; echo "GOT:$line"'`) through the real `spawn_agent`
  tmux path, call `steer_agent` with a known string, read back the
  `pipe-pane` logfile, and assert the string and its echo appear. Exercises
  the real `tmux` binary (already present in this environment).
- **`raios doctor`**: a test for the tmux-missing detection path.

## Explicitly Out of Scope (this spec)

- The interactive `raios run <agent>` path (human's own terminal).
- Busy/idle detection or a `delivered` vs `queued` distinction.
- The two other Omnigent-derived items discussed separately: policy 3-tier
  scoping and bwrap/Landlock sandboxing. Each gets its own spec/plan cycle.

## Open Question for the Implementation Plan

None outstanding — all decisions in this document were confirmed in
conversation before writing. The next step is `superpowers:writing-plans` to
turn this into a step-by-step implementation plan.
