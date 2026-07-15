# jcode Mining Roadmap — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Scope note:** this roadmap covers 9 independent subsystems mined from the `jcode` CLI (github.com/1jehuang/jcode) for adoption into raios. Per the writing-plans scope check, each task below is a self-contained subsystem — do not treat this file as one feature. Before an agent starts a given task, re-run `superpowers:writing-plans` scoped to *just that task* to expand it into full TDD steps (failing test → code → passing test → commit) against the actual current state of the files listed, since this roadmap fixes ordering and touch-points, not every line of code.

**Goal:** Port the highest-value architectural mechanisms found in jcode's design docs into raios, ordered cheapest/fastest → hardest, without duplicating things raios already does better (hash-chained audit, TOML policy gate, BM25+vector Cortex hybrid).

**Architecture:** Every task extends an existing raios module in-place (`raios-core/src/db/*`, `raios-runtime/src/intelligence/*`, `raios-runtime/src/system_scan/*`, `raios-runtime/src/cortex/*`) — no new crates, no new top-level daemons. All schema changes go through `raios-core/src/db/schema.rs` migrations, never raw SQL against `~/.config/raios/workspace.db` (see AGENT_CONSTITUTION §8.1 rule 5 — instinct/evolution tables specifically must only be touched via `raios instinct`/`raios evolve`, never manual SQL).

**Tech Stack:** Rust, `rusqlite`, existing `cp_*` / `mem_*` SQLite schema in `raios-core`.

## Verification Findings (2026-07-14, source-grounded against current tree)

The task anchors below were checked against the actual `R-AI-OS` source, and four tasks were corrected from the first draft. Read these before executing — they override any stale assumption:

1. **`raios memory <query>` does NOT search `mem_items`.** It runs `Cortex` over **`memory.md` files** (`crates/raios-surface-cli/src/cli/workspace.rs` → `cortex.index_memory_files` / `search_with_filter` with `MEMORY_PATTERNS`). `mem_items` rows are only reachable via `mem_list`/`mem_get`/`mem_export` (exact, no semantic search) — confirming AGENT_CONSTITUTION §8.1 rule 2. `crates/raios-runtime/src/cortex/store.rs` is **code-chunk** search (`upsert_file`/`chunk_count`), not memory. This reshaped **Task 8** entirely.
2. **`mem_lineage` is already a graph** (`crates/raios-core/src/db/schema.rs`): it has a free-text `relation` column (default `'derived_from'`) linking `item`/`node` ids. So Task 8 extends `mem_lineage` relations, it does not invent a parallel `mem_edges` table.
3. **`instinct_candidates` is the OLD schema** (`project_name, command, outcome, suggestion, status`) — no `type`, no `provenance`, no `confidence` column. Inserts go through `crates/raios-runtime/src/intelligence/evolution.rs` and dedup by `suggestion` text. Per constitution §8.1 rule 5, **never ALTER this table by hand.** This invalidated the "add columns to instinct candidates" step in Task 3 and the `--type` filter in Task 4.
4. **Confirmed-real anchors:** `cp_tasks.parent_task_id` exists; `cp_task_graph_nodes` exists (composite PK `(graph_id, node_id)`, no `node_kind` yet); health entry is `scan_system() -> AiAuditReport` in `system_scan/mod.rs`; handoff CLI is `cli/handoff.rs::cmd_handoff(to, status, msg, project_path, json)` dispatched from `cli/mod.rs:189`; `policy.rs::validate_tool_call(tool_name) -> Result<()>` (uses a `"confirm:"` error-prefix convention); `raios doctor` does **not** exist. `task_graphs` is a COMPAT CACHE — source of truth is `cp_task_graphs`/`cp_task_graph_nodes`.

## Global Constraints

- Single source of truth stays `~/.config/raios/workspace.db` — no new database files, no JSON sidecars for state that should be queryable.
- `mem_items` remains replace-on-write with `mem_lineage` archiving (raios-core/src/db/mem.rs) — additions must extend this pattern, not bypass it.
- Every new CLI surface must exist in both `raios-surface-cli` and, where it represents state an agent should query, `raios-surface-mcp` (MCP tool parity is required for the 4-agent matrix, not optional).
- Nothing here touches `instinct_candidates` schema directly by hand (dual-schema risk per constitution §8.1 rule 5) — go through `raios-runtime/src/intelligence/evolution.rs`'s `CandidateStore`.
- Fail-closed for policy/security paths, fail-open for observational/hook paths — matches the existing Phase 2 Policy Manager convention.

