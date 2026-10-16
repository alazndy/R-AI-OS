# workspace.db power-cut durability — read-only report (Plan 8 Phase 0)

Date: 2026-10-16 · Author: OpenCode Kaira · Scope: read-only investigation.
The live `~/.config/raios/workspace.db` was **never opened, copied or written** by this
investigation. Evidence comes from the source tree, file metadata, daemon logs, and throwaway
databases in `/tmp` that mirror the production open path.

## Verdict

The open path is already power-cut safe. Every connection runs in WAL with `synchronous=FULL`
(explicit in `open_db()`, FULL by default for the other openers), the effective busy timeout is
the rusqlite default of 5 s, and SIGKILL probes reproduced exactly the WAL semantics we want:
committed transactions survive, in-flight transactions vanish whole, `integrity_check` stays
`ok`. No corruption window and no committed-data-loss window was found. The real gaps are
operational, not correctness: backups are manual-only and the newest snapshot is five days old,
and the live WAL is 426 MiB. **No code change was made** — the handoff allows a one-line pragma
fix only for a proven defect, and none exists here.

## 1. How raios opens the database

Main path — `crates/raios-core/src/db/mod.rs:85-96`:

```rust
let conn = Connection::open(&db_path)?;
conn.execute_batch(
    "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
)?;
```

- Path: `~/.config/raios/workspace.db`, `RAIOS_DB_PATH` override (`mod.rs:99-107`).
- The daemon, CLI and MCP call this per operation with **fresh, short-lived connections** — there
  is no long-lived writer connection to go stale (`daemon/handlers.rs:19`,
  `daemon/scheduler.rs:63`, `daemon/lifecycle.rs:49`, …).

Every other opener of the same file, and what it sets:

| Opener | Location | Pragmas set |
|---|---|---|
| Cortex store | `crates/raios-runtime/src/cortex/store.rs:19-36` | WAL (+ schema) |
| Session store | `crates/raios-runtime/src/session.rs:45-50` | WAL, foreign_keys |
| Factory | `crates/raios-runtime/src/factory.rs:109-111` | WAL, foreign_keys |
| Evolution store | `crates/raios-runtime/src/intelligence/evolution.rs:35-39` | none (inherits) |
| Task-graph store | `crates/raios-core/src/task_graph/store.rs:37-38` | WAL, foreign_keys |
| BM25 index (aiosd startup, `raios search`, MCP) | `crates/raios-runtime/src/search/indexer.rs:375-399` | WAL, foreign_keys (+ schema) |
| Trigram index | `crates/raios-runtime/src/search/trigram.rs:230-242` | WAL, foreign_keys (+ schema) |
| Maintenance (check / backup / checkpoint) | `crates/raios-core/src/db/maintenance.rs:217-252` | read-only, or RW + 5 s busy |

Notes:

- `journal_mode=WAL` is a **persistent** property of the file, so openers that don't repeat it
  still run in WAL.
- `synchronous` is **per connection**. Only `open_db()` sets it explicitly; everyone else inherits
  SQLite's default, which is FULL (`2`). `git grep "PRAGMA synchronous"` has exactly one hit.
  The bundled `libsqlite3-sys` 0.28.0 has no compile-time default override.
- The child processes of `raios run` never open the DB themselves; the wrapper persists on their
  behalf (`crates/raios-runtime/src/agent_runner.rs:167-171`).

## 2. What survives a power cut (throwaway probes)

Method: `/tmp/opencode/p8-db-durability/throwaway.db`, opened with the same pragma batch, then
the process is SIGKILLed at a chosen point and the file is reopened (script in the appendix).

| Simulated crash point | Result on next open |
|---|---|
| Mid-transaction (INSERT, no COMMIT, SIGKILL) | `integrity_check: ok`; uncommitted row gone; earlier committed rows intact |
| Immediately after COMMIT, before close/checkpoint (SIGKILL) | `integrity_check: ok`; committed row present (WAL replay) |

