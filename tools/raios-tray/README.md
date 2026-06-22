# raios-tray

Cross-platform system tray for R-AI-OS. The app talks to the local `aiosd` HTTP API, shows daemon health and recent projects, can launch supported agents inside a platform-appropriate terminal, and now includes an in-app settings panel for `aiosd` tuning.

## Supported Platforms

- Linux desktop sessions with a working system tray / StatusNotifier host
- macOS 13+ with a logged-in GUI session
- Windows 10/11 with a logged-in desktop session

## Stack

- Python 3.10+
- PySide6 for tray UI and dialogs
- psutil for cross-platform process inspection

## Files

- `raios-tray.py`: main application
- `requirements.txt`: Python dependencies
- `raios-tray.service`: Linux systemd user service
- `raios-tray-macos.plist`: macOS LaunchAgent template
- `raios-tray-windows.ps1`: Windows startup helper
- `memory.md`: project memory log
- Tray settings can edit the platform `config.toml` directly:
  - Workspace scan root: `dev_ops_path`
  - Constitution / skills / vault paths
  - `daemon` worker switches and polling intervals
  - Lifecycle thresholds and startup indexing flags

## Install

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
python3 raios-tray.py
```

## Platform Notes

### Linux

- Prefers `ptyxis`, then `gnome-terminal`, `konsole`, `xfce4-terminal`, and `x-terminal-emulator`
- `raios-tray.service` is a user service and expects a graphical login session

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
