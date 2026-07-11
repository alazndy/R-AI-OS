# VS Code Extension Toolchain Modernization + Version Bump

> **For agentic workers:** This is dependency-modernization + light bug-hunting, not new feature work. Bump devDependencies incrementally (one at a time, recompile between each) rather than all at once — if something breaks, you want to know which bump caused it, not have to bisect three simultaneous major-version jumps.

**Goal:** `vscode-extension/` has been sitting at devDependency versions from whenever it was first scaffolded (`typescript ^5.3.0`, `@types/node ^20.0.0`) while the extension itself shipped real functionality through v0.8.0 (Scheduler panel, cron/handoff commands, session-token consolidation — see `git log --oneline -- vscode-extension/`). `memory.md`'s "Current Focus" has flagged "VS Code extension package bump" as pending since at least 2026-07-08. Close it out, and since you'll already be in this code, do a light manual pass through the extension in a real VS Code window to catch anything broken (mirrors the TUI usability pass from 2026-07-08 — that one found 4 real bugs just from using the thing).

**Context:** Zero runtime `dependencies` (good — keep it that way, don't add any without a clear reason). Only three `devDependencies`: `@types/node ^20.0.0` (currently resolves 20.19.43, latest major is 26.1.1), `@types/vscode ^1.85.0` (matches `engines.vscode`), `typescript ^5.3.0` (currently resolves 5.9.3, latest is 7.0.2 — that's TWO major version jumps, 5→6→7, expect real breaking changes in strictness/behavior). `engines.vscode` pins the minimum VS Code API version extension users need — check what's actually reasonable now (VS Code auto-updates for most users; a 1.85.0 floor was set at scaffold time, not chosen deliberately).

## Part 1: Baseline

- [ ] `cd vscode-extension && npm run compile` — confirm it's currently clean (0 errors) before touching anything, so you have a true baseline.
- [ ] `git log --oneline -5 -- vscode-extension/` — skim the last few real changes so you know what's recently-touched and worth being extra careful around (currently: `65f2eae` docs bump, `9182c30` session-token consolidation, `3429312` Scheduler panel + cron/handoff commands).

## Part 2: Incremental devDependency bumps

For each of the three, bump one, `npm install`, `npm run compile`, fix any new TS errors that surface (don't silence them with `// @ts-ignore` — actually resolve them, or revert that one bump and note why in your handoff if it's not reasonably fixable), commit, move to the next.

- [ ] `typescript`: bump `^5.3.0` → a current `^7.x` in `package.json`, `npm install`, `npm run compile`. TS 6/7 tightened several checks (notably around `unknown` narrowing and module resolution defaults) — if `tsconfig.json`'s `moduleResolution`/`module` settings predate Node16/bundler-style resolution, you may need to update them too. Read `tsconfig.json` first.
- [ ] `@types/node`: bump `^20.0.0` → a current `^26.x`. This should be low-risk (type-only), but VS Code extensions run in an Electron/Node hybrid host — if anything in `src/` uses a Node API whose types changed shape, `npm run compile` will catch it.
- [ ] `@types/vscode`: only bump this together with `engines.vscode` (they must stay in lockstep — bumping the types without bumping the engine floor makes the compiled extension claim compatibility with an API surface older VS Code installs don't have). Check the current VS Code stable version and pick a reasonable, not-bleeding-edge floor (e.g. something ~1 year old, not literally latest-stable) — err conservative, this extension should keep working for users who don't auto-update aggressively.

## Part 3: Light bug-hunting pass (you're already in here — use it)

Launch the Extension Development Host (`F5` in VS Code, or `code --extensionDevelopmentPath=.` if working headless) against a real registered raios project and exercise:
- [ ] Sidebar webview loads without a blank/error state.
- [ ] Status bar item reflects real daemon state (connected/disconnected — try it with `aiosd` stopped, confirm it says so rather than showing stale "connected").
- [ ] At least 2-3 of the 14 contributed commands (`package.json`'s `contributes.commands`) actually do something correct when invoked from the command palette — pick ones you can verify against a known-good CLI equivalent (e.g. whatever command surfaces `raios health` or `raios git status` equivalent data, cross-check against the real CLI output for the same project).
- [ ] The Scheduler panel / cron+handoff commands (newest addition, `3429312`) — these are the least-battle-tested part of the extension, prioritize actually clicking through them over older, more-exercised commands.
- [ ] `DaemonClient.ts`/`DaemonManager.ts`/`TokenBridge.ts` (the IPC layer) — if you find anything, note whether it's the same TCP/AUTH pattern used elsewhere this session (`127.0.0.1:42069`, `.session_token`) or something else; flag any divergence.

Fix anything real you find. Don't go hunting for hypothetical issues — only report/fix what you actually observe breaking or behaving wrong.

## Part 4: Version bump + repackage

- [ ] Bump `vscode-extension/package.json`'s `"version"` — pick patch/minor based on what Part 2/3 actually changed (pure toolchain bump with no behavior change = patch; any real bug fix from Part 3 = at least minor).
- [ ] Run `./install.sh` (already handles compile → package → uninstall-old → install-new → verify, see the script itself) and confirm the printed final line shows the new version installed.
- [ ] Delete the now-stale `raios-0.8.0.vsix` from the repo (it's committed per README's documented "no Node toolchain needed" install path) and commit the new `raios-<version>.vsix` in its place — check `.gitignore` doesn't already exclude `.vsix` (it currently commits these deliberately, per README line 290).
- [ ] Update README.md: the version badge (line ~32), the `## VS Code Extension (vX.Y.Z)` header (line ~252), and the `code --install-extension vscode-extension/raios-X.Y.Z.vsix --force` example command (line ~293) — all three currently say `0.8.0`, grep for `0.8.0` in README.md to make sure you catch all occurrences including any you find beyond these three.

## Report

```bash
raios handoff --to claude-kaira --status <success|failed|blocker> -p R-AI-OS --msg "<verbatim: old->new version for each of the 3 devDeps + engines.vscode, any compile errors hit and how resolved, bugs found+fixed during the manual pass with file:line, final extension version, vsix installed and verified>"
```
Work in a new isolated worktree. `npm run compile` must be clean and the final `.vsix` must actually install and show the right version before you report success. Do not merge/push without that handoff.
