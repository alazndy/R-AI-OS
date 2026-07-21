# Product Factory Phase 7: Evidence-Based Charter Proposal

## Objective

After the discovery gate is complete, the Factory should reduce user effort without inventing product facts. Phase 7 produces a deterministic Charter proposal from only the persisted, validated intake answers.

## Command

`GenerateCharterDraft { product_id, idempotency_key }` is a local, configuration-gated Factory command. The TUI exposes it as:

```text
/factory generate <product_id>
```

The command first applies the same required-intake readiness gate as manual Charter creation. It then reads the owner-bound product title and all required answer values from the current open session.

## Output

The generated Markdown contains five traceable sections: Problem, Target Users, First Release Outcome, Initial Platform, and Success Metric. It is stored as a normal immutable Charter revision with `generated: true` in the command result.

The user can still use the manual Charter command to provide revised text; generation is a proposal, never an authority override.

## Safety

No model, web source, or external integration runs in this phase. Input validation and secret screening have already happened before answer persistence. Ownership, trusted feature configuration, payload-bound idempotency, and hash-chained audit logging are enforced by the existing runtime boundary.

## Verification

- Pure composition test proves source answer evidence appears in the generated document.
- Runtime flow test proves a generated revision follows the manual revision and contains persisted target-user evidence.
- Contract and TUI parser tests cover the new command variant.
