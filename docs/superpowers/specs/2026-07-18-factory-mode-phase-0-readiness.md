# Product Factory Phase 0 — Readiness Boundary

## Status

Phase 0 is active. This document fixes the safety boundary before any Product
Factory business logic, cache migration, external integration, or release work
is allowed.

## Current Storage Classification

| Class | Canonical target | Rule |
| --- | --- | --- |
| Durable control state | `workspace.db` | Product, requirement, approval, release, and audit metadata remain transactional and recoverable. |
| Rebuildable search cache | planned `search.db` | BM25, trigram, and vector indexes may be rebuilt from source; they must not become the only copy of product state. |
| Content-addressed artifacts | planned artifact root | Large evidence, reports, builds, and store assets are referenced by digest rather than duplicated in SQLite. |
| Recovery snapshots | planned snapshot root | Backup metadata is retained independently of cache rebuilds. |

`FactoryConfig` remains disabled by default. Configuring a target path does not
create the path, open a database, move data, or enable Product Factory services.

## Migration Safety Contract

No command in this phase may delete, vacuum, rebuild, move, or detach the
existing `workspace.db`. A future cache split is permitted only when all of the
following are true:

1. the control owner explicitly approves the exact migration window;
2. a consistent copy of `workspace.db` exists outside the live database path;
3. `PRAGMA integrity_check` passes on the copy and on the live source before the migration;
4. the destination cache is reconstructible from a documented source set;
5. the old database remains untouched until post-migration verification succeeds;
6. rollback is a path switch to the untouched source database, never an in-place reconstruction.

## Ownership and Execution Boundary

Every future Factory mutation is bound to a `cp_workspace.owner_subject`. The
schema stores ownership now; it does not yet grant remote mutation authority.
Transport-derived authentication, membership checks, and object authorization
remain prerequisites for self-hosted multi-user mutation.

The existing `raios-runtime::factory::Factory` remains a compatibility queue.
`raios-runtime::job_executor::JobExecutor` is its new internal-facing name.
Product Factory does not inherit arbitrary shell execution from that queue.

## Phase 0 Exit Gates

- Product Factory remains disabled unless a control owner explicitly enables it.
- No destructive operation has run against `workspace.db`.
- New lifecycle state is represented by typed domain values and central,
  idempotent schema declarations.
- Product Factory tables have migration coverage in an in-memory SQLite test.
- Existing queue behavior has not been modified.

## Next Boundary

The current skeleton may be reviewed for type names, schema shape, ownership,
and contract boundaries. Planner logic, execution, external research, source
ingestion, artifact writes, app-store actions, and cache migration require a
separate approval after that review.