Semantics behind this: with WAL + FULL, each commit fsyncs the WAL before returning, so a
committed transaction survives a power cut; an in-flight transaction is discarded whole. WAL
recovery is automatic on the next open, and a crash during a checkpoint is restartable by
design. Schema migrations are idempotent and re-run on every open
(`crates/raios-core/src/db/schema.rs:3-4`), so a crash mid-migration heals on the next open
instead of wedging the DB.

Checkpointing: SQLite's automatic WAL checkpoint (default threshold) plus the manual
`raios db checkpoint [--truncate]` (`maintenance.rs:217-247`, passive or truncate mode, 5 s busy
timeout). Nothing calls it periodically today — see observations.

## 3. busy_timeout and writer contention

- rusqlite 0.31.0 sets `sqlite3_busy_timeout(db, 5000)` on **every** open
  (`rusqlite-0.31.0/src/inner_connection.rs:121` in the local registry source). So `open_db()`
  connections wait up to 5 s for a writer lock instead of failing immediately. Verified
  empirically through the project's own build (probe in the appendix): effective
  `busy_timeout=5000ms`, and a second writer racing a held write lock waited 330 ms and then
  succeeded.
- Explicit overrides exist where a different budget is wanted: 5 s in maintenance
  (`maintenance.rs:160,227`), contract recovery (`contract_recovery.rs:32`), audit writes
  (`verify_chain.rs:7,23`), wrapper liveness (`wrapper_liveness.rs:181`), BM25/cache
  (`bm25_storage.rs:58`, `cache_maintenance.rs:54`); 250 ms in session recovery
  (`session_recovery.rs:283`).
- The existing test suite already pins the wait-don't-fail behaviour for concurrent writers
  (`activity_events.rs`, `security_finding_sync_waits_for_an_existing_writer`).
- Doc nit: `factory.rs:155-162` claims the factory's connections run with "no `busy_timeout`
  set" and would "get an immediate `SQLITE_BUSY`". That is inaccurate under rusqlite 0.31 (5 s
  default). The in-process `write_lock` it justifies remains a harmless belt-and-braces.
- Real contention does occur on this machine: the current `aiosd.log` (48,115 lines) contains 5
  `database is locked` lines, all from the Scheduler claim / wrapper-recovery path; the two most
  recent sit 263 lines from the end of the log. The scheduler retries on the next tick, so the
  impact is delayed claims, not lost work.

## 4. Backups — online, integrity-checked, manual only

- Command: `raios db backup [--keep N]`, default retention 3, max 10
  (`crates/raios-surface-cli/src/cli/args.rs:97-101`) →
  `backup_workspace_database` (`maintenance.rs:69-76`).
- Mechanics (`maintenance.rs:127-215`): SQLite **online backup API** against a read-only source
  connection (512-page steps, 5 ms pauses), refuses a source that fails `integrity_check`,
  re-checks the snapshot after copying, removes empty WAL/SHM companions, fsyncs the snapshot
  file and then the directory **before** pruning older snapshots, writes a `.sha256` sidecar,
  keeps files private (0600) in `~/.config/raios/backups/`. A failure mid-way removes the
  partial files and leaves existing snapshots untouched.
- **There is no automatic schedule.** The only callers are the CLI and tests; there is no
  systemd timer for it (the only raios timer is `raios-anka-index.timer`), no crontab entry,
  and no daemon job. The three daemon scheduler jobs that exist ("JSON Backup",
  "Night pipeline", "Morning delivery") are agent jobs and currently fail to spawn anyway.
- Live state: 5 snapshots, ~12 GiB total, newest written 2026-10-11 01:05 (+03) — five days
  stale at the time of writing.

## 5. Live-state observations (file metadata only; DB not opened)

- `workspace.db` 1.72 GiB, `-wal` 426 MiB, `-shm` 864 KiB; `aiosd` (up 14 h) holds 7 open
  handles on the DB.
- The WAL was static across a 4 s sample but is large; a passive checkpoint is either not being
  attempted or cannot advance past some reader, or a big maintenance batch left it. Not a
  corruption risk — recovery simply replays it — but worth hygiene: a periodic passive
  checkpoint, and a look at what pins the checkpoint horizon.
