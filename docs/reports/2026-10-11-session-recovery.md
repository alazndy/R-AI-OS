# Global saved conversation recovery — 2026-10-11

Author: Codex Kaira.

Requirement: after power loss, battery exhaustion, or an interrupted agent process, select a saved conversation across projects and continue it in the correct directory. Recovery must not depend on normal shutdown or select an unrelated "latest" session.

## Implemented behavior

```sh
raios resume
raios resume --list --json
raios resume --list --agent codex --search "database"
raios resume codex:<native-conversation-uuid> --dry-run --json
raios resume claude:<native-conversation-uuid>
```

The default interactive terminal command displays a numbered picker across projects. Empty input cancels. Headless execution requires an explicit ID or a read-only listing. Selection uses exact `agent:id` keys, native IDs or an exactly linked wrapper run ID; ambiguous or missing IDs fail. List filters apply before the display limit. Selection is checked against filters and rescanned before launch; changed identity/project, known running/unverified wrapper ownership and missing directories block continuation. `--dry-run` returns the agent, project and exact argument vector without launching or writing.

Continuation runs through the existing RAIOS wrapper and its process witnesses. It switches to the saved project directory and uses Codex `resume <uuid>`, Claude `--resume <uuid>`, or OpenCode `--session <id>`. These argument forms were verified from the installed agents' local help. New handoffs are deferred during recovery; old conversation selection does not silently consume a new handoff.

## Persistence and crash boundary

Additive `cp_wrapper_resume` rows store wrapper run ID, absolute workspace root and an optional exact native conversation ID. The launch journal is created in the same transaction as task, contract, run and process witness, before child spawn. SQLite connections explicitly use WAL, foreign keys and `synchronous=FULL`. A failed checkpoint aborts launching the agent, including unnamed sessions. Normal shutdown is not needed to retain launch metadata.

Claude supports explicit creation IDs: RAIOS generates `--session-id <uuid>` before launch and durably binds that ID with an audit event before spawn. Explicit resume/session-ID flags are honored; continue/fork/no-persistence modes are not falsely bound to a guessed new identity. Exact-ID continuation of Codex/OpenCode also binds the known native ID. A transactional guard prevents two running wrappers of the same agent from binding the same conversation. Only process-witnessed dead runs are reconciled on continuation; legacy rows are excluded and uncertain sessions are retained.

Codex and OpenCode generate IDs internally for new sessions. Their own native stores persist conversations during work; global discovery reads those stores after an interruption without needing a successful wrapper exit. RAIOS does **not** guess a wrapper/native association from timestamps or the latest file. Unlinked wrapper names/work remain separate metadata entries until an exact native identity is known. Their native conversations can still be selected directly from the global inventory.

Conversation bodies remain in the agents' native stores. RAIOS stores pointers and existing canonical task labels, not another copy of provider histories. Crash continuation resumes the last state actually persisted by the agent, not unflushed RAM, an unfinished external action, or an unsaved editor buffer. Disk failure or deliberate native-history deletion cannot be recovered from an index.

## Discovery boundaries

- Codex: `$CODEX_HOME/sessions` or `~/.codex/sessions`, verified `session_meta` UUID/CWD metadata; names from the bounded native session index.
- Claude: `$CLAUDE_CONFIG_DIR/projects` or `~/.claude/projects`, top-level conversation files with matching filename/session UUID and direct CWD; subagent files are excluded.
- OpenCode: `$XDG_DATA_HOME/opencode/opencode.db` (default `~/.local/share`), read-only session metadata. Child/archived sessions are excluded and unsupported schemas are reported.
- Existing control-plane metadata is joined read-only; inventory never migrates the live DB. Missing/unreadable DBs still allow native discovery. Existing pre-feature databases use contract roots for wrapper metadata.
- Native files are read through the existing checked regular-file opener; walked symlinks and nested Claude subagents are skipped. Header reads, index reads and inventory traversal have bounds; traversal limits and unreadable directories produce inventory notes. Labels are redacted and terminal controls removed before display.
- Native files without valid metadata or a saved project are not treated as resumable. No timestamp-only identity inference or `--last` fallback.
- Antigravity's installed CLI exposes `--conversation`, but no verified local catalog adapter was established. Its wrapper launches are journaled; no invented recovery support is claimed.
- Known wrapper concurrency is guarded. Independently launched clients outside RAIOS cannot all be proven inactive by these adapters; provider-side locks and behavior still apply.

## Validation and deployment

- Full offline workspace suite: 1,262 tests passed, zero failures; two existing documentation examples ignored. Strict all-target/all-feature Clippy, rustfmt and whitespace checks passed.
- New regression tests cover reopening checkpoints without session end, immutable binding, audit rollback, concurrent binding exclusion, literal native identities/CWD, missing directories, symlink/subagent/forged-ID rejection, partial JSONL tails, old-schema read-only discovery, OpenCode child/archive exclusion and native argument planning.
- Existing handoff fixture now verifies three audit events, including the additional pre-spawn native-identity binding; its handoff state and delivery checks remain intact.
- Debug subprocess interruption test passed: SIGKILL the wrapper and fake Claude child, reconstruct the conversation from another directory, verify the read-only plan causes no audit writes, resume the exact UUID in the original project, reconcile the interrupted run, and verify foreign keys and audit chain. No paid model request.
- Optimized release CLI build passed (5m 51s). The same SIGKILL interruption scenario passed against the release binary, including native identity/project restoration, read-only planning, audited reconciliation and clean foreign keys.
- Read-only release inventory on the real host returned 1,030 records: 232 native conversations with available directories (Claude 51, Codex 173, OpenCode 8), five native conversations whose project directories are missing, and 793 wrapper records without an available exact native association. No discovery warnings. Those wrapper rows are retained as unavailable; they are not declared recovered.
- All 153 protected baseline file hashes match. No live database migration, live process cancellation, native history write, daemon restart or bulk relabeling was performed by validation.
- Installation was attempted only after validation with previous-binary hash checking, a retained backup and atomic replacement. Staging the new CLI in `/home/alaz/.local/bin` failed with `EROFS` under the current workspace-write/never-approval environment. The old installed CLI remains intact at SHA-256 `85b6843d4bbbad928155f5193741384c1a5b6e7e9aa1ddc3eca092b813465806`; a copy is retained at `.tmp/session-recovery-previous-raios`. No permission escalation or write workaround was attempted.
- The verified executable is `target/release/raios`, SHA-256 `02428a1e3a8530be093e5248cfb38b34cb4785430261217e062dc5c67f67a82c`. From any directory, use `/home/alaz/dev/core/R-AI-OS/target/release/raios resume` in the user's normal terminal. Its first wrapped launch performs the additive migration and writes recovery metadata. The PATH-installed `raios` does not yet expose this recovery command.
- Evidence: `.tmp/session-recovery-{tests-final,clippy-final,release}.log`, `.tmp/session-recovery-smoke-{debug,release}.json`, `.tmp/session-recovery-live-{debug,release}.json`, and `.tmp/session-recovery-install-attempt.log`.

Pre-change native security/dependency diagnostics were collected. The pattern scanner flags test-only fake password text, a fixed test table-name loop, traversal rejection fixtures in the protected native workspace, schema HTTP identifiers, and an existing auth-header `.unwrap()`. These are reviewed findings rather than a clean-security claim; this task does not edit protected fixtures. The local dependency command reports no outdated entries and zero vulnerabilities, subject to that command's coverage.

No changes to `tools/raios-workspace*`, `docs/superpowers/plans`, README or SIGMAP; native desktop UI integration stays within the previously excluded scope. No commit, paid model request or forced native-history rebuild.
