# raios-tray

Canonical source of truth for the R-AI-OS tray.

Cross-platform system tray for R-AI-OS. The app talks to the local `aiosd` HTTP API, shows daemon health and recent projects, can launch supported agents inside a platform-appropriate terminal, and now includes an in-app settings panel for `aiosd` tuning.

## Supported Platforms

- Linux desktop sessions with a working StatusNotifier host, including Budgie,
  GNOME with an indicator host, and Plasma
- macOS 13+ with a logged-in GUI session
- Windows 10/11 with a logged-in desktop session

## Stack

- Python 3.10+
- PySide6 for tray UI and dialogs
- psutil for cross-platform process inspection
- Qt's standard system-tray API; no GNOME Shell extension or GTK event loop is required

## Files

- `raios-tray.py`: main application
- `raios-tray.desktop`: Linux portal identity installed with the user service
- `requirements.txt`: Python dependencies
- `raios-tray.service`: Linux systemd user service
- `raios-tray-macos.plist`: macOS LaunchAgent template
- `raios-tray-windows.ps1`: Windows startup helper
- `memory.md`: project memory log
- Tray settings update only tray-owned `config.toml` keys atomically. Existing
  sections such as `[bootstrap]` and `[factory]` are preserved:
  - Workspace scan root: `dev_ops_path`
  - Constitution / skills / vault paths
  - `daemon` worker switches and polling intervals
  - Lifecycle thresholds and startup indexing flags

## Data and Responsiveness

- Tasks are read from the authenticated `/api/tasks` endpoint and changed only
  through typed `/api/v1/control/command` requests. The tray never writes the
  legacy SQLite `tasks` cache.
- The menu reports the full pending-task count but renders at most 50 task cards
  at once to keep the desktop responsive.
- API polling and Git dirty checks run outside Qt's UI thread. Dirty state is
  recalculated on each refresh rather than retained by a stale cache.

## Install

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
python3 raios-tray.py
```

## Platform Notes

### Linux

- Uses the desktop's StatusNotifier/system-tray host. Budgie needs its
  StatusNotifier/Indicator applet enabled; GNOME needs an indicator host; Plasma
  needs its System Tray widget.
- Prefers `ptyxis`, then `gnome-terminal`, `konsole`, `xfce4-terminal`, and `x-terminal-emulator`
- `raios-tray.service` is owned by `graphical-session.target`, so it starts only
  after the desktop session is ready and stops before the display disappears.
  It also installs `raios-tray.desktop` to the user application directory before
  start, allowing Qt to register its portal identity.
  Install or refresh the user service with:

  ```bash
  install -Dm644 raios-tray.service ~/.config/systemd/user/raios-tray.service
  systemctl --user daemon-reload
  systemctl --user reenable --now raios-tray.service
  ```

### macOS

- Uses `Terminal.app` via `osascript`
- Copy `raios-tray-macos.plist` into `~/Library/LaunchAgents/` and adjust paths if needed

### Windows

- Uses PowerShell for new agent terminals
- `raios-tray-windows.ps1` can register a per-user startup shortcut

## Runtime Expectations

- `aiosd` must be reachable at `http://127.0.0.1:42071`
- `aiosd` settings are stored in `config.toml` under the platform config directory and are loaded at daemon startup
- Token files are read from the platform config directory:
  - Linux: `~/.config/raios/`
  - macOS: `~/Library/Application Support/raios/`
  - Windows: `%APPDATA%\\raios\\`
