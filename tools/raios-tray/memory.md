# Project Memory: raios-tray

## Context
- **Status**: In Development
- **Stack**: Python 3 + PySide6 + psutil
- **Last Milestone**: Cross-platform rewrite plan prepared for Linux, macOS, and Windows tray support

## Active Objectives
- [x] Linux-only AppIndicator tray prototype
- [x] Add tray-side editor for `aiosd` config paths and worker intervals
- [ ] Validate the new PySide6 tray on Linux, macOS, and Windows
- [ ] Install platform-specific startup integration files

## Technical Decisions
- **Architecture**: Single-file Python tray with Qt-based UI, threaded polling worker, and cross-platform terminal launch helpers
- **Auth**: Bearer token from the platform-specific raios config directory
- **Polling**: Every 15 seconds via `QTimer`
- **Python**: Use `python3` / `python` from the active environment instead of Linux-only GI bindings
- **Config Editing**: Tray writes platform `config.toml` directly and offers optional `aiosd` restart after saving

## Important Links & Paths
- **Main Entry**: `./raios-tray.py`
- **Service**: `~/.config/systemd/user/raios-tray.service`
- **API**: `http://127.0.0.1:42069` — endpoints: /api/health, /api/projects, /api/usage

## Current Focus
- Validate settings dialog and daemon restart flow on live platforms

## Change Log & Agent Trail
- [2026-06-13] Claude Kaira: Initial implementation — tray with daemon status, CPU/RAM (aiosd+raios), project list, verify-chain status; systemd user service created
- [2026-06-18] Codex Kaira: Reworked the tray design toward a PySide6-based cross-platform implementation and added Linux/macOS/Windows startup assets in the project workspace draft
- [2026-06-18] Codex Kaira: Added tray-side `aiosd` settings editor for workspace paths, daemon worker switches, intervals, lifecycle thresholds, and config directory access
