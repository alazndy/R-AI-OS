# Search Tools — MCP Path & Ecosystem Coverage Verification

> **For agentic workers:** This is a verification/testing task, not new architecture. Follow it step by step; if any step surfaces a real bug (not just an expected finding below), stop and root-cause it before moving on — this exact pattern (dogfood → find a real bug → fix → re-verify) already happened three times on 2026-07-10 (extension list, walker depth, skip-dirs). Expect a fourth.

**Goal:** Close the two gaps Claude Kaira explicitly flagged as untested after yesterday's trigram grep + resident Cortex work: (1) the MCP tool paths (`grep_search`, `semantic_search`) that agents actually call — only the CLI (`raios grep`/`raios search`) has been verified so far; (2) ecosystem coverage beyond the 5 projects already tested (Rust, C, TypeScript, Python×2) — specifically Flutter/Dart, which is likely a repeat of the same extension-list gap.

**Context:** `crates/raios-runtime/src/search/indexer.rs`'s `INDEXED_EXTS`/`SKIP_DIRS` are the single source of truth for all three engines (BM25, trigram, Cortex — `cortex/mod.rs` imports both). Yesterday's fixes are in commits `d5c7f38` (extensions + depth) and `238ec9a` (skip-dirs). Current `INDEXED_EXTS`: `md, rs, ts, tsx, js, jsx, py, toml, json, yaml, yml, go, kt, kts, java, swift, c, cc, cpp, h, hpp, cs, rb, php, sh, sql` — **no `dart`**. Current installed binary: `raios 3.4.0` at `~/.local/bin/raios` + `~/.cargo/bin/raios`, `aiosd` daemon running the matching build.

---

## Part 1: MCP tool_pin is currently blocking ALL tool calls — resolve first

Claude Kaira found this by hand-testing the protocol just now (not a plan assumption — verified live):

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"grep_search","arguments":{"pattern":"DriveModeManager"}}}' | raios mcp-server
```

Currently returns:
```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32028,"message":"tool_pin: manifest tampered — all tool calls blocked. Run `raios pin-reset` after verifying the binary."}}
```

This is `raios-core::security::tool_pin` doing its job correctly — it hashes the MCP tool manifest and refuses calls when the hash drifts from the last pinned value, as a supply-chain-tamper defense. The manifest legitimately changed yesterday: `grep_search` and `semantic_search`'s `path` parameter were both added to the tool list (commits from 2026-07-10's trigram grep + BM25 cache wiring work, already merged and tested). This is expected, benign drift — not real tampering.

- [ ] **Step 1:** Confirm the drift is explained by yesterday's real changes, not something else:
```bash
cd /home/alaz/dev/core/R-AI-OS
git log --oneline -- crates/raios-surface-mcp/src/mcp/tools.rs | head -10
git diff d486549..HEAD -- crates/raios-surface-mcp/src/mcp/tools.rs | grep '^\+.*"name":' 
```
Expected: shows `grep_search` (and `semantic_search`'s `path` param) as the diff, nothing unexpected. If something you don't recognize also changed, STOP and investigate before resetting the pin — that's exactly the scenario this mechanism exists to catch.

- [ ] **Step 2:** Reset the pin:
```bash
raios pin-reset
```

- [ ] **Step 3:** Re-run the Part 1 smoke test command above. Expected: a real JSON result (either matches or an empty result set, not the -32028 error).

- [ ] **Step 4:** Note in your final report whether this blocker existed in your environment too, or was specific to Claude Kaira's session — if `tool_pin`'s state is per-machine (likely, it's probably in `~/.config/raios/`), you may hit the identical block and need to repeat Steps 1-3 yourself.

## Part 2: Live MCP verification — `grep_search`

- [ ] **Step 1:** `tools/list` and confirm `grep_search`'s schema (pattern required, path/case_insensitive optional):
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | raios mcp-server | python3 -m json.tool | grep -A 15 '"grep_search"'
```

