# Product Factory Phase 9: Impact Approval and Cycle Materialization

## Objective

Introduce a safe, local-only path from a requested product change to an approved requirement revision, then turn an explicitly approved plan into a canonical lifecycle cycle. This phase does not execute work, schedule agents, publish a release, or add public Factory transport.

## Change Control

The local Factory boundary supports this ordered flow:

1. Submit an owner-bound change request.
2. Assess its deterministic affected set: current product requirements and Charter revisions are recorded as stale targets.
3. Require explicit owner acceptance or rejection of the assessment.
4. Only an accepted assessment may create the next immutable revision of an affected requirement.

All mutations use parameterized SQL, bounded secret-screened input, payload-bound idempotency, and one control-plane transaction that also writes an audit decision.

## Plan to Cycle Boundary

`CreatePlanDraft` creates an owner-bound `planned` plan. `ApprovePlan` is a separate action. Only an approved plan can be materialized into a Factory cycle.

Materialization is idempotent by plan: it creates at most one `cp_factory_cycles` row and exactly these pending stage runs:

```text
discover → define → design → build → verify → release → support
```

The stage rows are planning records only. No stage executor, agent launch, external integration, quality pass, or release action is triggered.

## Local TUI Commands

```text
/factory plan <product_id> | <title>
/factory approve-plan <plan_id>
/factory cycle <plan_id>
```

## Verification

- Core tests prove accepted changes preserve the old requirement revision, create revision 2, and retain stale impact targets.
- Core tests prove an unapproved plan cannot materialize, approved plans create all seven stages, and repeated materialization returns the existing cycle.
- Contracts, runtime Factory, and TUI parser test groups pass.

## Release Readiness Extension

A release draft additionally requires a completed `verify` stage for the product and a passing, evidence-backed result for every required quality profile. An agent assertion, a plan approval, or a release-channel request cannot replace either evidence condition.
