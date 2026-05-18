# R-AI-OS VS Code Extension

Integrates the R-AI-OS AI OS Kernel (`aiosd` daemon) directly into VS Code.

## Features

- **Status Bar** — Real-time project health score (compliance grade, git status)
- **Command Palette** — `Ctrl+Shift+P` → `R-AI-OS:` commands
- **Diff Inbox** — Review and approve/reject agent-generated changes
- **Jump to Code** — TUI `[o]` key opens files directly in VS Code

## Requirements

- R-AI-OS v1.3.0+ installed (`raios` in PATH)
- `aiosd` daemon running (optional — status bar degrades gracefully)

## Commands

| Command | Description |
|---------|-------------|
| `R-AI-OS: Run Health Check` | Run health analysis on current project |
| `R-AI-OS: Commit & Push` | Intelligent commit with optional message |
| `R-AI-OS: Dispatch Task to Agent...` | Route task to best AI agent |
| `R-AI-OS: Re-index Cortex` | Rebuild semantic search index |
| `R-AI-OS: Security Scan` | OWASP security scan on current project |

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `raios.daemonPort` | `42069` | aiosd daemon TCP port |
| `raios.pollInterval` | `30` | Status bar refresh interval (seconds) |

## Connection

The extension connects to `aiosd` via TCP on `127.0.0.1:42069` using the token from `%APPDATA%\raios\.ipc_token`. If the daemon is not running, the extension degrades gracefully and retries every 5 seconds.
