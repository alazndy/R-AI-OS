# Product Factory Phase 2 — Read-Only Overview

## Objective

Expose the current Product Factory state inside the existing `WORK` route
without introducing a new mutation path, top-level route, or external request.

## Projection Path

```text
cp_factory_* canonical tables
        ↓ read-only SQL
raios_core::db::load_factory_overview
        ↓ typed mapping
raios_contracts::FactoryOverviewSnapshot
        ↓ additive serde-default field
WorkSnapshot.factory
        ↓ existing daemon snapshot transport
WORK route Product Factory panel
```

## Visible Data

- Factory enabled/disabled state from local configuration;
- total chartered products;
- active, planned, or blocked cycles;
- proposed, assessing, or approval-waiting change requests;
- new, triaged, or in-progress support items;
- latest product title and lifecycle status.

All values are projections. The view does not create products, start cycles,
change a requirement, grant an approval, run a task, or contact an integration.

## Compatibility and Authorization

`WorkSnapshot.factory` is default-valued when an older daemon does not emit the
field, so a newer TUI can deserialize an older snapshot. The projection reads
from the local control-plane connection only. It does not expose a remote
mutation endpoint or treat any client-supplied subject as an owner.

## UI Boundary

The panel lives at the top of the existing `WORK` right column. It deliberately
does not add a fifth primary TUI route. The disabled state remains visible so
the user can distinguish "no products yet" from "Factory capability disabled".

## Verification

- core repository test creates canonical in-memory lifecycle rows and verifies
  the summary;
- runtime control-plane test verifies `WorkSnapshot` mapping;
- TUI golden route test verifies the Product Factory panel renders;
- contracts, core, runtime, and TUI targeted test and clippy checks pass.

## Next Boundary

Phase 3 may introduce guided intake and draft Charter creation only after a
separate approval. It must use the typed Factory commands, owner binding,
idempotency, and explicit approval semantics; this read-only projection does
not authorize those mutations.