- Disk: 88 GB free on `/` (83 % used); snapshots alone are ~12 GiB.

## 6. Recommendations (no code change made in this PR)

- **R1 — schedule backups.** Run `raios db backup` on a systemd user timer (daily is plenty) or
  from daemon housekeeping. Today the only recovery points are manual and stale.
- **R2 — WAL hygiene.** Add a periodic passive checkpoint (`raios db checkpoint` logic) and
  investigate what keeps the live WAL at 426 MiB.
- **R3 — contention.** If scheduler `database is locked` recurrences matter, raise the busy
  timeout for the scheduler's claim path (one line at the call site). Not needed for durability.
- **R4 — docs.** Correct the `factory.rs` busy-timeout comment. For Plan 8's desk store
  (Phase C), reuse this repo's proven pattern from the backup module: fsync the file, then the
  directory, before replacing the previous recovery point.

## Appendix — probes (throwaway, not committed)

`crash_probe.py` (engine-level SIGKILL simulation, run against `/tmp` only):

```python
import os, signal, sqlite3, sys
PATH = "/tmp/opencode/p8-db-durability/throwaway.db"

def open_like_raios():
    con = sqlite3.connect(PATH)
    con.execute("PRAGMA journal_mode=WAL")
    con.execute("PRAGMA synchronous=FULL")
    con.execute("PRAGMA foreign_keys=ON")
    return con

def seed():
    for s in ("", "-wal", "-shm"):
        try: os.remove(PATH + s)
        except FileNotFoundError: pass
    con = open_like_raios()
    con.execute("CREATE TABLE t(a INTEGER, b TEXT)")
    con.execute("INSERT INTO t VALUES (1, 'committed-before-crash')")
    con.commit(); con.close()

def crash_mid_txn():
    con = sqlite3.connect(PATH)
    con.execute("BEGIN IMMEDIATE")
    con.execute("INSERT INTO t VALUES (2, 'uncommitted-should-vanish')")
    os.kill(os.getpid(), signal.SIGKILL)      # power cut, no commit

def crash_after_commit():
    con = sqlite3.connect(PATH)
    con.execute("BEGIN IMMEDIATE")
    con.execute("INSERT INTO t VALUES (3, 'committed-should-survive')")
    con.commit()
    os.kill(os.getpid(), signal.SIGKILL)      # killed before close/checkpoint

def verify():
    con = sqlite3.connect(PATH)
    print("integrity:", con.execute("PRAGMA integrity_check").fetchall())
    print("rows:", con.execute("SELECT a, b FROM t ORDER BY a").fetchall())

if __name__ == "__main__":
    {"seed": seed, "crash-mid-txn": crash_mid_txn,
     "crash-after-commit": crash_after_commit, "verify": verify}[sys.argv[1]]()
```

Raw results:

```
===SEED===                  seeded row 1; journal_mode: wal
===CRASH MID-TXN===         killed exit=137;  -wal 0 bytes
===VERIFY===                integrity: [('ok',)]; rows: [(1, 'committed-before-crash')]
===CRASH AFTER COMMIT===    killed exit=137;  -wal 4152 bytes
===VERIFY===                integrity: [('ok',)]; rows: [(1, 'committed-before-crash'),
                                                        (3, 'committed-should-survive')]
```

rusqlite probe (scratch integration test, run once and deleted; asserts against a throwaway DB):

```rust
let conn = Connection::open(&db)?;
conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
// -> busy_timeout=5000ms  synchronous=2  journal_mode=wal  foreign_keys=1
// second writer racing a held BEGIN IMMEDIATE: waited 330.4 ms, then Ok(1)
```

Raw results:

```
PROBE pragmas: busy_timeout=5000ms synchronous=2 journal_mode=wal foreign_keys=1
PROBE lock-wait: elapsed=330.406575ms result=Ok(1)
test result: ok. 2 passed; 0 failed
```
