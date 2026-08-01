# `raios bootstrap` Safety & Genericization — Design

## Context

This is sub-project **A** of a three-part market-readiness initiative ("generalize raios from a
personal tool to something a stranger can install"), decomposed during brainstorming on
2026-08-01 because the three parts are independent enough to need separate spec → plan →
implementation cycles:

- **A (this spec):** `raios bootstrap` safety + personal-data-leak cleanup.
- **B (next):** interactive TUI onboarding wizard that generates a new user's own
  `AGENT_CONSTITUTION.md` via Q&A.
- **C (after B):** multi-user / shared-server support (auth, per-user DB isolation, hub network
  exposure) — the largest and most security-sensitive piece, deliberately last.

Target audience for all three: public strangers who find R-AI-OS on GitHub and install it
themselves, not just known beta users. AGPL-3.0 licensing and OSS-vs-commercial positioning are
explicitly out of scope here — deferred until after generalization.

## Problem

A source-level audit (2026-08-01) found that `raios bootstrap`
(`crates/raios-surface-cli/src/cli/new.rs`, function `cmd_bootstrap`) is not a general raios
feature — it is Göktuğ's personal environment setup script, hardcoded into the shipped binary,
and it runs **entirely without confirmation**:

1. Installs 4 hardcoded global npm packages (`sigmap`, `ctx7`, `vercel`, `firebase-tools`) if not
   already present.
2. Adds two hardcoded third-party GitHub URLs as Claude Code plugin marketplaces
   (`josstei/maestro-orchestrate`, `affaan-m/everything-claude-code`) and installs plugins from
   them — silently granting those marketplaces' plugins tool access in the user's Claude Code
   install.
3. Clones (or pulls) `affaan-m/everything-claude-code` into a temp dir and copies its `rules/`
   content into `~/.claude/rules`, `~/.antigravity/rules`, and creates `~/.config/opencode` —
   injecting third-party-authored, unverified content directly into the global rule directories
   of three different AI agent CLIs.
4. If `~/Documents/Obsidian Vaults/Vault101/MASTER.md` doesn't exist, writes a hardcoded
   `DEFAULT_MASTER_MD` constant that begins `"You are Goktug's personal assistant"` — a literal
   personal-identity leak into any stranger's filesystem.
5. Force-enables 4 more plugins from the official Claude Code marketplace.

None of steps 1–5 ask for confirmation, none can be inspected before they run, and step 3 pulls
executable-adjacent content (agent rule files, which agents treat as instructions) from two
individuals' personal repositories with no integrity check — a supply-chain risk if either repo
is ever compromised or renamed. This is unacceptable for a binary a stranger downloads and runs.

Additionally: `crates/raios-runtime/src/cortex/mod.rs` has a stale doc-comment example path
(`Dev_Ops_New`, the pre-rename workspace folder name) — harmless but out of date, fixed in
passing since it's directly adjacent to this cleanup.

## Goals

- `raios bootstrap` never touches the network, the filesystem outside its own config read, or any
  other program's config, without the user first seeing an exact plan and confirming it.
- The specific tools/marketplaces/repos/plugins bootstrap acts on are **user data, not code** —
  moved out of the Rust source into `~/.config/raios/config.toml`, defaulting to empty.
- No personal identity string ("Goktug", `Vault101`, or equivalent) ships in the binary.
- Göktuğ's own current bootstrap behavior is preserved via a one-time config migration, not lost.

## Non-goals

- Redesigning what bootstrap *can* do (still: npm tools, Claude marketplaces/plugins, rule-sync
  git repos, plugin enables) — only how those actions are sourced and gated.
- The TUI setup wizard, `auto_detect()` heuristics, or `AGENT_CONSTITUTION.md` generation — that's
  sub-project B.
- Any multi-user/auth concern — that's sub-project C.

## Config schema

New `[bootstrap]` section in `~/.config/raios/config.toml` (all fields optional, all default
empty — an unconfigured install is a safe no-op):

```toml
[bootstrap]
global_npm_tools = []                # e.g. ["sigmap", "ctx7"]
enable_claude_plugins = []           # plugin names to enable from the official marketplace

[[bootstrap.claude_marketplaces]]
url = "https://github.com/user/repo.git"
plugins = ["plugin-name@marketplace-name"]

[[bootstrap.rule_sync_repos]]
git_url = "https://github.com/user/rules-repo.git"
targets = ["~/.claude/rules", "~/.antigravity/rules"]   # user-defined, not hardcoded to two paths
```

`targets` is an arbitrary-length list (today's code has exactly two hardcoded targets plus a third
empty-dir-only path for opencode); each target directory is created via `create_dir_all` if
missing, same as today, before the repo's `rules/` contents are copied into it.

`Config` (`crates/raios-core/src/config.rs`) gains a `#[serde(default)] pub bootstrap:
BootstrapConfig` field following the existing `FactoryConfig` pattern (data-only struct, defaults
to inert).

## Confirmation & dry-run flow

1. Load `config.toml`. If `bootstrap` is entirely empty, print
   `Nothing configured — see [bootstrap] in ~/.config/raios/config.toml` and exit 0. No I/O beyond
   the config read.
2. Otherwise, build a full **plan** before executing anything: a `Vec<BootstrapAction>` describing
   every concrete action in plain language (`Install npm package "sigmap"`,
   `Add marketplace https://... and install plugin "x@y"`,
   `Clone https://... and sync rules into ~/.claude/rules, ~/.antigravity/rules`,
   `Enable official plugin "z"`). Plan-building is a pure function of `BootstrapConfig` — no
   process calls, no filesystem writes — so it's unit-testable without touching the network.
3. Print the numbered plan.
4. Prompt `Proceed with N actions? [y/N]` — **default is No** (secure-by-default; matches the
   constitution's Insecure-Design rule). `--yes`/`-y` skips the prompt but the plan is still
   printed first, so scripted runs stay auditable in logs.
5. `--dry-run` prints the plan and exits 0 without asking or executing anything, regardless of
   `--yes` — lets a new user inspect exactly what bootstrap would do before ever risking it.

## Execution & error handling

- Each planned action executes independently (best-effort): one failure doesn't abort the rest.
- Every action that shells out checks the binary is on `PATH` first; if missing, it's recorded as
  `skipped: "claude" not found on PATH`, not silently swallowed via `let _ = ...status()` as
  today.
- A final summary lists every action as `ok` / `skipped: <reason>` / `failed: <reason>`.
- Exit code: `0` if nothing failed (`skipped` doesn't count), `1` if at least one action failed.

## `DEFAULT_MASTER_MD` / Vault101 removal

The current `cmd_bootstrap`'s step 5 ("Final Touches & Activations") does two unrelated things in
one block: (a) writes `~/Documents/Obsidian Vaults/Vault101/MASTER.md` with the hardcoded
`DEFAULT_MASTER_MD` template if it doesn't exist, and (b) enables 4 official-marketplace plugins.
Only (a) is removed here — (b) becomes the `enable_claude_plugins` action in the new plan, same as
any other configured action. Specifically, (a) is deleted because:

- It duplicates work that belongs to the sub-project B onboarding wizard.
- Its path is a second, independent hardcoded guess at the constitution location, inconsistent
  with `Config.master_md_path` which already exists for exactly this purpose.
- Its content is the one unambiguous personal-identity leak found in this audit.

`DEFAULT_MASTER_MD` and the associated write-if-missing logic are removed from `new.rs`, not
replaced or relocated — no bootstrap action writes a constitution file anymore.

## Migration for existing behavior (Göktuğ)

Because the hardcoded list disappears, a one-time migration step (part of this same change, not a
separate task) populates `~/.config/raios/config.toml`'s new `[bootstrap]` section with the
*current* hardcoded values (the same 4 npm tools, 2 marketplaces + their plugins, the
`everything-claude-code` rule-sync repo, the 4 official-marketplace plugin names) so that running
`raios bootstrap` post-change reproduces the exact same plan as before — just visible and
confirmed instead of silent.

## Testing

- Config parsing: empty / partially-filled / fully-filled `[bootstrap]` all produce the expected
  plan via `build_plan()`, with zero process/filesystem calls (unit tests, following the existing
  "no `/home/alaz` leaks" test pattern in `constitution.rs`).
- `build_plan` (pure) is tested separately from `execute()` (I/O), matching the existing
  plan/execute split style already used elsewhere in the CLI.
- A test asserting `--dry-run` and the empty-config no-op path never invoke `Command::new` or
  `fs::write` (can be verified structurally, since `build_plan` takes no I/O capability).
- A regression test asserting no code path writes to a path containing `Vault101` or a string
  containing `Goktug`, mirroring the existing `assert!(!content.contains("/home/alaz"))` pattern.

## Out of scope / deferred

- Sub-project B: TUI onboarding wizard, `AGENT_CONSTITUTION.md` Q&A generation.
- Sub-project C: multi-user/shared-server auth and DB isolation.
- Any integrity/checksum verification of the git-cloned rule-sync repos beyond what git itself
  provides (flagged as a real supply-chain gap in the Problem section, but fixing it — e.g. commit
  pinning, signature verification — is a larger piece of work than this cleanup; worth a follow-up
  spec if rule-sync stays in scope after B).
