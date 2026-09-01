# Project Memory: raios-tray

## Context
- **Status**: In Development
- **Stack**: Python 3 + PySide6 + psutil
- **Last Milestone**: Canonical control-plane task integration, lossless config saves, and non-blocking refresh deployed on Linux

## Active Objectives
- [x] Linux-only AppIndicator tray prototype
- [x] Add tray-side editor for `aiosd` config paths and worker intervals
- [x] Project Manager — add/edit/remove/pin projects, VSCode + agent launch
- [x] Git dirty status indicators per project, dirty count warning in tray
- [x] Native Wayland-compatible Qt tray with a portal desktop identity
- [x] Lossless config editing, canonical task API, and non-blocking refresh
- [x] Light/dark mode adaptive dialogs (Fusion palette, gsettings detection)
- [ ] Validate on macOS and Windows
- [ ] Install platform-specific startup integration files

## Technical Decisions
- **Architecture**: Single-file Python tray with Qt-based UI, persistent menus, a bounded background refresh executor, and zenity/xdg-open fallbacks for Wayland file dialogs
- **Auth**: Bearer token from the platform-specific raios config directory
- **Polling**: Every 15 seconds via QTimer; API and Git work execute outside the UI thread
- **Python**: Use `python3` / `python` from the active environment instead of Linux-only GI bindings
- **Config Editing**: Tray atomically patches owned config keys and preserves unknown TOML sections
- **Tasks**: Tray uses authenticated HTTP control-plane APIs; it never reads or writes the legacy `tasks` cache

## Important Links & Paths
- **Main Entry**: `./raios-tray.py`
- **Service**: `~/.config/systemd/user/raios-tray.service`
- **API**: `http://127.0.0.1:42071` — endpoints: /api/health, /api/projects, /api/tasks, /api/v1/control/command

## Current Focus
- Validate the desktop-independent Qt tray on Budgie, GNOME, Plasma, macOS, and Windows.

