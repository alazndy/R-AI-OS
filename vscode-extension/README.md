# R-AI-OS VS Code Extension

Integrates the R-AI-OS AI OS Kernel (`aiosd` daemon) directly into VS Code.

## Features

- **Control Panel (Sidebar)** — Live webview with Git status, Plans, Tasks (with write-back toggling), and active Swarm tasks (with one-click approve). Auto-starts the daemon if it isn't running, with quick `cargo build` / `cargo test` actions.
- **Status Bar** — Real-time project health score (compliance grade, git dirty state). Click to run a full health check.
- **Refactor Panel** — Background scan on every save; flags oversized/risky files in a dedicated Explorer tree view, gutter decorations, and a status bar item. Thresholds are configurable per file extension.
- **Security Diagnostics** — OWASP scan results surfaced as inline editor diagnostics + file decorations, debounced on save.
- **Diff Inbox** — Review and approve/reject agent-generated changes via VS Code's native diff viewer.
- **Jump to Code** — TUI `[o]` key opens files directly in VS Code.
- **Command Palette** — `Ctrl+Shift+P` → `R-AI-OS:` commands (see below).

## Requirements

- R-AI-OS v1.3.0+ installed (`raios` in PATH, or set `raios.pathToBinary`)
- `aiosd` daemon running (auto-started by the extension if missing; the status bar and sidebar degrade gracefully if it's unavailable)

## Commands

| Command | Description |
|---------|-------------|
| `R-AI-OS: Run Health Check` | Run health analysis on current project |
| `R-AI-OS: Commit & Push (Intelligent)` | Intelligent commit with optional message |
| `R-AI-OS: Dispatch Task to Agent...` | Route task to best AI agent |
| `R-AI-OS: Re-index Cortex` | Rebuild semantic search index |
| `R-AI-OS: Security Scan` | OWASP security scan on current project (or right-clicked folder) |
| `R-AI-OS: Scan File for Security Issues` | OWASP scan scoped to the active/right-clicked file |
| `R-AI-OS: License Compliance Check` | Scan dependencies for copyleft/unknown licenses |
| `R-AI-OS: Audit Web Page (Lighthouse)...` | Run a Lighthouse audit against a URL |
| `R-AI-OS: Open Project Memory` | Open `memory.md` for the current project |
| `R-AI-OS: Show Refactor Panel` | Focus the Refactor tree view in the Explorer |

Keybindings: `Ctrl+Shift+R S` (Security Scan), `Ctrl+Shift+R H` (Health Check), `Ctrl+Shift+R F` (Scan Current File) — `Cmd` on macOS.

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `raios.daemonPort` | `42069` | aiosd TCP IPC port (status bar, diff inbox, jump-to-code) |
| `raios.httpPort` | `42071` | aiosd HTTP API port (sidebar webview data) |
| `raios.pathToBinary` | `raios` | Path to the `raios` executable. Defaults to resolving from PATH / common install locations. |
| `raios.pollInterval` | `30` | Status bar refresh interval (seconds) |
| `raios.diagnosticsEnabled` | `true` | Run security diagnostics automatically on file save |
| `raios.diagnosticsDebounceMs` | `800` | Debounce delay (ms) before triggering a security scan on save |
| `raios.refactor.highLineThreshold` | `500` | Line count flagged HIGH for refactor |
| `raios.refactor.mediumLineThreshold` | `300` | Line count flagged MEDIUM for refactor |
| `raios.refactor.highUnwrapThreshold` | `10` | Risky-pattern count (`unwrap`/`!!`/`as any`) for HIGH severity |
| `raios.refactor.mediumUnwrapThreshold` | `5` | Risky-pattern count for MEDIUM severity |
| `raios.refactor.highNestingThreshold` | `10` | Max nesting depth (leading spaces ÷ 4) for HIGH severity |
| `raios.refactor.mediumNestingThreshold` | `8` | Max nesting depth for MEDIUM severity |
| `raios.refactor.extensions` | `{}` | Per-extension threshold overrides, e.g. `{ "rs": { "highLines": 600 } }` |

## Connection

The extension talks to `aiosd` over two channels:

- **TCP IPC** (`127.0.0.1:42069`) — used by the status bar, diff inbox, and jump-to-code. Authenticated with the token at `~/.config/raios/.ipc_token` (or `%APPDATA%\raios\.ipc_token` on Windows).
- **HTTP API** (`127.0.0.1:42071`) — used by the sidebar webview (Git status, Plans, Tasks, Swarm). Authenticated with a Bearer token read from `~/.config/raios/.session_token` (or the Windows equivalent), cached in VS Code's Secrets Storage as a backup.

If the daemon is not running, the extension attempts to spawn it automatically; otherwise it degrades gracefully and retries.

## Development

Open this folder in VS Code and press **F5** to launch an Extension Development Host with the extension loaded (`.vscode/launch.json` runs the `watch` build task first). To build and install a packaged copy instead:

```bash
./install.sh
```
