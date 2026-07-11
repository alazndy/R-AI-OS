# Raios Memory Engine Plan

## Purpose

This document is a handoff and architecture review brief for designing a Rust-native memory layer inside `raios`, inspired by the useful capabilities of MemOS, but not implemented as a direct dependency or full product clone.

The goal is to help Claude review the scope, architecture, and delivery plan before implementation begins.

## Problem Statement

`raios` currently acts as an orchestration and control-plane tool for project operations, task routing, health checks, security workflows, and agent coordination.

We want to add a native memory subsystem to `raios` that provides the most valuable MemOS-like capabilities without inheriting the operational cost and architectural sprawl of a Python/TypeScript service stack.

The user intent is not:

- to embed MemOS as-is
- to depend on Python, TypeScript, Neo4j, or Qdrant by default
- to recreate the entire MemOS platform

The user intent is:

- to rebuild the useful memory capabilities in Rust
- to make them native to `raios`
- to keep them local-first, auditable, deletable, and operationally lightweight

## Product Direction

Recommended direction:

`MemOS-inspired, raios-native operational memory engine`

This means:

- keep `raios` as the authority for orchestration and control-plane state
- add a separate memory layer for recall, learning, summarization, and preference retention
- design the memory layer as an assistive subsystem, not as the source of truth for task state

## What To Rebuild From MemOS

The proposal is to selectively rebuild these capabilities:

1. Semantic recall
2. Tool trace memory
3. Cross-agent shared memory
4. Preference and profile memory
5. Selective knowledge/document memory
6. Skill and instinct candidate extraction

These are the high-value ideas worth carrying over.

## What Not To Rebuild

These should stay out of the first system, and possibly out of scope entirely:

1. Full MemOS platform behavior
2. Cloud-first product surface
3. Full multimodal memory in the first release
4. Heavy distributed scheduling
5. Graph database dependency as a requirement
6. Control-plane authority transfer
7. Automatic memory persistence for every message by default

## Boundary Definition

This boundary is critical.

`raios` should remain responsible for:

- task orchestration
- approvals
- agent handoffs
- build/test/security execution
- deterministic auditability
- control-plane truth

The new memory engine should be responsible for:

- semantic recall
- retrieval of relevant prior solutions
- tool outcome retention
- preference extraction
- session summarization
- document chunk storage and retrieval
- candidate skill extraction

Short version:

- `raios` = authority
- memory engine = recall and learning

## Recommended Architecture

Use a 3-layer architecture.

### 1. Core Domain

Pure Rust domain types, business rules, query logic, ranking interfaces, retention policy interfaces.

This layer should know nothing about SQLite, filesystems, HTTP, or embeddings vendor details.

### 2. Infrastructure Layer

Concrete implementation details:

- SQLite storage
- FTS5 search
- optional embedding provider
- document import and chunking
- retention execution
- audit log storage

### 3. Raios Integration Layer

Command integration and orchestration hooks:

- `raios memory`
- `raios search`
- `raios handoff`
- `raios instinct`
- `raios task`

## First Release Scope

The first release should be deliberately narrow.

### Include

- text-only memory
- SQLite storage
- FTS5 search
- metadata filters
- optional embeddings interface, disabled by default
- tool trace ingestion
- session summary memory
- explicit write policy
- deduplication
- retention and deletion
- audit log
- export capability

### Exclude

- image/audio/video memory
- graph DB
- distributed scheduler
- cloud sync
- automatic always-on write behavior
- autonomous skill execution

## Domain Model Proposal

Suggested core entities:

- `MemoryRecord`
- `MemoryKind`
- `MemoryScope`
- `MemorySource`
- `MemoryQuery`
- `MemoryHit`
- `MemoryFeedback`
- `ToolTrace`
- `PreferenceFact`
- `SkillCandidate`
- `KnowledgeDocument`
- `DocumentChunk`
- `RetentionRule`

### MemoryKind

Suggested variants:

- `Fact`
- `Preference`
- `ToolTrace`
- `Decision`
- `SessionSummary`
- `ProjectNote`
- `SkillCandidate`
- `DocumentChunk`

### MemoryScope

Suggested variants:

- `User`
- `Agent`
- `Project`
- `Workspace`
- `Task`

### MemoryRecord

Suggested fields:

- `id`
- `kind`
- `scope`
- `scope_id`
- `content`
- `summary`
- `tags`
- `source`
- `confidence`
- `created_at`
- `updated_at`
- `expires_at`
- `visibility`
- `provenance`
- `embedding_ref`

### ToolTrace

Suggested fields:

- command
- environment
- project
- agent
- outcome
- stderr_summary
- fix_applied
- success
- elapsed_ms
- created_at

### PreferenceFact

Suggested fields:

- subject
- predicate
- value
- confidence
- derived_from
- last_confirmed_at

### SkillCandidate

Suggested fields:

- title
- trigger_pattern
- steps
- evidence_ids
- confidence
- approved

## Trait Design Proposal

The design should begin with stable interfaces.

Suggested traits:

