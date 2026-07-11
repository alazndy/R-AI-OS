# Review: Raios Trace Memory Revised Plan

**Reviewing:** `raios-trace-memory-revised-plan.md`
**Reviewer:** Claude (Sonnet 5), grounded in direct inspection of the current `raios` codebase (`/home/alaz/dev/core/R-AI-OS`).

## Overall Verdict

This revision correctly absorbs the prior review's core finding: it stops competing with Cortex, MemPalace, and `evolution.rs`, and narrows scope to `ToolTrace` only. The 3-command CLI surface, the module layout, and the explicit non-goals list are all sound. Two concrete gaps remain, both found by re-checking the revision against the actual code it references.

## Gap 1: The redaction section re-does exactly what it says not to

The plan's "Redaction Requirements" section describes building new masking logic for API keys/tokens/passwords. But `raios_core::security::secret_scan::looks_like_secret()` already exists, is already tested (AWS keys, Anthropic keys, OpenAI-style keys, GitHub tokens, private key blocks, generic key/secret/password/token assignments), and is already in production use in two places: the `raios handoff` CLI and the A2A `message/send` endpoint. Both use **refuse** semantics, not masking — verbatim from `crates/raios-surface-cli/src/cli/handoff.rs`:

```rust
if let Some(label) = raios_core::security::looks_like_secret(&msg) {
    eprintln!(
        "Handoff refused: message looks like it contains a {label}. \
         Remove it and resend — handoffs are stored in plain text (DB + process argv)."
    );
    std::process::exit(1);
}
```

"Mask" implies a different (and harder) design than what's already proven: finding and replacing the exact secret span within a larger string, with real risk of incomplete masking. The established, working pattern is simpler — detect, then reject the whole write. Trace recording should call the same function with the same refuse semantics, not introduce new masking logic.

## Gap 2: Schema placement is left ambiguous

`raios-core/src/db/` currently has two coexisting patterns:
- Central: `cp_tasks`, `cp_agent_runs`, `instinct_candidates`, etc. are all defined in `schema.rs`'s `migrate_existing()`. Other `db/*.rs` files (`wf_handoff.rs`, `wf_file_change.rs`, `wf_swarm.rs`) contain only query/mutation functions against these tables — they never define their own `CREATE TABLE`.
- Self-managed: some `raios-runtime` modules (`evolution.rs::CandidateStore`, `cortex/store.rs`, `search/indexer.rs`) own their own lazy `CREATE TABLE IF NOT EXISTS` inside their own `init()`.

The plan proposes `crates/raios-core/src/db/tool_traces.rs` (the central-pattern location) but never states which convention it follows. Given the file lives in `raios-core/src/db/`, it should follow the `wf_*.rs` convention: the `tool_traces` table goes into `schema.rs`'s central migration, and `tool_traces.rs` holds only CRUD functions — not a self-managed `CREATE TABLE`.

## Minor Finding: Sprint 2's `raios task` integration is weaker than it sounds

`raios task` resolves to `new::cmd_task(description, project, agent)` — a lightweight command that appends a line to `tasks.md`. It is not an execution point. Surfacing "relevant prior traces" there has limited practical value compared to doing it where an agent is actually about to attempt something (`raios run <agent>`) or where context is explicitly carried forward (`raios handoff`, which is already a mature CLI file and already has the `looks_like_secret` integration point proven in it). This directly answers the plan's own Question 5.

## Answers to the Ten Review Questions

**1. Is the scope now correctly reduced?**
Yes, largely — pending the redaction and schema-placement fixes above.

**2. Is `tool_traces` the right core abstraction?**
Yes. The field choices are consistent with existing conventions — `confidence REAL NOT NULL DEFAULT 0.5` matches `evolution.rs::CandidateStore`'s field verbatim, a good sign the plan was actually checked against the codebase this time.

**3. Should this use a trait, or follow current DB module style directly?**
Follow current style, no trait. None of `wf_handoff.rs`/`wf_file_change.rs`/`wf_swarm.rs` use a trait — all are plain `pub fn` + `&Connection`. A single-implementation trait (no mocking/test-double need identified) is unnecessary abstraction in this codebase's established style.

**4. Is the proposed command surface minimal but sufficient?**
Yes.

**5. Is `raios task` integration enough for Sprint 2, or should `handoff` be first?**
`handoff` should be first — see "Minor Finding" above. `raios task` isn't an execution point; `handoff.rs` is mature and already has the exact security-gate pattern (`looks_like_secret`) this feature needs to reuse.

**6. Should trace search stay simple in v1, or hook into Cortex ranking immediately?**
Stay simple in v1. The plan's own principle — "do not start by building a separate retrieval stack" — is internally consistent; no reason to front-load Cortex integration before there's real trace volume to rank.

**7. Are there obvious schema fields missing for future usefulness?**
One: `redacted INTEGER NOT NULL DEFAULT 0`. When a trace write is refused by `looks_like_secret`, there should be an auditable record of *that refusal* (even without the secret content) — otherwise rejected traces vanish silently with no operator visibility into how often redaction is firing.

**8. Is the MemPalace KG bridge the right way to handle preferences?**
Yes — `mempalace_kg_add` already has the subject/predicate/object model; the plan correctly avoids inventing a second preference store.

**9. Is there any remaining hidden duplication risk?**
Yes, exactly Gap 1 above (redaction). Nothing else found.

**10. If this still needs to be smaller, what should be removed next?**
Drop `related_handoff_id` from the Sprint 1 schema. Its correct shape depends on how the Sprint 2 handoff integration actually gets built — adding it now is a speculative FK against an integration that doesn't exist yet.

## Requested Review Output

**Architecture critique.** This revision correctly internalized the prior review's main finding. The remaining risk shifted from "unnecessary reinvention of Cortex/MemPalace/evolution.rs" to a narrower one: reinventing the *security* pattern (redaction) that already exists and is already proven, in the one place the plan didn't cross-check against running code.

**Scope validation.** Confirmed as correctly sized. Only adjustment: defer `related_handoff_id` from Sprint 1 to Sprint 2.

**Schema adjustments.**
- Add `redacted INTEGER NOT NULL DEFAULT 0`.
- Move the `CREATE TABLE tool_traces` statement into `schema.rs`'s central migration, matching `cp_tasks`/`instinct_candidates`; keep `tool_traces.rs` as CRUD-only, matching `wf_handoff.rs`/`wf_file_change.rs`/`wf_swarm.rs`.
- Drop the trait (`ToolTraceStore`) — use plain functions.

**Command UX adjustments.** None needed.

**Sprint corrections.** Sprint 2 should integrate with `raios handoff` before (or instead of, if time-constrained) `raios task` — it's the higher-leverage, already-proven integration point.

**Integration risk notes.** Call `raios_core::security::secret_scan::looks_like_secret` directly with refuse semantics for every trace write (`error_summary`, `fix_summary`, `context`, `command`) — do not write new masking/redaction logic. This is the one place this revision still risks duplicating an existing, tested system.