- [ ] **Step 2:** Call it against GT-Launcher (already proven via CLI to have 16 real `DriveModeManager` matches, commit `d5c7f38`'s own verification) and cross-check the MCP result matches:
```bash
cd /home/alaz/dev/mobile/GT-Launcher
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"grep_search","arguments":{"pattern":"DriveModeManager"}}}' | raios mcp-server > /tmp/mcp-grep-result.json
python3 -c "
import json
with open('/tmp/mcp-grep-result.json') as f:
    resp = json.load(f)
text = resp['result']['content'][0]['text']
print(text[:2000])
"
```
Expected: same 16 matches (modulo the `.claude/worktrees/` duplicate directory noise already known from CLI testing — filter that out same as before) as `raios grep "DriveModeManager"` found via CLI.

- [ ] **Step 3:** Call it with an explicit `path` argument from a DIFFERENT cwd (proving the MCP tool's own scope resolution — `resolve_search_scope` in `tools_workspace.rs` — works, not just the CLI's):
```bash
cd /tmp
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"grep_search","arguments":{"pattern":"DriveModeManager","path":"/home/alaz/dev/mobile/GT-Launcher"}}}' | raios mcp-server
```
Expected: same result as Step 2, proving `path` correctly overrides the (here, irrelevant) cwd.

- [ ] **Step 4:** Case-insensitive flag: 
```bash
cd /home/alaz/dev/mobile/GT-Launcher
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"grep_search","arguments":{"pattern":"drivemodemanager","case_insensitive":true}}}' | raios mcp-server
```
Expected: same matches as Step 2 (case-insensitive finds the same content that's actually `DriveModeManager` in the source).

## Part 3: Live MCP verification — `semantic_search`

- [ ] **Step 1:** Call against R-AI-OS itself for a concept query (semantic search's actual use case, not exact match):
```bash
cd /home/alaz/dev/core/R-AI-OS
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"semantic_search","arguments":{"query":"how does the resident cortex worker avoid rebuilding HNSW on every search","top_k":5}}}' | raios mcp-server > /tmp/mcp-search-result.json
python3 -c "
import json
with open('/tmp/mcp-search-result.json') as f:
    resp = json.load(f)
print(resp['result']['content'][0]['text'][:2000])
"
```
Expected: results reference `daemon/cortex.rs`'s dirty-flag logic, or related files (`daemon/handlers.rs`'s `VectorSearch` arm) — a genuinely relevant semantic hit, not garbage. Judge relevance yourself; this is inherently fuzzier than grep's exact-match verification.

- [ ] **Step 2:** Confirm response time is consistent with the resident-daemon path (should be ~1s-class per yesterday's verified numbers, not ~4-6s fallback) — `aiosd` should already be running from yesterday's install, confirm:
```bash
systemctl --user is-active aiosd
time (echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"semantic_search","arguments":{"query":"test query","top_k":3}}}' | raios mcp-server > /dev/null)
```
Note: unlike `raios search`, `tool_semantic_search`'s handler in `tools_workspace.rs` — check whether it was ever wired to delegate to the daemon the same way `cmd_search` in `cli/search.rs` was (2026-07-10's cortex-daemon-residency plan explicitly scoped this as "MCP-side delegation... one consumer migrated first, deliberately" — meaning `semantic_search` may STILL be doing full in-process `Cortex::init()` per call, unlike the CLI). If this call takes ~4-6s instead of ~1s, that's not a bug — it's the known, already-documented deferred scope from yesterday's design doc (`docs/superpowers/specs/2026-07-10-cortex-daemon-design.md`'s Non-Goals). Report the actual number either way, and if it's slow, note it as a legitimate follow-up (migrating `semantic_search` to daemon delegation too) rather than something to fix in this task.

## Part 4: Flutter/Dart ecosystem coverage

Two real Flutter projects exist in this workspace: `/home/alaz/dev/mobile/NEXUS` and `/home/alaz/dev/mobile/esp32flutter` (both have `pubspec.yaml`). `dart` is NOT in `INDEXED_EXTS` — this is very likely the same class of gap as yesterday's Kotlin discovery, verify and fix if so.

- [ ] **Step 1:** Confirm `.dart` files exist and pick a real symbol to search for:
```bash
find /home/alaz/dev/mobile/NEXUS -maxdepth 6 -iname "*.dart" | head -5
grep -n "^class \|^Widget \|^void " $(find /home/alaz/dev/mobile/NEXUS -maxdepth 6 -iname "*.dart" | head -1) | head -3
```

- [ ] **Step 2:** Test `raios grep` against that real symbol and diff against real `grep -rn --include="*.dart"`, exactly the same methodology as yesterday's 5 projects:
```bash
cd /home/alaz/dev/mobile/NEXUS
raios grep "<the symbol from Step 1>" --reindex 2>&1 | grep -v "\.claude/worktrees" > /tmp/nexus-raios.out
grep -rn "<the symbol from Step 1>" --include="*.dart" . > /tmp/nexus-real.out
diff <(sed "s|.*NEXUS/||" /tmp/nexus-raios.out | sort) <(sort /tmp/nexus-real.out)
```

- [ ] **Step 3:** If it finds zero `.dart` matches (expected, matching the hypothesis): add `dart` to `INDEXED_EXTS` in `crates/raios-runtime/src/search/indexer.rs` (same single-source-of-truth constant Steps from yesterday's commits `d5c7f38`/`238ec9a` — read that constant's doc comment first, it already explains the pattern). Also check `esp32flutter` — it's a *second* Flutter project; use it as a second confirmation after the fix, not just NEXUS.

- [ ] **Step 4:** Also check depth: Flutter/Dart projects conventionally nest under `lib/src/features/...` or similar — likely shallower than Android's `app/src/main/java/com/...` but verify rather than assume:
```bash
find /home/alaz/dev/mobile/NEXUS -iname "*.dart" -exec realpath --relative-to=/home/alaz/dev/mobile/NEXUS {} \; | awk -F/ '{print NF}' | sort -rn | head -3
```
If any result exceeds 12 (today's depth limit), that's a genuine additional finding — report it, don't silently bump the constant without understanding why (unlike yesterday's Android case, which had clear justification).

- [ ] **Step 5:** If a fix was needed (extension and/or depth), rebuild, reinstall using the established unlink-first procedure, restart `aiosd`, and re-verify:
```bash
cd /home/alaz/dev/core/R-AI-OS
cargo test --workspace 2>&1 | grep -E "^test result:|FAILED"
cargo build --release --workspace
rm ~/.local/bin/raios && cp target/release/raios ~/.local/bin/raios
rm ~/.cargo/bin/raios && cp target/release/raios ~/.cargo/bin/raios
rm ~/.cargo/bin/aiosd && cp target/release/aiosd ~/.cargo/bin/aiosd
rm ~/.local/bin/aiosd && cp target/release/aiosd ~/.local/bin/aiosd
systemctl --user restart aiosd
```

## Part 5: Report

Whatever you find — MCP fully working, or new bugs discovered and fixed, or Flutter clean with no changes needed — commit any code changes (conventional-commit message, same style as `d5c7f38`/`238ec9a`), then:

```bash
raios handoff --to claude-kaira --status <success|failed|blocker> -p R-AI-OS --msg "<verbatim: tool_pin resolution outcome, grep_search MCP verification result, semantic_search MCP verification result + its actual timing and whether it's daemon-delegated or not, Flutter/Dart finding (bug found+fixed, or clean), final test count>"
```

Do not merge/push without that handoff — if you made code changes, work in a new isolated worktree per the usual convention; if this task turns out to be verification-only with zero code changes, no worktree/branch/merge is needed at all, just report directly.
