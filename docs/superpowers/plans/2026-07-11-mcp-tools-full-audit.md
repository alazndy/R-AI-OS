# Full MCP Tool Surface Audit

> **For agentic workers:** This is a verification/testing task, not new architecture. The methodology that matters here was proven this session on `grep_search`/`semantic_search`: don't trust that a tool "should" work because it compiles and has a schema — call it live, over the real JSON-RPC protocol, against a real project, and check the actual output. That exact process found three real bugs in two tools (a >60s perf bug, a tool_pin security block, a duplicate-match bug) that had been sitting undetected. Assume the other 31 tools have not received this treatment and may hide similar issues.

**Goal:** Every MCP tool `raios mcp-server` exposes (33 total — see list below) gets a real, live `tools/call` invocation and a human judgment on whether the result is correct, not just "didn't crash."

**Why this matters:** Every agent that connects to raios over MCP (Claude, Codex, OpenCode, external tools) only ever sees this surface — never the CLI. A tool that's subtly broken here is invisible until an agent hits it mid-task and either silently gets bad data or visibly fails in front of a user. `semantic_search` was broken (>60s, unusable) for as long as it existed and nobody noticed until this session's dogfooding pass.

**Protocol reminder:** JSON-RPC 2.0 over stdio.
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | raios mcp-server
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"<tool>","arguments":{...}}}' | raios mcp-server
```
If you hit `-32028 tool_pin: manifest tampered`, that means the tool manifest hash drifted since last pin (expected the *first* time you run this, since you're not changing tools — if it happens, it's stale from a previous session, not something you caused). Verify with `git log --oneline -- crates/raios-surface-mcp/src/mcp/tools.rs` that nothing unexpected changed, then `raios pin-reset`.

## Tool inventory (33, all in `crates/raios-surface-mcp/src/mcp/tools.rs` + handlers in `tools.rs`/`tools_workspace.rs`)

Already verified this session — skip: `semantic_search`, `locate_search`.

## Part 1: Read-only / info tools (call once each, judge output sanity)

For each, call with a real project (R-AI-OS itself, or another registered project via `raios projects`) and confirm the response is well-formed JSON, matches what the tool's `description` in `tools/list` promises, and isn't obviously wrong (e.g. a stats tool returning all-zero on a project you know has data).

- [ ] `get_health` — cross-check against `raios health <project>` CLI output for the same project; should agree.
- [ ] `list_projects` — cross-check count/names against `raios projects` CLI.
- [ ] `get_stats` — cross-check against `raios stats` CLI.
- [ ] `project_info` — call with a known project name and a known absolute path; both should resolve to the same project.
- [ ] `portfolio_status` — sanity-check the summary against what you know is actually true of the workspace.
- [ ] `disk_usage` — cross-check one project's reported size against `du -sh` on that project's real path.
- [ ] `list_ports` — cross-check against `raios ps` CLI or plain `ss -tlnp`.
- [ ] `usage_status` — cross-check against `raios usage` CLI.
- [ ] `version_info` — should report `3.5.0` (current, post-release). If it reports something else, that's a real bug (stale version detection) — investigate before anything else.
- [ ] `env_status` — call against a project with a real `.env` file (check `raios env` CLI targets from prior sessions, or any project you know has one) and confirm it actually finds missing/undocumented keys, not just returns empty.
- [ ] `deps_status` — cross-check against `raios deps` CLI for the same project; should agree on outdated-package count.
- [ ] `git_status` — call against a repo with actual uncommitted changes (make a throwaway edit in a scratch project if none exists) and confirm it reports them accurately, not just "clean" by default/bug.
- [ ] `git_log` — confirm returned commits match `git log` for the same repo, same order, same count when a limit is passed.
- [ ] `git_diff` — call with actual uncommitted changes present; confirm the diff content is real and not truncated/empty when it shouldn't be.
- [ ] `get_validation_errors` — requires the daemon (`GetState` command over the same TCP protocol `locate_search`/`semantic_search` use, see `tool_get_validation_errors` in tools_workspace.rs) — confirm it actually reaches the daemon and doesn't silently return an empty/wrong result if the daemon is down (should error clearly, not fake-succeed).
- [ ] `list_swarm_tasks` — call after `create_swarm_task` (Part 2) creates at least one, confirm it shows up.
- [ ] `get_inbox` — cross-check against whatever `raios` surfaces as pending inbox items via CLI/TUI.
- [ ] `list_evolution_candidates` — cross-check against `raios evolve` CLI listing.
- [ ] `get_agent_stats` — cross-check against `raios agent-stats` CLI for the same agent identity.
- [ ] `ask_architect` — ask a real architectural question about R-AI-OS itself (e.g. "why does the daemon use a dedicated OS thread for Cortex instead of async") and judge whether the answer cites real, relevant source/decision content — this is semantic-search-backed (`Cortex::search`), so apply the same relevance judgment used for `semantic_search` in the prior verification round.
- [ ] `route_capability` — call with a realistic natural-language task description and confirm the routing decision is plausible (points at a capability that actually makes sense for that task).

## Part 2: State-mutating tools (test carefully, isolated scope — never against R-AI-OS's own live repo/DB unless explicitly read-only)

These have real side effects. **Do not run `git_commit`, `run_build`, `run_tests`, or `version_bump` against this repo (R-AI-OS) or any repo with real uncommitted work** — use a disposable scratch git repo (`mktemp -d && git init`) or an isolated worktree you create specifically for this test and can throw away.

- [ ] `add_task` — create a task via MCP, then confirm it's visible via `list_swarm_tasks`/`raios task` CLI and has the fields you set (not silently dropped/defaulted).
- [ ] `create_swarm_task` — create one in a scratch project, confirm it actually creates an isolated worktree (check `git worktree list` in that scratch repo afterward) — this is exactly the mechanism that leaves behind `.claude/worktrees/`-style artifacts (see 2026-07-11's `SKIP_DIRS` fix for why stale ones matter), so also confirm cleanup/lifecycle: does anything ever remove it, or is that manual?
- [ ] `approve_swarm_task` — approve the task created above, confirm its status actually transitions (query the control plane or `list_swarm_tasks` again).
- [ ] `handover` — call it, then check `raios sessions`/the control plane (`cp_tasks`/`cp_artifacts`, same tables inspected via `sqlite3 -readonly ~/.config/raios/workspace.db` earlier this session) to confirm the handoff was actually recorded, not just acknowledged in the response text.
- [ ] `session_note` — call it, then check it actually appended to the target project's real `memory.md` (not a different file, not silently no-op'd) with correctly-formatted content matching the template in AGENT_CONSTITUTION.md section 6.
- [ ] `update_state` — call it and confirm whatever state it's meant to update actually changed (check the specific file/DB row the handler touches — read `tool_update_state` in tools_workspace.rs first to know what to check).
- [ ] `version_bump` — **only in a scratch Cargo project**, never here. Confirm it actually edits the version and (if `--changelog`/`--tag`-equivalent args exist in the MCP schema) behaves consistently with the CLI's `raios version-bump`.
- [ ] `run_build` — **only in a scratch project** with a trivial buildable target (e.g. `cargo new` a tiny crate). Confirm it actually runs the build and reports real pass/fail, not a hardcoded success.
- [ ] `run_tests` — same scratch-project caveat. Confirm it runs real tests and reports real pass/fail counts.
- [ ] `git_commit` — **only in a scratch git repo you created for this test.** Confirm it stages+commits what you expect, with the message you passed, and doesn't do anything broader (e.g. accidentally `git add -A` when you only wanted specific files — this class of bug, over-broad staging, is exactly the kind of thing worth checking given AgentShield's "no implicit trust" principle).
- [ ] `promote_evolution_candidate` — promote a real pending candidate (check `raios evolve` CLI for one, or create one first if none exist) and confirm it actually transitions to promoted, matching `raios instinct`/`raios evolve` CLI-visible state afterward.

## Part 3: Security-relevant cross-check

- [ ] Run `raios security` on `crates/raios-surface-mcp/` specifically — this crate is the entire externally-reachable attack surface (every one of these 33 tools is something an agent, potentially compromised or prompt-injected, can call). Read findings critically per this codebase's own documented caveat (`raios security` is pattern-based, not taint-analysis — a clean scan means "no known pattern," not "no vulnerabilities").
- [ ] For `run_build`/`run_tests`/`git_commit` specifically: read the handler source and confirm arguments are never interpolated into a shell string (`sh -c "..."` style) — this exact bug class (shell injection via `{input}` substitution) was found and fixed elsewhere in this codebase on 2026-07-02 (see memory.md that date) in `proxy_store.rs`'s `Backend::Shell`; confirm these MCP handlers don't share that pattern.

## Report

```bash
raios handoff --to claude-kaira --status <success|failed|blocker> -p R-AI-OS --msg "<verbatim: per-tool pass/fail summary, any bugs found+fixed with file:line, the shell-injection cross-check result, raios security findings on raios-surface-mcp>"
```
If any code changes were made (bug fixes), work in a new isolated worktree, `cargo test --workspace` must stay green, and do not merge/push without that handoff.