## Change Log & Agent Trail
- [2026-08-26] Codex Kaira: Fixed the live tray controller ownership bug. `RaiosTray` is now parented to `QApplication`; previously its timers could be garbage-collected after initial DBus registration, leaving the active indicator menu permanently on “Loading…”. Live StatusNotifier verification now shows daemon, projects, dirty state, memory, and canonical task counts.
- [2026-08-26] Codex Kaira: Fixed tray root defects: atomically preserve unknown config sections; migrate task reads and mutations to authenticated canonical control-plane APIs; render a bounded task window; move API/Git refresh work off the Qt UI thread; remove stale dirty caching; add the portal desktop identity and deploy the refreshed user service. Verified live service startup without portal warnings, 7 Python tests, control-plane contract test, security scan A/100, and clean dependency audit.
- [2026-07-27] Codex Kaira: Replaced the GTK/AppIndicator event-loop bridge with PySide6's standard tray API, added desktop-session detection with Budgie-specific host guidance, and covered the portable detector with unit tests. The tray now has no GNOME Shell extension or GTK dependency.
- [2026-06-25] Codex Kaira: Promoted this directory to the canonical raios-tray source of truth. External copies must launch or mirror from here instead of diverging.
- [2026-06-13] Claude Kaira: Initial implementation — tray with daemon status, CPU/RAM (aiosd+raios), project list, verify-chain status; systemd user service created
- [2026-06-18] Codex Kaira: Reworked the tray design toward a PySide6-based cross-platform implementation and added Linux/macOS/Windows startup assets in the project workspace draft
- [2026-06-18] Codex Kaira: Added tray-side `aiosd` settings editor for workspace paths, daemon worker switches, intervals, lifecycle thresholds, and config directory access
- [2026-08-19] Codex Kaira: Fixed the Linux tray's `Start aiosd`/`Stop aiosd` actions to delegate to the systemd user service instead of directly spawning or killing a daemon process, keeping daemon ownership canonical and making the action reliable after service restarts.
- [2026-06-22] Claude Kaira: Fixed QThread lifecycle crash (ABRT/SIGSEGV). Removed FetchWorker+QThread entirely — switched to sync fetch on main event loop. Service file fixed to use `.venv/bin/python` and `QT_QPA_PLATFORM=xcb` for Wayland compatibility. Tray now stable on Ubuntu 26.04 Wayland.
- [2026-06-22] Claude Kaira: Fixed menu items not responding — persistent `self.menu` instance attribute prevents PySide6 GC from collecting QMenu/actions on Wayland+xcb. Removed manual `menu.popup(QCursor.pos())` (broken on Wayland). Added `zenity` fallback for file/dir picker in settings. Added `xdg-open` fallback for Open Config Directory. Corrected API port to 42071 with auth token.
- [2026-06-25] Claude Kaira: Added Project Manager — new dialog for add/edit/remove/pin projects. Managed projects persist in tray-projects-config.json. Pinned projects shown at top of tray menu. Added VSCode launch option per project. Added "Manage Projects..." menu entry.
- [2026-06-25] Claude Kaira: Removed Gemini agent. Agents now nested under "Agents" submenu per project. Added git dirty status detection — dirty projects show ● indicator in menu and Manage dialog. Tray tooltip shows dirty count. Menu shows "● X dirty projects" warning line when daemon is online.
- [2026-06-25] Codex Kaira: Hardened project agent menus against PySide6 menu ownership/GC issues. Switched Manage Projects agent chooser to `QToolButton` with `InstantPopup` and retained submenu/action references so agent entries render reliably.
- [2026-06-25] Codex Kaira: Replaced pin emoji markers with themed logo icons in the tray/menu UI and cached git dirty checks by repo state + TTL to reduce menu lag during refreshes.
- [2026-06-25] Codex Kaira: Refactored Manage Projects into a two-column card grid with stacked action rows so the dialog stays within smaller screens instead of overflowing horizontally.
- [2026-06-25] Codex Kaira: Removed the legacy `ProjectsDialog` path and routed `All Projects` to `ProjectManagerDialog`, ensuring the two-column layout plus VSCode and agent actions appear consistently from every menu entry.
- [2026-06-26] Claude Kaira: Fixed two recurring crash patterns. (1) Boot crash: added `ExecStartPre` display-wait loop (up to 30s) so service no longer ABRTs when XWayland isn't ready. (2) Session-end crash: added `PartOf=graphical-session.target` so systemd stops tray before X11 dies on logout. Also removed `ProjectsDialog` dead code (ghost type annotation + unreachable `_apply_state` block for the deleted class).
- [2026-06-26] Claude Kaira: Switched tray stack to native Wayland. (1) `QT_QPA_PLATFORM=xcb→wayland` in service file. (2) `QSystemTrayIcon` replaced with `AyatanaAppIndicator3 + Gtk.Menu`; GTK events pumped every 50ms via QTimer so both toolkits share the main thread. (3) All dialogs (ProjectManagerDialog, SettingsDialog) switched from `exec()` to `show()+raise_()+activateWindow()` for Wayland focus. (4) Full light/dark mode: `_is_dark_mode()` via gsettings, `_card_theme()` palette, Fusion style + `_apply_dark_palette()` in main().
- [2026-08-18] Codex Kaira: Fixed repeatable cold-boot Wayland ABRT by changing the systemd user unit's install owner from `default.target` to `graphical-session.target`. The unit already declared `After=`/`PartOf=graphical-session.target`, but `default.target` scheduled it at 11:25:16 while the graphical target became active only at 11:25:20, so Qt attempted `wl_display` four seconds too early. Added a standard-library lifecycle regression test and wired it into CI; updated Linux install instructions. Live unit reenabled under `graphical-session.target.wants`, active with zero restarts; full reboot verification remains for the final deployment pass.