---

### Task 1 (cheapest): Memory & control-plane size budget in `raios health`

**Why first:** pure read-only addition, zero schema risk, catches problems before any of the other 8 tasks add more tables/rows.

**Files:**
- Modify: `crates/raios-runtime/src/system_scan/mod.rs` (entry is `scan_system() -> AiAuditReport` at line 91 — extend the `AiAuditReport` this returns; do NOT invent a new return type)
- Modify: `crates/raios-surface-cli/src/cli/mod.rs` (surface the new check under `raios health`)
- Create: `docs/BUDGET.md` at repo root — documented caps per table, mirroring jcode's `MEMORY_BUDGET.md` ratchet-checklist approach

**Steps:**
- [ ] Add a `db_budget_check()` function that runs `SELECT COUNT(*)` per table and `PRAGMA page_count * PRAGMA page_size` for total DB size against `mem_items`, `cp_tasks`, `cp_agent_runs`, `cp_artifacts`, and the audit ledger table, comparing against hardcoded soft caps (start with: mem_items ≤ 5,000 rows/project, workspace.db total ≤ 500MB — adjust once real numbers are known)
- [ ] Fold the result into the `AiAuditReport` returned by `scan_system()` (add a `db_budget` field/section to the report struct rather than a separate return path)
- [ ] Write `docs/BUDGET.md`: one line per table — current soft cap, what happens when exceeded (warn vs. block), and a one-paragraph PR-review checklist item ("does this change grow a hot table without a corresponding budget bump?")
- [ ] Run `cargo test -p raios-runtime system_scan` and confirm no regression
- [ ] Commit: `feat(health): add DB size/row budget check to raios health`

---

### Task 2: Structured handoff artifact (replace free-text `--msg`)

**Files:**
- Modify: `crates/raios-core/src/db/wf_handoff.rs` (`create_handoff_workflow`, currently takes `msg: &str` — signature: `from_agent, to_agent, status, msg, diff_stat`; it already builds a `metadata_json` via `serde_json::json!({...})`)
- Modify: `crates/raios-surface-cli/src/cli/handoff.rs` (`cmd_handoff(to, status, msg, project_path, json)` — dispatched from `cli/mod.rs:189`; add the `--report` arg here and in `cli/args.rs`)
- Modify: `crates/raios-surface-tui/src/ui/panels/inbox.rs` (render the structured fields instead of raw string)

**Steps:**
- [ ] Define a `HandoffReport` struct in `raios-core` (co-locate with `wf_handoff.rs`): `findings: String, evidence: Vec<String>, edge_cases_considered: Vec<String>, open_questions: Vec<String>, confidence: f32, what_i_did_not_check: Vec<String>`
- [ ] Accept `--msg` as before (back-compat: wrap a bare string into `HandoffReport { findings: msg, ..Default::default() }`) *and* accept `--report <path-to-json>` for the structured form — do not force a breaking CLI change
- [ ] Serialize `HandoffReport` into the existing `metadata_json` blob in `create_handoff_workflow` (it already builds a `serde_json::json!({...})` — add the fields there) instead of adding a new column
- [ ] Update `inbox.rs` rendering to pretty-print the structured fields when present, falling back to raw string display for legacy free-text handoffs
- [ ] Test: round-trip a `HandoffReport` through `create_handoff_workflow` → read back via whatever query `inbox.rs` uses → assert fields match
- [ ] Commit: `feat(handoff): structured HandoffReport replaces free-text --msg`

---

### Task 3: Confidence decay + provenance on mem_items / instinct candidates

