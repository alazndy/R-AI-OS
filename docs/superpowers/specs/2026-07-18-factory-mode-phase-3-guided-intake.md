# Product Factory Phase 3: Guided Intake and Draft Charter

## Scope

Phase 3 implements only the local mutation boundary required to discover a product and save its first Charter draft. It does not expose a public HTTP or CLI write route, enable the Factory feature flag, run a planner, invoke agents, access third-party systems, or submit releases.

## Commands

`FactoryCommand` supports the following payload-idempotent operations:

- Create an owner-bound workspace.
- Create a draft product inside an owned workspace.
- Start or resume one open intake session for an owned product.
- Upsert an answer keyed by a bounded intake question key.
- Append an immutable Charter revision for an owned product.

All commands validate bounded non-empty input, reject probable secrets before durable persistence, derive the actor only from the authenticated transport boundary, and reject a disabled Factory before opening a transaction.

## Authorization and Integrity

- `ControlActor` remains transport-derived and is never serialized from client input.
- Mutations require both the trusted Factory configuration gate and `may_mutate_control_plane`.
- Workspace, product, intake session, and Charter writes verify the stored owner subject on the server-side query.
- The existing `cp_idempotency` table stores the exact command payload hash and cached result. A reused key with a different payload is rejected.
- Each completed mutation writes a hash-chained `product_factory` audit decision in the same transaction.

## Durable Content Boundary

Short intake answers and Charter draft text are stored in newly additive, bounded text columns. Existing rows receive safe empty defaults. This is temporary durable control state, not an artifact-store substitute: Phase 4+ must move large or generated documents to the content-addressed artifact store through an explicit migration.

## Verification

- Core repository test proves owner checks, intake answer persistence, and immutable Charter revision creation.
- Runtime service test proves end-to-end local creation, idempotent replay, disabled-gate rejection, and remote-session rejection.
- Contract, core, and runtime lint/test commands are run before promotion.
