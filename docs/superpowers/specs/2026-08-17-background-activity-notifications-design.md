# Background Activity Notifications — Design Spec

**Date:** 2026-08-17
**Status:** Approved by Göktuğ, ready for implementation planning
**Author:** Claude Kaira

## Context

`aiosd`'s background workers (health, git, lifecycle, scheduler) and agent
sessions (`cp_agent_runs`) already do a lot of work the user never sees unless
they actively run `raios health`/`reflect` or read the TUI. Today's session
(2026-08-17) is a concrete example: a real bug (`aiosd` dead for ~44h after a
silent `Kernel::run()` exit), a 6.3 GB disk cleanup, and a stale-detection fix
across 8 call sites all happened and were only visible because the user asked.

The user wants `raios` to surface this proactively as desktop notifications:
what background work happened, and what it recommends — without needing to
ask. Two tiers, decided during brainstorming:

- **Important** — delivered as soon as the next client poll picks it up
  (effectively near-instant, ~15s, given existing poll cadence).
- **Routine** — batched into a periodic digest (default 30 min, configurable)
  with a short summary and the top outstanding recommendation.

## Scope (explicitly decided)

- **Data model**: a new dedicated `activity_events` table — not a diff over
  existing tables. Decided explicitly over the initially-proposed
  diff-based/cursor-only approach because a real append-only log is easier to
  reason about, query, and extend later (e.g. a future activity-history view)
  than reconstructing "what changed" from snapshots.
- **Producers (v1)**: health worker (dirty-count deltas), git worker (new
  commits), lifecycle worker (status transitions), scheduler worker (fires +
  overdue jobs), agent-run completion (`cp_agent_runs` → completed/failed),
  and `audit_log` CRITICAL-severity rows. The sentinel worker (`cargo check`
  compile monitoring) is a natural future producer but is **out of scope for
  v1** — no existing hook point was reused for it and adding one is a
  separate, unapproved scope increase.
- **Transport**: plain REST polling on the existing HTTP API
  (`127.0.0.1:42071`), reusing each client's existing poll loop. Both current
  consumers (`tools/raios-tray/raios-tray.py`, kaira-launcher's GNOME
  extension) already poll REST on a timer and have zero WebSocket/daemon-TCP
  client code — adding a second transport for this feature alone was
  considered (and rejected) as disproportionate. See "Approaches considered"
  below.
- **Consumers (v1)**: `tools/raios-tray/raios-tray.py` (Qt, cross-desktop) and
  `kaira-launcher`'s GNOME Shell extension (`code/gnome-extension/`). Both
  get the same two endpoints; each renders through its own native
  notification API.
- **Retention**: `activity_events` rows older than 30 days are pruned.
  Piggybacks on the lifecycle worker's existing periodic-cleanup rhythm
  rather than introducing a new prune worker.

### Approaches considered

1. **REST polling (chosen)** — no new transport; both clients already poll
   REST on a timer. "Near-instant" for important events is satisfied by the
   existing ~15s poll interval, which is indistinguishable from push for
   desktop-notification UX.
2. **Daemon TCP broadcast (`kernel.rs`'s existing `broadcast::Sender<String>`
   StateSync channel)** — genuinely real-time, but currently has exactly one
   consumer (the TUI). Wiring two more clients (one Python/Qt, one GJS) into
   a new transport is a materially bigger change than this feature needs.
3. **Hybrid (important via broadcast, routine via poll)** — the "most
   correct" option but requires maintaining two delivery paths for one
   feature. Rejected as disproportionate to the actual latency requirement
   (15s poll is already fast enough that no user will notice the difference
   between that and a push).

## Data Model

```sql
CREATE TABLE activity_events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ts           TEXT NOT NULL DEFAULT (datetime('now','utc')),
    source       TEXT NOT NULL,              -- 'git' | 'lifecycle' | 'scheduler'
                                              -- | 'agent_run' | 'audit'
    project      TEXT,                       -- nullable; NULL = workspace-wide event
    tier         TEXT NOT NULL,              -- 'important' | 'routine'
    summary      TEXT NOT NULL,              -- short human-readable line
    detail_json  TEXT                        -- optional structured extra data
);

CREATE INDEX idx_activity_events_tier_ts ON activity_events(tier, ts);
CREATE INDEX idx_activity_events_tier_id ON activity_events(tier, id);
```

```sql
CREATE TABLE notification_cursors (
    client_id            TEXT PRIMARY KEY,
    last_important_ts    TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
    last_digest_ts       TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
    last_important_id    INTEGER NOT NULL DEFAULT 0,
    last_digest_event_id INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE security_notification_state (
    project     TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    summary     TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now','utc')),
    PRIMARY KEY(project, fingerprint)
);
```

Cursors live server-side, keyed by a client-supplied `client_id`, so a
client's poll cadence or restart never causes duplicate or dropped
notifications — the server always knows exactly what each named client has
already seen. Event IDs are the delivery boundary; timestamps remain response
metadata and the digest interval clock. This prevents same-timestamp rows from
being skipped. `security_notification_state` stores the currently active
High/Critical finding fingerprints so repeated scans remain quiet while a
finding persists, but a resolved finding is notified again if it reappears.

## Producers — trigger rules