**Scope correction (per Verification Finding #3):** this task touches **`mem_items` ONLY.** The `instinct_candidates` table is the old dual-risk schema — do NOT add provenance/confidence columns to it (constitution §8.1 rule 5 forbids hand-ALTERing it). Instinct provenance, if ever wanted, must go through a `raios evolve` schema migration in a separate future task, not here.

**Files:**
- Modify: `crates/raios-core/src/db/schema.rs` (add `provenance TEXT`, `confidence REAL`, `last_used_at TEXT` columns to `mem_items`)
- Modify: `crates/raios-core/src/db/mem.rs` (`MemItemRow` at line 5, `MemUpsert` at line 18, `mem_upsert` at line 41 — all currently lack these fields)

**Steps:**
- [ ] Add `Provenance` enum (`UserStated`, `Observed`, `Inferred`, `Corrected`) to `raios-core`, `#[derive(Serialize, Deserialize)]`
- [ ] Migration in `schema.rs`: `ALTER TABLE mem_items ADD COLUMN provenance TEXT DEFAULT 'observed'`, `ADD COLUMN confidence REAL DEFAULT 1.0`, `ADD COLUMN last_used_at TEXT` — guard with the codebase's existing idempotent-migration pattern (columns already default-populate for existing rows)
- [ ] Extend `MemItemRow` and `MemUpsert` structs + `mem_upsert` INSERT to carry the three new fields (default provenance `Observed`, confidence `1.0`)
- [ ] Add `on_used(conn, mem_id)` in `mem.rs`: bumps `confidence` toward 1.0 and sets `last_used_at = now` — called wherever a mem_item is surfaced (note: today that is `mem_get`/`mem_list` call sites, since there is no semantic mem search path — see Finding #1)
- [ ] Add lazy decay at read time (half-life per `item_type`, e.g. `feedback` = 90 days, `project` = 30 days): `effective_confidence = confidence * 0.5^(days_since_last_used/half_life)` computed in a `MemItemRow` helper — no new background job, no stored decayed value
- [ ] Test: insert a mem_item, backdate `last_used_at` in the test DB, assert `effective_confidence()` decays as expected; assert existing rows migrate with default provenance/confidence
- [ ] Commit: `feat(mem): add provenance + lazy confidence decay to mem_items`

---

### Task 4: Approval history → policy-rule promotion

**Scope correction (per Verification Finding #3):** `instinct_candidates` has NO `type`/`kind` column — inserts use `(project_name, command, outcome, suggestion, status)` and dedup by `suggestion` text (`evolution.rs:62-66`). `raios instinct list` has no `--type` filter (`InstinctCmd` only has `List`). So a policy-promotion suggestion is stored as a normal candidate row with a **recognizable `suggestion`-text prefix** (e.g. `"[policy] allow tool <name>"`), and surfaced by the same `raios instinct list` — filtering is by prefix convention, not a schema column. Do NOT ALTER the table.

**Files:**
- Modify: `crates/raios-runtime/src/intelligence/evolution.rs` (`CandidateStore` — reuse the existing `INSERT INTO instinct_candidates (project_name, command, outcome, suggestion, status)` + dedup-by-suggestion pattern at lines 62-66; add an approval-decision signal source alongside the current `JobComplete`/`JobFailed` broadcast subscription)
- Reference: `crates/raios-core/src/db/wf_handoff.rs` and the swarm/task-graph workflows (approval rows written into `cp_approvals`; resolution sets `status`/`resolved_by`)

**Steps:**
- [ ] Identify the exact write path for `cp_approvals` resolution (grep `cp_approvals` + `resolved_by` across `raios-core/src/db/`) and confirm whether a broadcast event already fires on resolution; if not, add one following the existing `JobComplete`/`JobFailed` event pattern
- [ ] In the evolution worker, count consecutive same-tool approvals per `(tool_name, project)` with zero interleaved denials
- [ ] After N consecutive approvals (start N=5, configurable), insert a candidate via the existing pattern: `command = tool_name`, `outcome = "repeated_manual_approval"`, `suggestion = "[policy] allow tool <tool_name> in <project>"`, `status = "pending"` — dedup-by-`suggestion` already prevents duplicates
- [ ] A denial breaking the streak resets the counter (no candidate); do not delete an already-emitted candidate
- [ ] Surface via existing `raios instinct list` (rows show up naturally; the `[policy]` prefix makes them scannable)
- [ ] Test: simulate 5 approvals for the same tool via the `CandidateStore` test harness → assert one `[policy]`-prefixed candidate row; simulate a denial mid-streak → assert no candidate
- [ ] Commit: `feat(evolution): suggest policy-rule promotion from repeated manual approvals`

---

### Task 5: Hooks system in the Policy Manager

**Files:**
- Modify: `crates/raios-core/src/security/policy.rs` (Phase 2 Policy Manager — this is the extension point, not a new module)
- Create: `crates/raios-core/src/security/hooks.rs`
- Modify: `raios-policy.toml` schema (add `[hooks]` table)

**Steps:**
- [ ] Add `[hooks]` section to the TOML schema: `pre_tool_call = "path/to/script"`, `run_start`, `run_end`, `handoff_sent` — each optional
- [ ] In `hooks.rs`, implement `run_hook(name: &str, payload: &HookPayload) -> HookOutcome` — shells out via `std::process::Command`, JSON payload via stdin or env var (cap at 16KB like jcode does), exit code contract: `0` = allow/continue, `2` = block (for `pre_tool_call` only) with stderr as the block reason surfaced back to the calling agent, anything else = fail-open (log + continue)
- [ ] Add a recursion guard: set `RAIOS_HOOKS_DISABLED=1` in the spawned hook's environment so a hook script that itself shells out to `raios` doesn't recurse
- [ ] Wire `pre_tool_call` into the existing policy-gate path. Note: the current gate is `PolicyConfig::validate_tool_call(&self, tool_name: &str) -> Result<()>` (uses a `"confirm:"` error-prefix convention). It only receives `tool_name` — to give a hook useful context you must either thread the tool arguments into this call site or add a sibling `validate_tool_call_with_hook(tool_name, payload)`. The hook runs *after* the TOML allow/deny/confirm decision, only when the decision was `Allow` (hooks add restriction, never bypass a `deny`)
- [ ] Wire `run_start`/`run_end`/`handoff_sent` as fire-and-forget calls at the corresponding existing lifecycle points (`raios run`, `raios handoff`)
- [ ] Test: a `pre_tool_call` hook script that exits 2 blocks a tool call that the TOML rules would have allowed; a hook that exits 1 (crashes) does not block (fail-open verified)
- [ ] Commit: `feat(security): add scriptable lifecycle hooks (pre_tool_call/run_start/run_end/handoff_sent) to Policy Manager`

---

### Task 6: Automated verify-gate before handoff SUCCESS

**Files:**
- Modify: `crates/raios-core/src/db/wf_task_graph.rs` (already has `cp_task_graph_nodes` — add a node kind)
- Modify: `crates/raios-core/src/db/wf_handoff.rs` (`create_handoff_workflow` — gate the SUCCESS path)

**Steps:**
- [ ] Add a `node_kind` column to `cp_task_graph_nodes` (default `'work'`, new value `'verify_gate'`)
- [ ] Add `insert_verify_gate_node(conn, graph_id, parent_node_id, shell_cmd)` in `wf_task_graph.rs`, reusing `create_task_graph_node_workflow`'s pattern (it already takes a `shell_cmd` param — a verify gate is just a work node whose `shell_cmd` is the build/test command)
- [ ] In `create_handoff_workflow`, before setting `status = 'success'`, check: does an open `verify_gate` node exist for this task's graph? If yes and it hasn't passed, reject the handoff attempt with a clear error (return `Err`, don't silently downgrade status) rather than allowing SUCCESS to be recorded
- [ ] Make this opt-in per task (a `require_verify_gate: bool` flag on task creation) — don't force every existing handoff caller to suddenly need a gate
- [ ] Test: create a task with `require_verify_gate = true`, attempt handoff SUCCESS before the gate node passes → assert rejection; mark gate node passed → assert handoff now succeeds
- [ ] Commit: `feat(swarm): add opt-in automated verify-gate blocking handoff SUCCESS`

---

### Task 7: `raios doctor <agent>` — tiered provider/agent health check

**Files:**
- Modify: `crates/raios-runtime/src/system_scan/usage.rs` (already has `scan_codex_usage`, `scan_claude_usage`, `UsageConfidence`, `USAGE_CACHE_STALENESS_HOURS = 24` — this is the direct extension point)
- Create: `crates/raios-runtime/src/system_scan/doctor.rs`
- Modify: `crates/raios-surface-cli/src/cli/dev.rs` (add `doctor` subcommand alongside existing dev-tooling commands)

**Steps:**
- [ ] Define `DoctorTier { Offline, Auth, Full }` and `DoctorResult { agent: String, tier_reached: DoctorTier, notes: Vec<String>, checked_at: String }`
- [ ] `Offline` tier: reuse the existing `installed` check pattern from `scan_codex_usage`/`scan_claude_usage` (binary on PATH via `resolve_command_path`, config dir exists)
- [ ] `Auth` tier: reuse the existing credential-file / env-var checks already in `usage.rs` (`~/.codex/auth.json`, `~/.claude/.credentials.json`, `OPENAI_API_KEY` etc.) — flag any check older than `USAGE_CACHE_STALENESS_HOURS` (already defined!) as `"stale, re-check"` per jcode's `AUTH_CREDENTIAL_SOURCES.md` lesson, rather than trusting a cached pass silently
- [ ] `Full` tier: shell out a trivial round-trip through `raios run <agent> -- <minimal no-op task>` (or the existing `raios run` machinery) with a short timeout; record pass/fail
- [ ] Persist `DoctorResult` rows in a small new table (`agent_doctor_runs`) via `schema.rs` migration — this is the "coverage ledger" jcode keeps
- [ ] Add `raios doctor <agent> [--tier offline|auth|full]` CLI command in `dev.rs`, and surface the last result next to `raios usage`'s existing output
- [ ] Add MCP tool parity (`mcp__raios__doctor`) per the Global Constraints rule
- [ ] Test: mock an agent with no binary installed → `Offline` tier fails with clear message; mock one with stale auth (fake a >24h-old cached check) → flagged stale, not silently green
- [ ] Commit: `feat(system_scan): add raios doctor tiered health check for agent CLIs`

---

### Task 8: Graph-aware memory (supersede-collapse at export + relation edges)

**Reframed (per Verification Findings #1 and #2).** The first draft was wrong: `mem_items` have no semantic-retrieval path to bolt an "expansion pass" onto — `raios memory` searches **`memory.md` text via Cortex** (`cli/workspace.rs` → `cortex.index_memory_files`/`search_with_filter`), and `cortex/store.rs` is code-chunk search. Meanwhile `mem_lineage` is **already** a relation graph. So the high-value, correctly-placed win is: **make the supersede/contradict graph do work at the `mem_export` boundary** so stale/superseded items never reach the `memory.md` that Cortex indexes — that is what actually improves recall quality of `raios memory`. A brand-new `mem_edges` table is unnecessary; extend `mem_lineage`.

**Files:**
- Modify: `crates/raios-core/src/db/mem.rs` (`mem_lineage_add` at line 289, `mem_lineage_parents` at 306; `mem_export` at line 158 — the export→memory.md path Cortex later indexes)
- Modify: `crates/raios-core/src/db/schema.rs` only if a helpful index on `mem_lineage(relation)` is needed — the `relation` column already exists (free TEXT, default `'derived_from'`), so no new table
- Reference: `crates/raios-surface-cli/src/cli/workspace.rs` (the `raios memory` search path whose input quality this improves)

**Steps:**
- [ ] Establish the relation vocabulary already in use: `mem_lineage.relation` currently defaults to `'derived_from'`; standardize on adding `'supersedes'` and `'contradicts'` as recognized values (no CHECK constraint change needed — column is free TEXT, but document the vocabulary)
- [ ] Add helpers in `mem.rs`: `supersede_chain(conn, item_id) -> Vec<String>` (walk `supersedes` lineage to the terminal/current item) and `is_superseded(conn, item_id) -> bool`
- [ ] Modify `mem_export` so superseded items are dropped (or folded into their successor) before writing `memory.md` — this is the actual recall-quality lever, since Cortex only ever sees the exported text
- [ ] Optionally surface `contradicts` edges as a warning annotation in the exported `memory.md` (so a human/agent reading it sees the conflict), rather than silently dropping either side
- [ ] Do **not** add an LLM listwise rerank (jcode's optional "Mode 2") and do **not** build a parallel `mem_edges` table — reuse `mem_lineage`
- [ ] Test: create item A, supersede it with B via `mem_lineage_add(..., relation='supersedes')`, run `mem_export`, assert the exported `memory.md` contains B and not A; assert `supersede_chain(A)` resolves to `[B]`
- [ ] (Stretch, separate commit) Measure: index the before/after `memory.md` through Cortex and compare `raios memory` results for a query that A/B both matched, to confirm the stale item stopped surfacing — mirror jcode's before/after recall methodology
- [ ] Commit: `feat(mem): collapse superseded items at export via mem_lineage graph`

---

### Task 9 (hardest): Ownership-partitioned task graph expansion

**Grounding (verified):** `cp_tasks.parent_task_id` already exists (`REFERENCES cp_tasks(id) ON DELETE SET NULL`) and `cp_tasks.assignee_id` is the ownership field. Source of truth is `cp_task_graphs`/`cp_task_graph_nodes` — `task_graphs`/`task_graph_nodes` are COMPAT CACHES (marked so in `schema.rs`), do NOT write to them directly.

**Files:**
- Modify: `crates/raios-core/src/db/schema.rs` (no `cp_tasks` change needed — `parent_task_id` present; add an index if ancestry walks need it)
- Modify: `crates/raios-core/src/db/wf_task_graph.rs` (`create_task_graph_node_workflow` and friends — this already writes `cp_tasks` + `cp_agent_runs` + `cp_task_graph_nodes` atomically; the new expansion reuses this insert path)
- Modify: `crates/raios-surface-cli/src/cli/mod.rs` + `cli/args.rs` (new `raios swarm expand` verb)
- Modify: MCP surface for parity (`raios-surface-mcp`)

**Steps:**
- [ ] Confirm current model: read `wf_task_graph.rs` in full and the `cp_tasks`/`cp_task_graph_nodes` schema in `schema.rs` to establish exactly what "ownership" means today (right now a node has one `assignee_id` at creation time, set by whoever calls `create_task_graph_node_workflow`)
- [ ] Add `expand_task_node(conn, graph_id, owner_node_id, requesting_agent, new_node: NewNodeSpec) -> Result<NodeId>`: validates `requesting_agent == cp_tasks.assignee_id` for `owner_node_id` (ownership check), validates the new node's parent pointer doesn't create a cycle (walk `parent_task_id` chain, reject if `owner_node_id` appears in its own ancestry), then inserts via the existing `create_task_graph_node_workflow` path — this is additive, no lock needed because writes are partitioned by ownership rather than serialized through one coordinator
- [ ] Add `raios swarm expand --graph <id> --owner-node <id> --agent <name> --spec <json>` CLI verb
- [ ] Decide and document a hard cap on graph depth/fanout for this project's scale (jcode's 1000-agent cap doesn't apply — raios's fixed 4-agent matrix means the relevant cap is *depth* of sub-task chains per agent, not agent count; start with depth ≤ 5 as a sane default, configurable)
- [ ] Do **not** implement jcode's broadcast/DM channel comms or recursive agent spawning — out of scope per the mining report (raios supervises 4 external CLIs, it doesn't spawn nested in-process agents)
- [ ] Test: agent A owns node X, calls `expand_task_node` to add child Y under X → succeeds; agent B (not owner of X) attempts the same → rejected; an expansion that would make X a descendant of itself → rejected as a cycle
- [ ] Migration test: existing single-assignee `cp_tasks` flows (the current handoff model) continue to work unchanged — this is a strict superset, not a replacement
- [ ] Commit: `feat(swarm): add ownership-partitioned task graph expansion (raios swarm expand)`

---

## Self-Review Notes

- **Coverage:** all 9 items from the jcode mining report (memory graph, handoff schema, confidence decay, approval→policy promotion, hooks, verify-gate, doctor, task-graph expansion, budget check) map 1:1 to a task above.
- **Grounding:** every task references real files confirmed to exist in the current `R-AI-OS` tree as of 2026-07-14 (`wf_task_graph.rs`, `wf_handoff.rs`, `handoff.rs`, `mem.rs`, `evolution.rs`, `usage.rs`, `cortex/store.rs`, `search/hybrid.rs`, `policy.rs`, `workspace.rs`) rather than guessed paths. A source-verification pass (see "Verification Findings" at top) corrected four tasks: Task 3 (dropped the illegal instinct-table ALTER), Task 4 (no `--type` column → suffix-prefix convention), Task 8 (rewritten — mem_items have no semantic search path; the real lever is supersede-collapse at `mem_export`, reusing `mem_lineage` not a new table), and Task 1/2/9 anchor fixes. Several tables (`cp_tasks.parent_task_id`, `cp_task_graph_nodes`, `USAGE_CACHE_STALENESS_HOURS`, `mem_lineage.relation`) already exist, which turned Tasks 3/6/7/8/9 from "build from scratch" into "extend."
- **Explicitly excluded** (per the original mining report, not silently dropped): AgentCard/discovery marketplace, soft-interrupt/resume/multi-session client architecture, crate-ownership compile-speed refactors, agent-native VCS lane/draft-patch design, LLM-sidecar rerank, broadcast/DM comms, recursive agent spawning.
