# Product Factory Phase 5: Discovery Intake Baseline

## Objective

Every first Product Factory intake must collect the smallest information set that can support a reviewable product Charter without forcing a long, free-form planning session.

## Required Prompts

The stable `builtin:discovery/v1` prompt set is seeded when a local owner starts an intake session:

1. `problem_statement`
2. `target_user`
3. `core_outcome`
4. `first_platform`
5. `success_metric`

Each prompt has a stable key and a human-readable domain prompt. The database stores only the versioned prompt reference; answer text is stored separately on the intake item and may be updated by the owner while the session remains open.

## Resumption and Integrity

Starting intake for a product with an already-open session returns that session and idempotently fills any missing baseline prompts. This protects sessions created before Phase 5 and prevents duplicate prompt rows on retries. Owner checks, Factory configuration gate, input validation, secret screening, idempotency receipts, and audit logging remain in the runtime mutation boundary.

## User Surface

The WORK Product Factory panel identifies the guided sequence. The local command palette continues to submit answers with the documented `/factory answer <session_id> | <question_key> | <response>` action; no public write transport is added.

## Verification

- Core repository coverage proves five prompt rows are created once and retained on session resumption.
- Core domain coverage proves keys are unique and required.
- Runtime authorization and secret-screening tests remain green.