| Source | Tier | Trigger | Existing hook point |
|---|---|---|---|
| `git` | routine | a project's daemon git status changes | `daemon/git.rs`'s per-project git-status update |
| `lifecycle` | important | status transition (`active`↔`beklemede`↔`archived`) | `daemon/lifecycle.rs`, right after `update_project_status` succeeds |
| `scheduler` | routine | a cron job fired successfully | `daemon/scheduler.rs`'s fire-success path |
| `scheduler` | important | a job is overdue by >2× its configured interval | `daemon/scheduler.rs`'s claim/backoff path (same code path fixed 2026-08-17 for the JSON Backup retry-storm bug) |
| `agent_run` | routine | a run succeeds with a non-empty completion summary | `raios-core/src/db/wf_sessions.rs`'s `cp_session_end_with_summary`, after it writes `run_status`/`task_status` |
| `audit` | important | a new or reappearing High/Critical security finding is observed | `daemon/health.rs`'s per-project security scan, atomically synchronized with `security_notification_state` |

Each producer writes directly to `activity_events` — no shared "emit event"
abstraction is introduced in v1 beyond a small `raios_core::db::log_activity_event(...)`
helper (keeps the insert SQL in one place; each caller supplies
source/project/tier/summary/detail).

## API

Two read-only endpoints on the existing HTTP API (Axum, port 42071):

```
GET /api/notifications/important?client_id=<id>
  -> { events: [{ ts, project, summary, source }...], cursor_ts }
  Server-side: SELECT ... WHERE tier='important' AND id > cursor.last_important_id
  ORDER BY id. Advances notification_cursors.last_important_id to the maximum
  returned ID and retains the corresponding timestamp in last_important_ts.
  The endpoint read advances the cursor; there is no separate ack call.

GET /api/notifications/digest?client_id=<id>
  -> { since_ts, until_ts, summary: "<one-paragraph text>",
       top_recommendation: "<string, mirrors reflect's #1 recommendation>",
       event_count } | null (if nothing new)
  Server-side: only fires a non-null digest if now - cursor.last_digest_ts >=
  digest_interval_secs (config-driven, see below); otherwise returns null and
  does not advance the cursor. When it fires, it snapshots the maximum event
  ID, groups routine rows in `(last_digest_event_id, snapshot_id]`, then
  advances both the event-ID boundary and interval timestamp atomically.
```

`summary` is built deterministically, no LLM call (the daemon has no model
access) — group the window's routine rows by `source`, render one clause per
non-empty group in a fixed order (git → health → scheduler → agent_run),
e.g. `"3 new commits across 2 projects; dirty-file count changed in 1
project; 4 scheduled jobs ran"`. `event_count == 0` still returns a non-null
digest once the interval elapses, with `summary: "No background activity."`
and just `top_recommendation` populated — a quiet window is itself useful
signal, not something to suppress. `top_recommendation` reuses `raios-surface-cli/src/cli/reflect.rs`'s
existing `build_recommendations` function (already unit-tested per this
repo's 2026-08-11 PR#18 memory.md entry) rather than inventing new
recommendation logic.

Both require the existing session-token auth already used by every other
`/api/*` route — no new auth mechanism.

## Config

New field in the existing `[daemon]` section of `config.toml`:

```toml
[daemon]
digest_interval_secs = 1800   # 30 min default, per brainstorming decision
```

Read via `raios_core::config::Config`, alongside the daemon's existing interval
fields.

## Clients

**`raios-tray.py`** (Qt): its existing `refresh()` (15s `QTimer`) gains a
call to `GET /api/notifications/important`; any returned events are shown via
the already-working `self._notify()`. A second, independent `QTimer` at
`digest_interval_secs` polls the digest endpoint and shows one notification
per non-null response. The tray persists a per-install UUID at
`~/.config/raios/notification-client-id`; if that path cannot be written, it
uses a deterministic UUID fallback rather than sharing a global cursor.

**`kaira-launcher` GNOME extension**: the existing `fetchRaiosJson` helper
(already shared between the systray indicator and the Super+Space overlay,
per the 2026-07-06 work in `memory.md`) is reused to hit the same two
endpoints on the same cadence pattern, rendering through GNOME's
`Main.notify()` (or a `MessageTray.Source`, matching how the weather
indicator's popup already surfaces summary text).

Every client must send a stable ID of 1–128 ASCII characters using only
letters, digits, `-`, `_`, `.`, or `:`. Invalid IDs receive HTTP 400 before
any database access.

Both clients use a stable per-install `client_id` (e.g. derived from
hostname + app name) so server-side cursors don't collide across machines
sharing one `aiosd`.

## Testing

- `raios-core`: unit tests for `log_activity_event` (writes land correctly,
  `tier`/`project` nullability) and cursor advancement logic (TDD, per
  project convention).
- Per-producer: one test per trigger rule confirming the right `tier`/
  `summary` gets written (e.g. lifecycle transition → exactly one important
  row; two consecutive identical dirty-counts → no duplicate routine row).
- API layer: integration tests against a fixture DB confirming cursor
  isolation between two different `client_id`s, and the digest endpoint's
  interval-gating (`null` before interval elapsed, non-null after).
- Client-side (Python/GJS): out of scope for automated testing per existing
  project convention (raios-tray and the GNOME extension have no existing
  test harness); verified manually per the project's established reload
  caveats (documented in `kaira-launcher/memory.md`).

## Non-goals (v1)

- No sentinel-worker (compile-check) producer yet.
- No per-user notification preferences/mute list — all important events go
  to all registered clients.
- No historical activity-feed UI (TUI panel or web view) — `activity_events`
  is queryable via `sqlite3`/`raios mem`-style tooling if needed, but no
  dedicated browsing UI ships in v1.
- No push transport (WebSocket/daemon-TCP) — see "Approaches considered."
