# Project Memory: raios-tray

## Context
- **Status**: In Development
- **Stack**: Python 3 + PySide6 + psutil
- **Last Milestone**: Cross-platform rewrite plan prepared for Linux, macOS, and Windows tray support

## Active Objectives
- [x] Linux-only AppIndicator tray prototype
- [x] Add tray-side editor for `aiosd` config paths and worker intervals
- [x] Project Manager — add/edit/remove/pin projects, VSCode + agent launch
- [x] Git dirty status indicators per project, dirty count warning in tray
- [ ] Validate the new PySide6 tray on Linux, macOS, and Windows
- [ ] Install platform-specific startup integration files

## Technical Decisions
- **Architecture**: Single-file Python tray with Qt-based UI, sync polling via QTimer, persistent QMenu instance (prevents GC on Wayland), zenity/xdg-open fallbacks for Wayland file dialogs
- **Auth**: Bearer token from the platform-specific raios config directory
- **Polling**: Every 15 seconds via `QTimer`
- **Python**: Use `python3` / `python` from the active environment instead of Linux-only GI bindings
- **Config Editing**: Tray writes platform `config.toml` directly and offers optional `aiosd` restart after saving

## Important Links & Paths
- **Main Entry**: `./raios-tray.py`
- **Service**: `~/.config/systemd/user/raios-tray.service`
- **API**: `http://127.0.0.1:42069 (health) / 42071 (agents)` — endpoints: /api/health, /api/projects, /api/usage

## Current Focus
- Validate settings dialog and daemon restart flow on live platforms

## Change Log & Agent Trail
- [2026-06-25] Codex Kaira: Promoted this directory to the canonical raios-tray source of truth. External copies must launch or mirror from here instead of diverging.
- [2026-06-13] Claude Kaira: Initial implementation — tray with daemon status, CPU/RAM (aiosd+raios), project list, verify-chain status; systemd user service created
- [2026-06-18] Codex Kaira: Reworked the tray design toward a PySide6-based cross-platform implementation and added Linux/macOS/Windows startup assets in the project workspace draft
- [2026-06-18] Codex Kaira: Added tray-side `aiosd` settings editor for workspace paths, daemon worker switches, intervals, lifecycle thresholds, and config directory access
- [2026-06-22] Claude Kaira: Fixed QThread lifecycle crash (ABRT/SIGSEGV). Removed FetchWorker+QThread entirely — switched to sync fetch on main event loop. Service file fixed to use `.venv/bin/python` and `QT_QPA_PLATFORM=xcb` for Wayland compatibility. Tray now stable on Ubuntu 26.04 Wayland.
- [2026-06-22] Claude Kaira: Fixed menu items not responding — persistent `self.menu` instance attribute prevents PySide6 GC from collecting QMenu/actions on Wayland+xcb. Removed manual `menu.popup(QCursor.pos())` (broken on Wayland). Added `zenity` fallback for file/dir picker in settings. Added `xdg-open` fallback for Open Config Directory. Corrected API port to 42071 with auth token.
- [2026-06-25] Claude Kaira: Added Project Manager — new dialog for add/edit/remove/pin projects. Managed projects persist in tray-projects-config.json. Pinned projects shown at top of tray menu. Added VSCode launch option per project. Added "Manage Projects..." menu entry.
- [2026-06-25] Claude Kaira: Removed Gemini agent. Agents now nested under "Agents" submenu per project. Added git dirty status detection — dirty projects show ● indicator in menu and Manage dialog. Tray tooltip shows dirty count. Menu shows "● X dirty projects" warning line when daemon is online.
- [2026-06-25] Codex Kaira: Hardened project agent menus against PySide6 menu ownership/GC issues. Switched Manage Projects agent chooser to `QToolButton` with `InstantPopup` and retained submenu/action references so agent entries render reliably.
- [2026-06-25] Codex Kaira: Replaced pin emoji markers with themed logo icons in the tray/menu UI and cached git dirty checks by repo state + TTL to reduce menu lag during refreshes.
- [2026-06-25] Codex Kaira: Refactored Manage Projects into a two-column card grid with stacked action rows so the dialog stays within smaller screens instead of overflowing horizontally.
- [2026-06-25] Codex Kaira: Removed the legacy `ProjectsDialog` path and routed `All Projects` to `ProjectManagerDialog`, ensuring the two-column layout plus VSCode and agent actions appear consistently from every menu entry.
