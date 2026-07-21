# Product Factory Phase 6: Charter Readiness Gate

## Objective

A Charter is a product decision artifact, not a blank note. The Factory must prevent a draft Charter from skipping the minimum discovery evidence collected in Phase 5.

## Server-Side Gate

Before handling `CreateCharterDraft`, the runtime reads the product's current open intake session and calculates required `builtin:discovery/v1` prompts that do not have a non-empty `answered` response.

If any prompt is missing, the command fails with `INVALID_INPUT` and returns only the stable missing keys. No Charter revision, idempotency receipt, or audit decision is written for the rejected request.

If all five answers are present, the existing owner-bound Charter mutation proceeds in its transaction and remains payload-idempotent and audit-logged.

## Ownership and Safety

The gate is applied after trusted configuration and authenticated actor checks. It does not trust TUI state. Existing input bounds and secret screening run before any mutable work; the low-level repository remains parameterized SQLite only.

## User Surface

The WORK panel now identifies the Charter completion rule. The TUI may continue submitting answers through the local command palette; a server error reports the exact next intake key rather than silently generating an incomplete document.

## Verification

- Core tests verify missing required keys before and after a saved answer.
- Runtime tests verify an incomplete Charter request is rejected, then succeeds after the remaining required answers are recorded.
- TUI render coverage verifies the gate guidance remains visible.
