# Review: Raios Memory Engine Plan

**Reviewing:** `raios-memos-rust-plan.md`
**Reviewer:** Claude (Sonnet 5), grounded in direct inspection of the current `raios` codebase (`/home/alaz/dev/core/R-AI-OS`) — not a cold read of the plan alone.

## Headline Finding

The plan does not account for what already exists. Of its own six-item "What To Rebuild From MemOS" list, five already have working implementations in the user's current stack — three inside `raios` itself, one in an already-running external MCP server (MemPalace). Building the plan as written means standing up a fourth, parallel system next to three that already do most of this.

| Plan item | Status | Evidence |
|---|---|---|
| 1. Semantic recall | **Already exists** | `crates/raios-runtime/src/cortex/` (1131 lines) — real embedding-based vector search (`index_workspace`, `search`, `search_with_filter`), live as the `semantic_search` MCP tool. Also MemPalace's `mempalace_search`. |
| 2. Tool trace memory | **Partial — the real gap** | `security/audit.rs` + hash-chained `verify_chain.rs` record allow/deny/confirm decisions, but never outcome or fix context. |
| 3. Cross-agent shared memory | **Already exists** | MemPalace's Wing/Room/Drawer + `tunnel` system is already shared across `claude_kaira`/`codex_kaira`/etc.; `cp_*` control plane already does structured, atomic handoff. |
| 4. Preference/profile memory | **Structurally identical already** | MemPalace's `mempalace_kg_add` (subject→predicate→object, with `valid_from`/`valid_to`) is the same shape as the plan's own `PreferenceFact` example (`subject=user, predicate=preferred_review_style, value=security_first, confidence=0.92`). |
| 5. Knowledge/document memory | **Already exists** | MemPalace drawers (verbatim content) + Cortex's `index_memory_files` (file ingest/chunking). |
| 6. Skill/instinct candidate extraction | **Already exists, nearly 1:1** | `crates/raios-runtime/src/intelligence/evolution.rs::CandidateStore`: `rule, source, confidence (REAL), expires_at, promoted` backed by the `instinct_candidates` SQLite table, with a 7-day auto-expiry and a `promote()` human-approval step — this is most of the plan's Phase 6 already shipped. `intelligence/instinct.rs`'s `suggest_from_health/outcome/failure` plus `raios policy suggest` (learns `[[tools.rules]]` from `audit_log`, pending → human review → active) apply the exact same philosophy again. |

`search/indexer.rs` also already provides BM25-style keyword search — the plan's "Retrieval v1: FTS5" equivalent, just a different implementation of the same idea.

**Conclusion:** the only item without an existing, working counterpart is cross-session, queryable **tool-outcome + fix-context** memory ("how did we fix this Android build failure before?"). That is the one real, non-redundant gap. Everything else in the plan is a re-implementation of Cortex, MemPalace, or `evolution.rs`/`instinct.rs` under new names.

## Answers to the Ten Review Questions

**1. Are the domain boundaries correct?**
Conceptually yes (`raios` = authority, memory = recall), but misdrawn in practice. The real boundary isn't "raios vs. a new memory engine" — it's "raios vs. Cortex + MemPalace + `evolution.rs`," and the plan never looks at that side of the line.

**2. Is the architecture too broad for a real MVP?**
Yes, substantially. 8 crates (Option A), 6 traits, 13 entities, 10 tables — most of it re-implementing systems that already run. Real MVP scope is roughly 15% of what's listed.

**3. What should be removed from v1 immediately?**
- `MemoryStore` / `EmbeddingProvider` traits — Cortex already does this; extend it instead.
- `KnowledgeDocument` / `DocumentChunk` — use Cortex's `index_memory_files`.
- `PreferenceEngine` / `PreferenceFact` — write into MemPalace's KG; the triple model already exists.
- `SkillEngine` / `SkillCandidate` — extend `evolution.rs::CandidateStore`; don't rewrite it.
- Phase 4 (KB and Document Memory) and Phase 5 (Embeddings) in their entirety — both already exist.

**4. Is SQLite + FTS5 enough for the first release?**
Wrong framing — `raios` already uses bundled `rusqlite` with a custom BM25-style indexer instead of FTS5. Consistency argues for extending `search/indexer.rs`'s existing pattern, not introducing a second search engine with a different backend.

**5. Should embeddings be delayed further?**
Also the wrong question — embeddings already ship in Cortex (see `memory.md`: "Cortex Real Embeddings & hf-hub Redirect Fix"). Nothing to delay; there's something to integrate with instead.

**6. Should skill candidate extraction be postponed?**
It's already in production (`evolution.rs`, confidence-scored, TTL-expired, human-promoted). Not to be postponed — to be used.

**7. Should preference extraction start rule-based or via structured summarization?**
MemPalace's KG is already a triple store. No new extraction engine is needed — only a rule for *when* a KG fact gets written.

**8. Is the trait design clean enough, or should it be simplified further?**
Once the real gap is isolated, six traits collapse to one: `ToolTraceStore`. The rest are integration points into existing systems, not new abstractions.

**9. What is the minimal command surface that still creates real value?**
```
raios trace record            # persist a tool outcome + fix
raios trace search "<query>"  # recall a prior fix
raios trace forget <id>
```
Three commands. Everything else the plan proposes (`memory add/search/forget/explain/policy/reindex/import-doc/traces/prefs/skills`) is already covered by MemPalace, Cortex, or `raios policy`/`raios instinct`.

**10. If this must fit into 2 practical sprints, how should scope be reduced?**
- **Sprint 1:** `tool_traces` table + `ToolTraceStore` trait + `raios trace record/search` + secret redaction + dedup.
- **Sprint 2:** integrate into `raios task`/`raios handoff` (surface relevant past traces automatically) + a thin bridge that writes confirmed preferences into MemPalace's KG (a call into the existing `mempalace_kg_add`, not a new engine).

## Requested Review Output

**Architecture critique.** The plan is technically competent but was written without reading the codebase it's meant to extend — it answers the abstract question "what's worth taking from MemOS" rather than "what does `raios` still need after accounting for Cortex, MemPalace, and `evolution.rs`." The result is a parallel-platform plan, not an integration plan.

**Scope reduction.** 8 crates → 1 module (`raios-core/src/db/tool_traces.rs` + a thin CLI layer). 13 entities → 2 (`ToolTrace`, `TraceQuery`). 6 phases → 2 sprints.

**Boundary corrections.** `raios` = authority + Cortex (semantic search) + MemPalace (preference / cross-agent / knowledge memory) + `evolution.rs` (skill candidates). The new memory work is scoped to tool-trace/fix-context memory only; everything else is an integration point into what already exists.

**Refined module structure.**
```
crates/raios-core/src/db/tool_traces.rs     — table + CRUD, following the existing cp_* pattern
crates/raios-surface-cli/src/cli/trace.rs   — raios trace record/search/forget
```
No new crate. No new multi-crate architecture.

**MVP command set.** `raios trace record`, `raios trace search`, `raios trace forget`. That's the full surface needed to deliver real value without duplicating existing systems.

**Risk notes.** The plan already names the right risk — "weak boundary between memory plane and control plane" — but points it in the wrong direction. The actual weak boundary risk is between the *new* system and `raios`'s *own* existing systems: without explicit reuse, this plan produces two unsynchronized knowledge graphs and two unsynchronized preference stores (the new one and MemPalace's).

**Implementation priorities.** 1) `tool_traces` table + recording. 2) Search/recall. 3) `raios task`/`raios handoff` integration. Do not touch the preference, skill, or knowledge-document sections — they're already built.
