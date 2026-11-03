# Daemon test coverage

Status: **proposed** (2026-11-03). Nothing here is implemented yet.

## Problem

Line coverage is about 39% overall. The daemon workers sit near 0%: `daemon/{scheduler,lifecycle,sentinel,git,health,cortex}.rs`, `kernel.rs` and `workers.rs`. These are the parts that run unattended, so a regression is found late.

What exists today (`#[test]` counts): scheduler 5, lifecycle 6, health 4, cortex 5, sentinel 2, kernel 15, git 1, workers 0. These test **pure helpers** (`next_lifecycle_status`, the overdue threshold, `job_due_at`). What no test reaches is the loop itself: `start_lifecycle_worker`, `start_scheduler_worker` and the like.

Why the loops are unreachable (read from `lifecycle.rs` and `scheduler.rs`):

1. The body is inside an infinite `loop`, with the work inlined, so a test can only run it by never returning.
2. A fixed `sleep(Duration::from_secs(90))` before the first pass (lifecycle).
3. The database comes from `raios_core::db::open_db()` (process-wide env), not from a parameter.
4. Side effects (`git log`, spawned jobs) go straight to `std::process::Command` / `tokio::spawn`.

## Approach

Make each worker a thin loop around a function that does **one pass**, and test the pass. Do not build a socket harness first: the loops have no socket. Only `daemon/server.rs` and `proxy.rs` do, and `proxy.rs` already has 20 tests.

### Slice 1: one pass per worker (no behaviour change)

- [ ] `lifecycle`: extract `run_lifecycle_pass(&state, &tx, &conn, now, standby, archive) -> bool`. The worker becomes `sleep; loop { open_db; pass; sleep }`. The 90 s settle delay becomes a parameter (production passes 90 s).
- [ ] `scheduler`: extract `run_due_jobs_pass(&conn, now) -> Vec<JoinHandle>` (or a list of fired job ids).
- [ ] Same for `health`, `sentinel`, `git`, `cortex` once each has been read; split only where a loop body is more than a few lines.
- [ ] Tests use a temp-dir database through the existing `DB_ENV_LOCK` pattern and `tokio::time::pause()`, so no test sleeps for real.

Exit: coverage of those files measured before and after with the same `cargo llvm-cov` command CI uses; numbers go in the PR.

### Slice 2: the unattended-failure cases

Each is a test that fails if the guard is removed (mutation-checked, as with the wrapper work):

- [ ] lifecycle never touches `production`, `early`, `legacy`, `waiting`, and skips projects without `.git`.
- [ ] a DB open failure sleeps and continues instead of ending the worker.
- [ ] scheduler fires a due job once, advances `next_run_at`, and records a failure without stopping the loop.
- [ ] an overdue job produces the overdue event exactly once.
- [ ] retention pruning removes only events older than 30 days.

### Slice 3: process boundaries

- [ ] Put `git` and job execution behind a small trait (or function pointer) so a pass can run against a fake that returns a canned commit time or exit status. Keep the real implementation as the default.
- [ ] One real-git test per file in a temp repo, to keep the fake honest.

### Slice 4: server wiring (only if slices 1 to 3 leave gaps)

- [ ] A test that starts `daemon/server.rs` on an ephemeral port or temp Unix socket, sends one request per handler family and checks the reply shape. This is the only place that needs a real socket.

## Out of scope

- Changing worker intervals, retention or thresholds.
- A coverage gate in CI (decide after slice 1 shows the new number).
- The native workspace and the TUI.

## Risks

- Extracting passes touches production loops. Each slice is a refactor commit (tests pass unchanged) followed by a test commit.
- Tests that open the DB share process-wide env; they must take `DB_ENV_LOCK`, or they will flake under parallel runs.
- `tokio::time::pause()` needs the `test-util` feature; check it is a dev-dependency before slice 1.

Each slice ends with fmt, clippy `-D warnings`, the full test run and its own PR.