- `MemoryStore`
- `EmbeddingProvider`
- `MemoryExtractor`
- `PreferenceEngine`
- `SkillEngine`
- `RetentionEngine`

Illustrative shape:

```rust
pub trait MemoryStore {
    fn put(&self, record: MemoryRecord) -> Result<MemoryId>;
    fn get(&self, id: MemoryId) -> Result<Option<MemoryRecord>>;
    fn delete(&self, id: MemoryId) -> Result<()>;
    fn query(&self, query: MemoryQuery) -> Result<Vec<MemoryHit>>;
    fn list_by_scope(&self, scope: MemoryScope, scope_id: &str) -> Result<Vec<MemoryRecord>>;
}

pub trait EmbeddingProvider {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub trait MemoryExtractor {
    fn extract(&self, input: ExtractionInput) -> Result<Vec<MemoryCandidate>>;
}

pub trait PreferenceEngine {
    fn derive_preferences(&self, records: &[MemoryRecord]) -> Result<Vec<PreferenceFact>>;
}

pub trait SkillEngine {
    fn derive_skill_candidates(&self, traces: &[ToolTrace]) -> Result<Vec<SkillCandidate>>;
}

pub trait RetentionEngine {
    fn apply(&self, now: DateTime<Utc>) -> Result<RetentionReport>;
}
```

Review question:

Should these be synchronous for simplicity, or fully async from day one?

## Storage Proposal

Start with SQLite.

Reasons:

- local-first
- simple deployment
- easy backup/export
- good fit for CLI workflows
- good enough for MVP
- avoids heavy infra

Suggested tables:

- `memory_records`
- `memory_tags`
- `memory_links`
- `tool_traces`
- `preference_facts`
- `skill_candidates`
- `knowledge_documents`
- `document_chunks`
- `memory_feedback`
- `memory_audit_log`

## Search and Retrieval Plan

The retrieval layer should not rely on one signal only.

### Retrieval v1

Use:

- scope narrowing
- metadata filters
- FTS5 keyword search
- recency boost
- confidence boost
- tag overlap

### Retrieval v2

Add:

- embedding similarity
- candidate merge
- fused rerank

Suggested pipeline:

1. narrow by scope
2. apply structured filters
3. run FTS search
4. optionally run vector similarity
5. merge candidates
6. rerank
7. return `top_k`

## Write Policy

This is one of the most important design points.

The engine should not store everything.

### Write

- successful tool outcomes
- failed tool outcomes with useful resolution context
- session summaries
- approved project decisions
- confirmed preferences
- imported document chunks
- recurring fix patterns

### Do Not Write By Default

- raw secrets
- full `.env` contents
- raw tokens
- high-risk personal data
- giant noisy logs
- unfiltered raw conversations

## Write Pipeline

Suggested flow:

1. input arrives
2. redaction runs
3. content is classified
4. dedup check runs
5. quality threshold check runs
6. content is persisted
7. optional derived preference or skill candidate is created

## Deduplication and Quality Control

Without this, the memory layer will become a junk drawer.

Suggested controls:

- exact hash-based duplicate prevention
- similarity-based merge for repetitive tool traces
- confidence thresholds
- `pending` vs `active` states

Suggested lifecycle states:

- `pending`
- `active`
- `archived`
- `expired`
- `deleted`

## Preference Memory

This is high-value for `raios`.

Examples:

- user prefers security-first review
- user prefers Turkish in chat and English in code/docs
- user prefers search before editing
- user avoids destructive git operations
- project prefers local-first deployment

These should not remain as raw text. They should become normalized facts.

Example structure:

```text
subject=user
predicate=preferred_review_style
value=security_first
confidence=0.92
```

## Tool Trace Memory

This is likely the biggest practical win for `raios`.

The engine should remember:

- which command ran
- in which project
- for which failure mode
- what fix resolved it
- how often it has worked
- whether it is reusable

This enables queries like:

- "How did we fix this Android build failure before?"
- "What is the usual onboarding pattern for this repo type?"
- "What command sequence previously worked for this situation?"

## Skill and Instinct Candidate Extraction

Do not auto-create executable skills in the first release.

Instead:

- detect recurring high-value trace clusters
- extract shared steps
- generate candidate workflow summaries
- require human review before promotion

Examples:

- Rust CLI repo onboarding
- Android Gradle compile triage
- Python venv recovery after dependency drift
- pre-commit docs sync and audit workflow

This should likely integrate later with `raios instinct`.

## Knowledge and Document Memory

Rebuild a narrow, safe subset.

### Include

- local file ingest
- Markdown, text, JSON, YAML support
- document chunking
- chunk-level retrieval
- project-scoped document search
- document delete/export

### Exclude Initially

- broad automatic web ingest
- automatic indexing of unknown directories
- unsafe default import of private data

Goal:

Project memory should eventually combine:

- `memory.md`
- decisions
- tool traces
- handoff summaries
- imported project docs

## Legal and Safety Constraints

This should be designed in from day one.

