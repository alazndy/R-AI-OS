# Product Factory: Evidence-Gated Stage Completion

## Rule

An active Factory stage cannot transition to `completed` until an owner-bound stage evidence link with a non-empty content reference exists.

## Local Commands

```text
/factory stage-evidence <cycle_id> | <stage> | <content_ref>
/factory complete-stage <cycle_id> | <stage>
```

## Release Rule

Release draft creation requires both a completed `verify` stage and evidence-backed passing checks for all required quality profiles.

It also requires that no impact assessment is still awaiting the product owner's decision. Accepted
and rejected assessments remain in the audit trail but are not permanent release blockers. The
readiness payload exposes the pending assessment count so the TUI can state the exact hold.

## Dependency-aware staleness

Stage evidence may be linked to a requirement with:

```text
/factory link-evidence <evidence_id> | <requirement_id>
```

When an accepted requirement change creates a new revision, only evidence explicitly linked to
that requirement becomes `stale`. Stale evidence is counted in release readiness and blocks a new
release draft until current replacement evidence is recorded. Unrelated evidence is unchanged.

## Content-addressed artifacts

The runtime `FactoryArtifactStore` writes accepted artifact bytes outside SQLite under
`artifacts/<first-two-sha256-bytes>/<full-sha256>`. It rejects oversized content and text that
resembles a secret. A canonical `cp_artifacts` row with a digest `content_ref` can then be linked
to its exact stage task-graph node; the Factory evidence link retains only the artifact ID and
digest reference, never the artifact body.

## Safety

Evidence links are references only; no credentials, tester PII, or raw secrets may be placed in their content reference. Empty or whitespace-only evidence references are refused both at the runtime boundary and in the repository layer. Command validation, ownership checks, idempotency, and audit logging remain at the runtime mutation boundary.
