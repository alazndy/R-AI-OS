# Agent matrix: Claude Code, Codex, OpenCode

What "works with an agent" means for the workspace, and how each cell is checked. Automated cells run in `cargo test -p rwk-core --test agent_matrix` with a script that imitates the `raios run` and `raios resume` argument shapes, so no model, account or network is used. Manual cells need the real binaries and are run by hand on the reference machine; record the date and versions when you do.

## Automated (every `cargo test`)

| Cell | Claude | Codex | OpenCode | Antigravity (`agy`) |
|---|---|---|---|---|
| `raios run <agent>` argument | yes | yes | yes | yes |
| Daemon identity (`*_kaira`, underscore form) | `claude_kaira` | `codex_kaira` | `opencode_kaira` | `antigravity_kaira` |
| Starts in the project root with `TERM=xterm-256color` | yes | yes | yes | yes |
| Name and prompt reach the wrapper as separate arguments (`--name N`, then the prompt) | yes | yes | yes | yes |
| No name argument when unnamed | yes | yes | yes | yes |
| Input reaches the child, resize reaches the PTY, exit is noticed | yes | yes | yes | yes |
| `raios resume <agent:id>` for a valid key | yes | yes | yes | not offered (no native continuation) |
| A key of another agent is refused before anything starts | yes | yes | yes | n/a |

The daemon identity is the underscore form the control plane stores and `raios run` derives (`canonical_agent_identity`). The `raios handoff --to` CLI option spells the same identities with a hyphen (`claude-kaira`); the workspace sends the underscore form through `CreateHandoff`.

## Manual (per release, real binaries)

Run in a scratch project. A tick means observed, not assumed.

| Cell | Claude | Codex | OpenCode |
|---|---|---|---|
| Starts through the workspace (`Ctrl+Shift+A`), prompt visible | | | |
| Colours, cursor shape, alternate screen, resize look right | | | |
| Paste (single and multiline preview), mouse reporting | | | |
| Unicode input and IME pre-edit | | | |
| Stop (`Ctrl+Shift` stop command): wrapper finalises within 5 s | | | |
| Close and reopen the project: an offer appears and takes the conversation back | | | |
| Handoff to another agent queues and the receipt reaches Pending | | | |
| Receiver started from the composer consumes the handoff (receipt Consumed) | | | |

## Observed on the reference machine, 2026-10-24

| Item | Result |
|---|---|
| Installed versions | Claude Code 2.1.310, Codex CLI 0.164.0, OpenCode 2.1.4 |
| `raios run <agent> --version` through the wrapper | all three start under the wrapper with identities `claude_kaira`, `codex_kaira`, `opencode_kaira` and exit 0 |
| `raios resume --list --json` | Claude conversations are `resumable: true`; the Codex and OpenCode rows listed were `resumable: false`, so the workspace offers a resume only for Claude on this machine until those agents' conversations become resumable |
| `raios resume <claude key> --dry-run` | prints the continuation plan (`--resume ...`) without launching |
| Interactive cells above | not run in this pass; they need a person at the keyboard |

Note: the `--version` runs created wrapper session records in the local `workspace.db`; they are harmless but visible in `raios resume --list`.