The main legal risk is not open-source licensing. The main legal risk is data handling.

### Important context

- MemOS repository is licensed under Apache 2.0
- that license is generally compatible with commercial use
- the real compliance concern is retention, privacy, data minimization, deletion, and user awareness

### Recommended safeguards

- local-first default
- explicit opt-in writes
- secret redaction
- PII tagging
- retention rules
- delete by id/scope/project/user
- export capability
- audit log
- visible memory controls

### Required commands

- `raios memory status`
- `raios memory forget <id>`
- `raios memory purge --scope <scope>`
- `raios memory export --scope <scope>`
- `raios memory policy show`

### Recommended defaults

- store summaries, not raw transcripts
- do not store secrets
- permanent storage only for allowlisted memory kinds

## Raios Command Integration

Suggested new commands:

- `raios memory search "<query>"`
- `raios memory add`
- `raios memory forget`
- `raios memory explain <id>`
- `raios memory policy`
- `raios memory reindex`
- `raios memory import-doc <path>`
- `raios memory traces`
- `raios memory prefs`
- `raios memory skills`

Suggested existing command integrations:

- `raios search`
  - include memory-backed results
- `raios task`
  - fetch relevant prior solutions and preferences
- `raios handoff`
  - generate structured summary candidates
- `raios instinct`
  - accept approved skill candidates
- `raios health`
  - include memory DB health and storage checks

## Suggested Rust Module Layout

Option A: multi-crate layout

```text
crates/raios-memory-domain
crates/raios-memory-store-sqlite
crates/raios-memory-fts
crates/raios-memory-embed
crates/raios-memory-extract
crates/raios-memory-policy
crates/raios-memory-kb
crates/raios-memory-service
crates/raios-memory-cli
```

Option B: single-repo modular layout

```text
src/memory/domain/
src/memory/store/
src/memory/search/
src/memory/extract/
src/memory/policy/
src/memory/kb/
src/memory/service/
src/commands/memory/
```

## Suggested Technical Stack

- Rust
- SQLite
- FTS5
- `serde`
- `uuid`
- `sqlx` or `rusqlite`
- `reqwest` for optional embedding backends
- local file parsers for Markdown/text/JSON/YAML

Question for review:

Is `sqlx + SQLite` the right starting point, or is `rusqlite` simpler and more appropriate for a CLI-first MVP?

## Delivery Roadmap

### Phase 0: RFC and Design Freeze

Duration:

2-4 days

Deliverables:

- architectural RFC
- schema draft
- CLI contract
- retention and privacy policy draft

### Phase 1: Minimal Memory Core

Duration:

1 week

Deliverables:

- SQLite schema
- CRUD operations
- FTS5 query support
- scope/tag/TTL support
- audit log
- `raios memory add/search/forget`

### Phase 2: Tool Trace and Session Memory

Duration:

1 week

Deliverables:

- tool trace ingestion
- session summary records
- dedup logic
- handoff summary enrichment
- better ranking

### Phase 3: Preference Engine

Duration:

4-6 days

Deliverables:

- rule-based preference extraction
- confidence model
- manual confirm/deny flow

### Phase 4: KB and Document Memory

Duration:

1 week

Deliverables:

- file ingest
- chunking
- project-scoped retrieval
- delete/export

### Phase 5: Embeddings and Hybrid Search

Duration:

1 week

Deliverables:

- embedding provider abstraction
- optional vector scoring
- merged ranking

### Phase 6: Skill Candidate Extraction

Duration:

1-2 weeks

Deliverables:

- recurring trace detection
- skill candidate generation
- human approval flow
- `raios instinct` integration

## MVP Success Criteria

The MVP is successful if:

1. An agent can retrieve a relevant prior solution in one query
2. Repeated operational failures do not have to be solved from scratch every time
3. Project and user preferences can be remembered in a structured way
4. Stored memory can be deleted and exported safely
5. The system remains local, lightweight, and operable without embeddings

## Main Risks

Key risks:

- overengineering too early
- noisy low-value memory accumulation
- poor recall quality
- tool trace spam
- premature embeddings complexity
- weak boundary between memory plane and control plane
- unsafe data retention

Mitigations:

- narrow MVP
- explicit write policy
- dedup
- confidence gating
- local-first default
- human approval for skill promotion

## Questions For Claude Review

Please review the plan and answer these specifically:

1. Are the domain boundaries correct?
2. Is the architecture too broad for a real MVP?
3. What should be removed from v1 immediately?
4. Is SQLite + FTS5 enough for the first release?
5. Should embeddings be delayed further?
6. Should skill candidate extraction be postponed until after the first useful release?
7. Should preference extraction begin rule-based, or through structured summarization?
8. Is the trait design clean enough, or should it be simplified further?
9. What is the minimal command surface that still creates real value?
10. If this must fit into 2 practical sprints, how should the scope be reduced?

## Requested Review Output

The requested output from Claude is:

- architecture critique
- scope reduction suggestions
- boundary corrections
- refined module structure
- MVP command set
- risk notes
- implementation priorities

