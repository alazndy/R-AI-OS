#!/usr/bin/env python3
"""R-AI-OS system tray for Linux, macOS, and Windows."""

from __future__ import annotations

import json
import os
import platform
import shlex
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterable

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10
    import tomli as tomllib

import psutil
from PySide6.QtCore import QObject, QTimer, Qt, QUrl
from PySide6.QtGui import QAction, QColor, QDesktopServices, QIcon, QPalette
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFormLayout,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

# GTK + AyatanaAppIndicator3 (native Wayland tray on GNOME)
_gi_path = "/usr/lib/python3/dist-packages"
if _gi_path not in sys.path:
    sys.path.insert(0, _gi_path)
os.environ.setdefault("GI_TYPELIB_PATH", "/usr/lib/x86_64-linux-gnu/girepository-1.0")

import gi  # noqa: E402
gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

try:
    gi.require_version("AyatanaAppIndicator3", "0.1")
    from gi.repository import AyatanaAppIndicator3 as AppIndicator3
except ValueError:
    gi.require_version("AppIndicator3", "0.1")
    from gi.repository import AppIndicator3

try:
    gi.require_version("Notify", "0.7")
    from gi.repository import Notify as _GtkNotify
    _GTK_NOTIFY = True
except ValueError:
    _GtkNotify = None
    _GTK_NOTIFY = False

API_BASE = "http://127.0.0.1:42071"
REFRESH_SECONDS = 15
MAX_PROJECTS = 10
APP_NAME = "R-AI-OS Tray"
DIRTY_CACHE_TTL_SECONDS = 90

CONFIG_TOP_LEVEL_KEYS = (
    "dev_ops_path",
    "master_md_path",
    "skills_path",
    "vault_projects_path",
    "system_name",
    "github_user",
)

DAEMON_BOOL_FIELDS = (
    ("startup_bm25_indexing", "Startup BM25 indexing"),
    ("startup_cortex_indexing", "Startup Cortex indexing"),
    ("enable_health_worker", "Enable health worker"),
    ("enable_sentinel_worker", "Enable sentinel worker"),
    ("enable_port_monitor", "Enable port monitor"),
)

DAEMON_INT_FIELDS = (
    ("health_interval_secs", "Health interval (sec)", 5, 86400),
    ("git_interval_secs", "Git interval (sec)", 5, 86400),
    ("sentinel_interval_secs", "Sentinel interval (sec)", 5, 86400),
    ("port_monitor_interval_secs", "Port monitor interval (sec)", 5, 86400),
    ("port_probe_timeout_ms", "Port probe timeout (ms)", 25, 10000),
    ("lifecycle_standby_days", "Standby after days", 1, 3650),
    ("lifecycle_archive_days", "Archive after days", 1, 3650),
    ("lifecycle_interval_secs", "Lifecycle interval (sec)", 60, 604800),
)


def _is_dark_mode() -> bool:
    for args in (
        ["gsettings", "get", "org.gnome.desktop.interface", "color-scheme"],
        ["gsettings", "get", "org.gnome.desktop.interface", "gtk-theme"],
    ):
        try:
            out = subprocess.run(args, capture_output=True, text=True, timeout=1).stdout.lower()
            if "dark" in out:
                return True
        except Exception:
            pass
    try:
        return QApplication.palette().color(QPalette.Window).lightness() < 128
    except Exception:
        return False


def _card_theme() -> dict[str, str]:
    if _is_dark_mode():
        return {
            "pinned_bg":  "#2e3140",
            "card_bg":    "#252836",
            "border":     "#3e4257",
            "muted":      "#8b8fa8",
            "hint":       "#6b6f8a",
            "dirty":      "#f87171",
            "api_tag":    "#6b6f8a",
        }
    return {
        "pinned_bg":  "#eef2ff",
        "card_bg":    "#ffffff",
        "border":     "#e5e7eb",
        "muted":      "#6b7280",
        "hint":       "#9ca3af",
        "dirty":      "#dc2626",
        "api_tag":    "#9ca3af",
    }


@dataclass(frozen=True)
class Agent:
    name: str
    commands: tuple[str, ...]


@dataclass
class TrayState:
    online: bool = False
    health: dict | None = None
    projects: list[dict] | None = None
    projects_from_cache: bool = False
    usage: dict | None = None
    aiosd_cpu: float = 0.0
    aiosd_ram_mb: float = 0.0
    error: str = ""
    dirty_projects: set[str] = field(default_factory=set)
    mem_items: list[dict] = field(default_factory=list)
    tasks: list[dict] = field(default_factory=list)


AGENTS: tuple[Agent, ...] = (
    Agent("Claude", ("claude", "/home/alaz/.local/bin/claude")),
    Agent("Codex", ("codex", "/home/alaz/.local/bin/codex")),
    Agent("AGY", ("agy", "/home/alaz/.local/bin/agy")),
    Agent("OpenCode", ("opencode", "/home/alaz/.opencode/bin/opencode")),
)


def detect_platform() -> str:
    return platform.system().lower()


def config_dir() -> Path:
    system = detect_platform()
    if system == "darwin":
        return Path.home() / "Library" / "Application Support" / "raios"
    if system == "windows":
        base = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
        return Path(base) / "raios"
    return Path.home() / ".config" / "raios"


def default_paths() -> dict[str, str]:
    home = Path.home()
    return {
        "dev_ops_path": str(home / "dev"),
        "master_md_path": str(home / "AGENT_CONSTITUTION.md"),
        "skills_path": str(home / ".codex" / "skills"),
        "vault_projects_path": "",
    }


def default_daemon_config() -> dict[str, int | bool]:
    windows = detect_platform() == "windows"
    return {
        "startup_bm25_indexing": False,
        "startup_cortex_indexing": False,
        "enable_health_worker": True,
        "health_interval_secs": 900 if windows else 300,
        "git_interval_secs": 300 if windows else 120,
        "enable_sentinel_worker": not windows,
        "sentinel_interval_secs": 300 if windows else 30,
        "enable_port_monitor": True,
        "port_monitor_interval_secs": 30 if windows else 10,
        "port_probe_timeout_ms": 75 if windows else 100,
        "lifecycle_standby_days": 14,
        "lifecycle_archive_days": 90,
        "lifecycle_interval_secs": 3600,
    }


def default_raios_config() -> dict:
    defaults = default_paths()
    defaults.update(
        {
            "system_name": "k-ai-ra",
            "github_user": "",
            "daemon": default_daemon_config(),
        }
    )
    return defaults


CONFIG_DIR = config_dir()
CONFIG_PATH = CONFIG_DIR / "config.toml"
RAIOS_DB_PATH = CONFIG_DIR / "workspace.db"
TOKEN_CANDIDATES = (
    CONFIG_DIR / ".session_token",
    CONFIG_DIR / ".ipc_token",
)
USAGE_PATH = CONFIG_DIR / "tray-usage.json"
CACHE_PATH = CONFIG_DIR / "tray-projects-cache.json"
PROJECTS_CONFIG_PATH = CONFIG_DIR / "tray-projects-config.json"
DIRTY_STATUS_CACHE: dict[str, tuple[float, bool, tuple[float, float]]] = {}

MEM_TYPE_COLORS_DARK = {
    "feedback":  "#f0a500",
    "project":   "#4caf50",
    "user":      "#60a5fa",
    "reference": "#c084fc",
}
MEM_TYPE_COLORS_LIGHT = {
    "feedback":  "#d97706",
    "project":   "#16a34a",
    "user":      "#2563eb",
    "reference": "#7c3aed",
}


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def read_token() -> str:
    for path in TOKEN_CANDIDATES:
        token = read_text(path)
        if token:
            return token
    return ""


def api_get(path: str, token: str):
    request = urllib.request.Request(
        API_BASE + path,
        headers={"Authorization": f"Bearer {token}"} if token else {},
    )
    try:
        with urllib.request.urlopen(request, timeout=4) as response:
            return json.loads(response.read())
    except Exception:
        return None


def load_json(path: Path, fallback):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return fallback


def save_json(path: Path, payload) -> None:
    try:
        ensure_parent(path)
        path.write_text(json.dumps(payload), encoding="utf-8")
    except OSError:
        pass


def load_usage() -> dict:
    return load_json(USAGE_PATH, {})


def save_usage(usage: dict) -> None:
    save_json(USAGE_PATH, usage)


def load_cache() -> list[dict]:
    return load_json(CACHE_PATH, [])


def save_cache(projects: list[dict]) -> None:
    save_json(CACHE_PATH, projects)


def load_projects_config() -> dict:
    return load_json(PROJECTS_CONFIG_PATH, {"projects": []})


def save_projects_config(config: dict) -> None:
    save_json(PROJECTS_CONFIG_PATH, config)


def load_mem_items(project_key: str | None = None, limit: int = 100) -> list[dict]:
    if not RAIOS_DB_PATH.exists():
        return []
    try:
        conn = sqlite3.connect(f"file:{RAIOS_DB_PATH}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        if project_key:
            rows = conn.execute(
                "SELECT * FROM mem_items WHERE project_key = ? ORDER BY updated_at DESC LIMIT ?",
                (project_key, limit),
            ).fetchall()
        else:
            rows = conn.execute(
                "SELECT * FROM mem_items ORDER BY updated_at DESC LIMIT ?",
                (limit,),
            ).fetchall()
        conn.close()
        return [dict(r) for r in rows]
    except Exception:
        return []


def load_tasks(limit: int = 50) -> list[dict]:
    if not RAIOS_DB_PATH.exists():
        return []
    try:
        conn = sqlite3.connect(f"file:{RAIOS_DB_PATH}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        rows = conn.execute(
            "SELECT * FROM tasks WHERE completed = 0 ORDER BY created_at DESC LIMIT ?",
            (limit,),
        ).fetchall()
        conn.close()
        return [dict(r) for r in rows]
    except Exception:
        return []


def add_task(text: str, project: str | None = None) -> bool:
    if not RAIOS_DB_PATH.exists():
        return False
    try:
        conn = sqlite3.connect(str(RAIOS_DB_PATH))
        conn.execute(
            "INSERT INTO tasks (text, completed, agent, project) VALUES (?, 0, ?, ?)",
            (text.strip(), "raios-tray", project or None),
        )
        conn.commit()
        conn.close()
        return True
    except Exception:
        return False


def complete_task(task_id: int) -> bool:
    if not RAIOS_DB_PATH.exists():
        return False
    try:
        conn = sqlite3.connect(str(RAIOS_DB_PATH))
        conn.execute("UPDATE tasks SET completed = 1 WHERE id = ?", (task_id,))
        conn.commit()
        conn.close()
        return True
    except Exception:
        return False


def delete_task(task_id: int) -> bool:
    if not RAIOS_DB_PATH.exists():
        return False
    try:
        conn = sqlite3.connect(str(RAIOS_DB_PATH))
        conn.execute("DELETE FROM tasks WHERE id = ?", (task_id,))
        conn.commit()
        conn.close()
        return True
    except Exception:
        return False


def get_pin_logo() -> QIcon:
    icon = QIcon.fromTheme("utilities-terminal")
    if icon.isNull():
        icon = QIcon.fromTheme("applications-system")
    return icon


def bump_usage(project_name: str) -> dict:
    usage = load_usage()
    usage[project_name] = usage.get(project_name, 0) + 1
    save_usage(usage)
    return usage


def sort_projects(projects: Iterable[dict], usage: dict) -> list[dict]:
    return sorted(
        projects,
        key=lambda project: usage.get(project.get("name", ""), 0),
        reverse=True,
    )


def find_existing_command(candidates: tuple[str, ...]) -> str | None:
    for candidate in candidates:
        expanded = os.path.expanduser(candidate)
        if os.path.isabs(expanded) and os.path.exists(expanded):
            return expanded
        located = shutil.which(candidate)
        if located:
            return located
    return None


def open_terminal(project_path: str, command: str) -> bool:
    system = detect_platform()
    quoted_path = shlex.quote(project_path)
    quoted_command = f"cd {quoted_path} && exec {command}"

    try:
        if system == "linux":
            terminal_candidates = (
                ("ptyxis", "-s", "--", "bash", "-lc", quoted_command),
                ("gnome-terminal", "--", "bash", "-lc", quoted_command),
                ("konsole", "-e", "bash", "-lc", quoted_command),
                ("xfce4-terminal", "--command", f"bash -lc {shlex.quote(quoted_command)}"),
                ("x-terminal-emulator", "-e", f"bash -lc {shlex.quote(quoted_command)}"),
            )
            for candidate in terminal_candidates:
                binary = shutil.which(candidate[0])
                if binary:
                    subprocess.Popen([binary, *candidate[1:]])
                    return True
            return False

        if system == "darwin":
            apple_script = (
                'tell application "Terminal"\n'
                "activate\n"
                f'do script "{quoted_command.replace(chr(34), chr(92) + chr(34))}"\n'
                "end tell\n"
            )
            subprocess.Popen(["osascript", "-e", apple_script])
            return True

        if system == "windows":
            subprocess.Popen(
                ["cmd", "/c", "start", "R-AI-OS", "powershell", "-NoExit", "-Command", command],
                cwd=project_path,
            )
            return True
    except OSError:
        return False

    return False


def launch_agent(project_path: str, agent: Agent, project_name: str) -> bool:
    command = find_existing_command(agent.commands)
    if not command:
        return False
    bump_usage(project_name)
    return open_terminal(project_path, shlex.quote(command))


def launch_vscode(project_path: str) -> bool:
    command = find_existing_command(("code",))
    if not command:
        return False
    try:
        subprocess.Popen([command, project_path])
        return True
    except OSError:
        return False


def check_git_dirty(project_path: str) -> bool:
    if not project_path:
        return False
    repo_path = Path(project_path)
    git_dir = repo_path / ".git"
    if not git_dir.exists():
        return False
    head_path = git_dir / "HEAD"
    index_path = git_dir / "index"
    try:
        signature = (
            head_path.stat().st_mtime if head_path.exists() else 0.0,
            index_path.stat().st_mtime if index_path.exists() else 0.0,
        )
    except OSError:
        signature = (0.0, 0.0)
    now = time.monotonic()
    cached = DIRTY_STATUS_CACHE.get(project_path)
    if cached:
        checked_at, cached_value, cached_signature = cached
        if cached_signature == signature and (now - checked_at) < DIRTY_CACHE_TTL_SECONDS:
            return cached_value
    try:
        result = subprocess.run(
            ["git", "-C", project_path, "status", "--porcelain"],
            capture_output=True, text=True, timeout=2,
        )
        is_dirty = bool(result.stdout.strip())
        DIRTY_STATUS_CACHE[project_path] = (now, is_dirty, signature)
        return is_dirty
    except (OSError, subprocess.TimeoutExpired):
        if cached:
            return cached[1]
        return False


def merge_projects(api_projects: list[dict], managed_config: dict) -> tuple[list[dict], dict[str, bool]]:
    managed = managed_config.get("projects", [])
    managed_names = {p["name"] for p in managed}
    managed_set = {p["name"]: p.get("pinned", False) for p in managed}
    pinned = [p for p in managed if p.get("pinned")]
    unpinned_managed = [p for p in managed if not p.get("pinned")]
    others = [p for p in api_projects if p.get("name") not in managed_names]
    pinned_display = []
    for p in pinned:
        api_match = next((ap for ap in api_projects if ap.get("name") == p["name"]), None)
        if api_match:
            pinned_display.append(api_match)
        else:
            pinned_display.append({"name": p["name"], "local_path": p["path"]})
    unpinned_display = []
    for p in unpinned_managed:
        api_match = next((ap for ap in api_projects if ap.get("name") == p["name"]), None)
        if api_match:
            unpinned_display.append(api_match)
        else:
            unpinned_display.append({"name": p["name"], "local_path": p["path"]})
    return pinned_display + unpinned_display + others, managed_set


def find_aiosd_executable() -> str | None:
    candidates = (
        "aiosd.exe",
        "aiosd",
        str(Path.home() / ".cargo" / "bin" / "aiosd.exe"),
        str(Path.home() / ".cargo" / "bin" / "aiosd"),
    )
    return find_existing_command(candidates)


def iter_aiosd_processes() -> Iterable[psutil.Process]:
    for process in psutil.process_iter(["name", "exe", "cmdline"]):
        try:
            names = {
                process.info.get("name") or "",
                os.path.basename(process.info.get("exe") or ""),
            }
            cmdline = " ".join(process.info.get("cmdline") or [])
            if "aiosd" in " ".join(names).lower() or "aiosd" in cmdline.lower():
                yield process
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue


def is_aiosd_running() -> bool:
    return any(True for _ in iter_aiosd_processes())


def stop_aiosd() -> tuple[bool, str]:
    processes = list(iter_aiosd_processes())
    if not processes:
        return True, "aiosd is not running"

    for process in processes:
        try:
            process.terminate()
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue

    _, alive = psutil.wait_procs(processes, timeout=3)
    for process in alive:
        try:
            process.kill()
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue

    return True, "aiosd stopped"


def start_aiosd() -> tuple[bool, str]:
    executable = find_aiosd_executable()
    if not executable:
        return False, "aiosd executable not found"

    try:
        subprocess.Popen([executable], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return True, "aiosd started"
    except OSError:
        return False, "failed to start aiosd"


def restart_aiosd() -> tuple[bool, str]:
    ok, message = stop_aiosd()
    if not ok:
        return ok, message
    return start_aiosd()


def toggle_aiosd() -> tuple[bool, str]:
    if is_aiosd_running():
        return stop_aiosd()
    return start_aiosd()


def proc_stats(name: str) -> tuple[float, float]:
    cpu_total = 0.0
    ram_total = 0.0
    for process in psutil.process_iter(["name", "exe", "memory_info"]):
        try:
            process_name = process.info.get("name") or os.path.basename(process.info.get("exe") or "")
            if not process_name.lower().startswith(name.lower()):
                continue
            cpu_total += process.cpu_percent(interval=None)
            memory_info = process.info.get("memory_info")
            if memory_info:
                ram_total += memory_info.rss / (1024 * 1024)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    return cpu_total, ram_total


def merge_config_values(base: dict, loaded: dict) -> dict:
    merged = dict(base)
    for key, value in loaded.items():
        if key == "daemon" and isinstance(value, dict):
            daemon = dict(merged.get("daemon", {}))
            daemon.update(value)
            merged["daemon"] = daemon
        else:
            merged[key] = value
    return merged


def load_raios_config() -> dict:
    defaults = default_raios_config()
    if not CONFIG_PATH.exists():
        return defaults

    try:
        loaded = tomllib.loads(CONFIG_PATH.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return defaults
    return merge_config_values(defaults, loaded)


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def toml_bool(value: bool) -> str:
    return "true" if value else "false"


def serialize_raios_config(config: dict) -> str:
    lines: list[str] = []
    for key in CONFIG_TOP_LEVEL_KEYS:
        lines.append(f"{key} = {toml_string(str(config.get(key, '')))}")

    lines.append("")
    lines.append("[daemon]")

    daemon = config.get("daemon", {})
    for key, _ in DAEMON_BOOL_FIELDS:
        lines.append(f"{key} = {toml_bool(bool(daemon.get(key, False)))}")
    for key, _, _, _ in DAEMON_INT_FIELDS:
        lines.append(f"{key} = {int(daemon.get(key, 0))}")

    lines.append("")
    return "\n".join(lines)


def save_raios_config(config: dict) -> None:
    ensure_parent(CONFIG_PATH)
    CONFIG_PATH.write_text(serialize_raios_config(config), encoding="utf-8")


def open_in_file_manager(path: Path) -> bool:
    try:
        if detect_platform() == "windows":
            subprocess.Popen(["explorer", str(path)])
            return True
        if detect_platform() == "darwin":
            subprocess.Popen(["open", str(path)])
            return True
        if QDesktopServices.openUrl(QUrl.fromLocalFile(str(path))):
            return True
        xdg = shutil.which("xdg-open")
        if xdg:
            subprocess.Popen([xdg, str(path)])
            return True
        return False
    except OSError:
        return False


def fetch_state() -> TrayState:
    token = read_token()
    health = api_get("/api/health", token)
    projects_raw = api_get("/api/projects", token)
    usage = load_usage()

    if isinstance(projects_raw, list) and projects_raw:
        save_cache(projects_raw)
        projects = sort_projects(projects_raw, usage)
        projects_from_cache = False
    else:
        projects = sort_projects(load_cache(), usage)
        projects_from_cache = True

    dirty_projects: set[str] = set()
    for p in (projects or []):
        path = p.get("local_path", "")
        if path and check_git_dirty(path):
            name = p.get("name", "") or Path(path).name
            if name:
                dirty_projects.add(name)

    aiosd_cpu, aiosd_ram = proc_stats("aiosd")
    mem_items = load_mem_items()
    tasks = load_tasks()
    return TrayState(
        online=health is not None,
        health=health or {},
        projects=projects,
        projects_from_cache=projects_from_cache,
        usage=usage,
        aiosd_cpu=aiosd_cpu,
        aiosd_ram_mb=aiosd_ram,
        error="" if health is not None else "R-AI-OS API unreachable",
        dirty_projects=dirty_projects,
        mem_items=mem_items,
        tasks=tasks,
    )


class PathInput(QWidget):
    def __init__(self, initial_value: str, mode: str, parent: QWidget | None = None):
        super().__init__(parent)
        self.mode = mode
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

        self.input = QLineEdit(initial_value, self)
        browse = QPushButton("Browse", self)
        browse.clicked.connect(self.pick_path)

        layout.addWidget(self.input, stretch=1)
        layout.addWidget(browse)

    def value(self) -> str:
        return self.input.text().strip()

    def pick_path(self) -> None:
        current = self.input.text().strip() or str(Path.home())
        path = ""
        if self.mode == "file":
            path, _ = QFileDialog.getOpenFileName(self, "Choose file", current)
        else:
            path = QFileDialog.getExistingDirectory(self, "Choose directory", current)
        if not path and shutil.which("zenity"):
            try:
                args = ["zenity", "--file-selection", "--filename", current]
                if self.mode == "dir":
                    args.insert(2, "--directory")
                result = subprocess.run(args, capture_output=True, text=True, timeout=30)
                if result.returncode == 0:
                    path = result.stdout.strip()
            except (OSError, subprocess.TimeoutExpired):
                pass
        if path:
            self.input.setText(path)


class SettingsDialog(QDialog):
    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self.config = load_raios_config()
        self.restart_requested = False
        self.path_inputs: dict[str, PathInput] = {}
        self.text_inputs: dict[str, QLineEdit] = {}
        self.bool_inputs: dict[str, QCheckBox] = {}
        self.int_inputs: dict[str, QSpinBox] = {}

        self.setWindowTitle("R-AI-OS Settings")
        self.setMinimumSize(760, 720)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 12, 12, 12)
        outer.setSpacing(10)

        note = QLabel(
            f"Config file: {CONFIG_PATH}\nDaemon worker and scan path changes require an aiosd restart.",
            self,
        )
        note.setWordWrap(True)
        note.setStyleSheet(f"color: {_card_theme()['hint']};")
        outer.addWidget(note)

        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        content = QWidget(scroll)
        content_layout = QVBoxLayout(content)
        content_layout.setContentsMargins(0, 0, 0, 0)
        content_layout.setSpacing(12)

        content_layout.addWidget(self._build_workspace_group())
        content_layout.addWidget(self._build_general_group())
        content_layout.addWidget(self._build_daemon_group())
        content_layout.addStretch(1)

        scroll.setWidget(content)
        outer.addWidget(scroll)

        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel, self)
        save_restart = buttons.addButton("Save + Restart aiosd", QDialogButtonBox.ActionRole)
        open_config_dir = buttons.addButton("Open Config Dir", QDialogButtonBox.ActionRole)
        buttons.accepted.connect(self.save_only)
        buttons.rejected.connect(self.reject)
        save_restart.clicked.connect(self.save_and_restart)
        open_config_dir.clicked.connect(self.open_config_dir)
        outer.addWidget(buttons)

    def _build_workspace_group(self) -> QGroupBox:
        group = QGroupBox("Workspace and Scan Locations", self)
        form = QFormLayout(group)

        self.path_inputs["dev_ops_path"] = PathInput(str(self.config.get("dev_ops_path", "")), "dir", group)
        self.path_inputs["master_md_path"] = PathInput(str(self.config.get("master_md_path", "")), "file", group)
        self.path_inputs["skills_path"] = PathInput(str(self.config.get("skills_path", "")), "dir", group)
        self.path_inputs["vault_projects_path"] = PathInput(str(self.config.get("vault_projects_path", "")), "dir", group)

        form.addRow("Dev workspace root", self.path_inputs["dev_ops_path"])
        form.addRow("Constitution path", self.path_inputs["master_md_path"])
        form.addRow("Skills path", self.path_inputs["skills_path"])
        form.addRow("Vault projects path", self.path_inputs["vault_projects_path"])
        return group

    def _build_general_group(self) -> QGroupBox:
        group = QGroupBox("General", self)
        form = QFormLayout(group)

        self.text_inputs["system_name"] = QLineEdit(str(self.config.get("system_name", "")), group)
        self.text_inputs["github_user"] = QLineEdit(str(self.config.get("github_user", "")), group)

        form.addRow("System name", self.text_inputs["system_name"])
        form.addRow("GitHub user", self.text_inputs["github_user"])
        return group

    def _build_daemon_group(self) -> QGroupBox:
        group = QGroupBox("Daemon Worker Tuning", self)
        layout = QVBoxLayout(group)

        checks = QGroupBox("Worker Switches", group)
        checks_form = QFormLayout(checks)
        daemon_cfg = self.config.get("daemon", {})
        for key, label in DAEMON_BOOL_FIELDS:
            widget = QCheckBox(group)
            widget.setChecked(bool(daemon_cfg.get(key, False)))
            self.bool_inputs[key] = widget
            checks_form.addRow(label, widget)
        layout.addWidget(checks)

        intervals = QGroupBox("Intervals and Thresholds", group)
        intervals_form = QFormLayout(intervals)
        for key, label, minimum, maximum in DAEMON_INT_FIELDS:
            widget = QSpinBox(group)
            widget.setRange(minimum, maximum)
            widget.setValue(int(daemon_cfg.get(key, minimum)))
            self.int_inputs[key] = widget
            intervals_form.addRow(label, widget)
        layout.addWidget(intervals)

        hint = QLabel(
            "These values control health, git, sentinel, lifecycle, and port-monitor workers loaded by aiosd at startup.",
            group,
        )
        hint.setWordWrap(True)
        hint.setStyleSheet(f"color: {_card_theme()['hint']};")
        layout.addWidget(hint)
        return group

    def build_config(self) -> dict:
        config = default_raios_config()
        for key in CONFIG_TOP_LEVEL_KEYS:
            if key in self.path_inputs:
                config[key] = self.path_inputs[key].value()
            else:
                config[key] = self.text_inputs[key].text().strip()

        daemon = default_daemon_config()
        for key in self.bool_inputs:
            daemon[key] = self.bool_inputs[key].isChecked()
        for key in self.int_inputs:
            daemon[key] = int(self.int_inputs[key].value())
        config["daemon"] = daemon
        return config

    def validate_config(self, config: dict) -> str | None:
        if not config["dev_ops_path"]:
            return "Dev workspace root cannot be empty."
        if not config["master_md_path"]:
            return "Constitution path cannot be empty."
        if config["daemon"]["lifecycle_archive_days"] < config["daemon"]["lifecycle_standby_days"]:
            return "Archive days must be greater than or equal to standby days."
        return None

    def persist(self) -> bool:
        config = self.build_config()
        error = self.validate_config(config)
        if error:
            QMessageBox.warning(self, APP_NAME, error)
            return False

        try:
            save_raios_config(config)
        except OSError as exc:
            QMessageBox.critical(self, APP_NAME, f"Could not write config.toml:\n{exc}")
            return False
        return True

    def save_only(self) -> None:
        self.restart_requested = False
        if self.persist():
            self.accept()

    def save_and_restart(self) -> None:
        self.restart_requested = True
        if self.persist():
            self.accept()

    def open_config_dir(self) -> None:
        if not open_in_file_manager(CONFIG_DIR):
            QMessageBox.warning(self, APP_NAME, "Unable to open config directory.")


class MemoryBrowserDialog(QDialog):
    def __init__(self, items: list[dict], parent: QWidget | None = None):
        super().__init__(parent)
        self._all_items = items
        self.setWindowTitle("Memory Browser — raios")
        self.setMinimumSize(720, 540)
        self.setModal(False)

        tc = _card_theme()
        dark = _is_dark_mode()
        type_colors = MEM_TYPE_COLORS_DARK if dark else MEM_TYPE_COLORS_LIGHT

        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 12, 12, 12)
        outer.setSpacing(8)

        # Header: search + export
        header = QHBoxLayout()
        self._search = QLineEdit(self)
        self._search.setPlaceholderText("Search memory items…")
        self._search.textChanged.connect(self._filter)
        header.addWidget(self._search, stretch=1)

        export_btn = QPushButton("Export All", self)
        export_btn.setToolTip("raios mem export — regenerate markdown files from DB")
        export_btn.clicked.connect(self._export)
        header.addWidget(export_btn)
        outer.addLayout(header)

        count_text = f"{len(items)} item(s)" if items else "No memory items yet"
        self._count_label = QLabel(count_text, self)
        self._count_label.setStyleSheet(f"color: {tc['muted']}; font-size: 11px;")
        outer.addWidget(self._count_label)

        # Scrollable card list
        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        container = QWidget(scroll)
        self._cards_layout = QVBoxLayout(container)
        self._cards_layout.setContentsMargins(0, 0, 0, 0)
        self._cards_layout.setSpacing(8)
        self._cards_layout.addStretch(1)
        scroll.setWidget(container)
        outer.addWidget(scroll)

        self._card_widgets: list[tuple[QWidget, str]] = []  # (widget, search_text)
        for item in items:
            card = self._make_card(item, tc, type_colors)
            self._cards_layout.insertWidget(self._cards_layout.count() - 1, card)
            search_text = " ".join([
                item.get("slug", ""),
                item.get("title", ""),
                item.get("description", ""),
                item.get("body", ""),
                item.get("item_type", ""),
            ]).lower()
            self._card_widgets.append((card, search_text))

        # Bottom bar
        bottom = QHBoxLayout()
        bottom.addStretch(1)
        close_btn = QPushButton("Close", self)
        close_btn.clicked.connect(self.close)
        bottom.addWidget(close_btn)
        outer.addLayout(bottom)

    def _make_card(self, item: dict, tc: dict, type_colors: dict) -> QWidget:
        card = QWidget(self)
        card.setStyleSheet(
            f"background: {tc['card_bg']}; border: 1px solid {tc['border']}; border-radius: 6px;"
        )
        layout = QVBoxLayout(card)
        layout.setContentsMargins(12, 10, 12, 10)
        layout.setSpacing(4)

        # Top: type badge + title
        top = QHBoxLayout()
        top.setSpacing(8)

        itype = item.get("item_type", "?")
        color = type_colors.get(itype, tc["muted"])
        badge = QLabel(f"[{itype}]", card)
        badge.setStyleSheet(f"color: {color}; font-weight: bold; font-size: 11px;")
        top.addWidget(badge)

        title = QLabel(item.get("title", item.get("slug", "?")), card)
        title_font = title.font()
        title_font.setBold(True)
        title.setFont(title_font)
        top.addWidget(title, stretch=1)

        ts = item.get("updated_at", "")[:10]
        if ts:
            ts_label = QLabel(ts, card)
            ts_label.setStyleSheet(f"color: {tc['hint']}; font-size: 10px;")
            top.addWidget(ts_label)
        layout.addLayout(top)

        # Description
        desc = item.get("description", "")
        if desc:
            desc_label = QLabel(desc, card)
            desc_label.setWordWrap(True)
            desc_label.setStyleSheet(f"color: {tc['muted']}; font-style: italic; font-size: 11px;")
            layout.addWidget(desc_label)

        # Body (full text, selectable)
        body = item.get("body", "").strip()
        if body:
            body_label = QLabel(body, card)
            body_label.setWordWrap(True)
            body_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
            body_label.setStyleSheet(f"font-size: 12px; padding-top: 4px;")
            layout.addWidget(body_label)

        return card

    def _filter(self, text: str) -> None:
        query = text.strip().lower()
        visible = 0
        for card, search_text in self._card_widgets:
            show = not query or query in search_text
            card.setVisible(show)
            if show:
                visible += 1
        total = len(self._card_widgets)
        self._count_label.setText(
            f"{visible}/{total} item(s)" if query else f"{total} item(s)"
        )

    def _export(self) -> None:
        raios = shutil.which("raios")
        if not raios:
            QMessageBox.warning(self, APP_NAME, "raios binary not found in PATH.")
            return
        try:
            result = subprocess.run(
                [raios, "mem", "export"],
                capture_output=True, text=True, timeout=10,
            )
            msg = result.stdout.strip() or result.stderr.strip() or "Done."
            QMessageBox.information(self, APP_NAME, msg)
        except (OSError, subprocess.TimeoutExpired) as exc:
            QMessageBox.critical(self, APP_NAME, f"Export failed: {exc}")


class QuickAddTaskDialog(QDialog):
    def __init__(self, projects: list[dict], parent: QWidget | None = None):
        super().__init__(parent)
        self.setWindowTitle("Add Task — raios")
        self.setMinimumWidth(440)
        self.setModal(True)

        tc = _card_theme()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(10)

        lbl = QLabel("New task:", self)
        lbl.setStyleSheet("font-weight: bold;")
        layout.addWidget(lbl)

        self._text = QLineEdit(self)
        self._text.setPlaceholderText("What needs to be done?")
        self._text.returnPressed.connect(self._submit)
        layout.addWidget(self._text)

        proj_row = QHBoxLayout()
        proj_lbl = QLabel("Project:", self)
        proj_row.addWidget(proj_lbl)
        self._project = QLineEdit(self)
        self._project.setPlaceholderText("(optional)")
        proj_row.addWidget(self._project, stretch=1)
        layout.addLayout(proj_row)

        hint = QLabel("Enter or click Add to save.", self)
        hint.setStyleSheet(f"color: {tc['muted']}; font-size: 11px;")
        layout.addWidget(hint)

        btns = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel, self)
        btns.button(QDialogButtonBox.Ok).setText("Add")
        btns.accepted.connect(self._submit)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

        self._text.setFocus()

        # Pre-fill project from pinned/most-used if only one
        if len(projects) == 1:
            self._project.setText(projects[0].get("name", ""))

    def _submit(self) -> None:
        text = self._text.text().strip()
        if not text:
            return
        project = self._project.text().strip() or None
        if add_task(text, project):
            self.accept()
        else:
            QMessageBox.warning(self, APP_NAME, "Failed to save task to database.")


class TaskListDialog(QDialog):
    def __init__(self, tasks: list[dict], projects: list[dict], parent: QWidget | None = None):
        super().__init__(parent)
        self.setWindowTitle("Tasks — raios")
        self.setMinimumSize(520, 420)
        self.setModal(False)

        self._tc = _card_theme()
        self._projects = projects

        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 12, 12, 12)
        outer.setSpacing(8)

        header = QHBoxLayout()
        self._count_lbl = QLabel(self)
        self._count_lbl.setStyleSheet(f"color: {self._tc['muted']}; font-size: 11px;")
        header.addWidget(self._count_lbl, stretch=1)
        add_btn = QPushButton("+ Add Task", self)
        add_btn.clicked.connect(self._on_add)
        header.addWidget(add_btn)
        outer.addLayout(header)

        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        self._container = QWidget(scroll)
        self._cards_layout = QVBoxLayout(self._container)
        self._cards_layout.setContentsMargins(0, 0, 0, 0)
        self._cards_layout.setSpacing(6)
        self._cards_layout.addStretch(1)
        scroll.setWidget(self._container)
        outer.addWidget(scroll)

        bottom = QHBoxLayout()
        bottom.addStretch(1)
        close_btn = QPushButton("Close", self)
        close_btn.clicked.connect(self.close)
        bottom.addWidget(close_btn)
        outer.addLayout(bottom)

        self._reload(tasks)

    def _reload(self, tasks: list[dict]) -> None:
        while self._cards_layout.count() > 1:
            item = self._cards_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

        for task in tasks:
            self._cards_layout.insertWidget(
                self._cards_layout.count() - 1, self._make_card(task)
            )

        n = len(tasks)
        self._count_lbl.setText(f"{n} pending task(s)" if n else "No pending tasks")

    def _make_card(self, task: dict) -> QWidget:
        tc = self._tc
        card = QWidget(self._container)
        card.setStyleSheet(
            f"background: {tc['card_bg']}; border: 1px solid {tc['border']}; border-radius: 6px;"
        )
        row = QHBoxLayout(card)
        row.setContentsMargins(12, 8, 8, 8)
        row.setSpacing(8)

        text_col = QVBoxLayout()
        text_lbl = QLabel(task.get("text", ""), card)
        text_lbl.setWordWrap(True)
        text_lbl.setTextInteractionFlags(Qt.TextSelectableByMouse)
        text_col.addWidget(text_lbl)

        meta_parts: list[str] = []
        proj = task.get("project")
        if proj:
            meta_parts.append(proj)
        ts = (task.get("created_at") or "")[:10]
        if ts:
            meta_parts.append(ts)
        if meta_parts:
            meta_lbl = QLabel("  ".join(meta_parts), card)
            meta_lbl.setStyleSheet(f"color: {tc['muted']}; font-size: 10px;")
            text_col.addWidget(meta_lbl)
        row.addLayout(text_col, stretch=1)

        tid: int = task["id"]

        done_btn = QPushButton("✓", card)
        done_btn.setToolTip("Mark as done")
        done_btn.setFixedSize(28, 28)
        done_btn.setStyleSheet(
            "QPushButton{color:#4ade80;font-weight:bold;border:1px solid #4ade80;border-radius:4px;}"
            "QPushButton:hover{background:#4ade80;color:#000;}"
        )
        done_btn.clicked.connect(lambda _c, t=tid: self._on_done(t))
        row.addWidget(done_btn)

        del_btn = QPushButton("✕", card)
        del_btn.setToolTip("Delete task")
        del_btn.setFixedSize(28, 28)
        del_btn.setStyleSheet(
            "QPushButton{color:#f87171;font-weight:bold;border:1px solid #f87171;border-radius:4px;}"
            "QPushButton:hover{background:#f87171;color:#000;}"
        )
        del_btn.clicked.connect(lambda _c, t=tid: self._on_delete(t))
        row.addWidget(del_btn)

        return card

    def _on_done(self, task_id: int) -> None:
        complete_task(task_id)
        self._reload(load_tasks())

    def _on_delete(self, task_id: int) -> None:
        delete_task(task_id)
        self._reload(load_tasks())

    def _on_add(self) -> None:
        dlg = QuickAddTaskDialog(self._projects, self)
        if dlg.exec() == QDialog.Accepted:
            self._reload(load_tasks())


class ProjectEditDialog(QDialog):
    def __init__(self, parent: QWidget | None, name: str = "", path: str = ""):
        super().__init__(parent)
        self.setWindowTitle("Project Editor")
        self.setMinimumWidth(520)

        layout = QFormLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(10)

        self.name_input = QLineEdit(name, self)
        self.path_input = PathInput(path, "dir", self)

        layout.addRow("Project Name", self.name_input)
        layout.addRow("Project Path", self.path_input)

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel, self)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addRow(buttons)

    def result(self) -> tuple[str, str]:
        return self.name_input.text().strip(), self.path_input.value()


class ProjectManagerDialog(QDialog):
    def __init__(self, parent: QWidget | None, projects: list[dict], usage: dict, managed_config: dict,
                 on_launch: Callable[[str, Agent, str], bool], on_vscode: Callable[[str], bool],
                 dirty_projects: set[str] | None = None):
        super().__init__(parent)
        self.api_projects = projects
        self.usage = usage
        self.managed_config = managed_config
        self.on_launch = on_launch
        self.on_vscode = on_vscode
        self.dirty_projects = dirty_projects or set()
        self.rows: list[tuple[QWidget, str]] = []
        self._menu_refs: list[QMenu | QAction] = []

        self.setWindowTitle("Manage Projects")
        self.setMinimumSize(860, 600)
        self.setModal(False)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 12, 12, 12)
        outer.setSpacing(10)

        header = QHBoxLayout()
        self.search = QLineEdit(self)
        self.search.setPlaceholderText("Search projects...")
        self.search.textChanged.connect(self.filter_rows)
        header.addWidget(self.search, stretch=1)

        add_btn = QPushButton("+ Add Project", self)
        add_btn.clicked.connect(self.add_project)
        header.addWidget(add_btn)
        outer.addLayout(header)

        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        container = QWidget(scroll)
        self.rows_layout = QGridLayout(container)
        self.rows_layout.setContentsMargins(0, 0, 0, 0)
        self.rows_layout.setHorizontalSpacing(10)
        self.rows_layout.setVerticalSpacing(10)
        scroll.setWidget(container)
        outer.addWidget(scroll)

        hint = QLabel("Right-click or use buttons to manage. Pinned projects appear at top of tray menu.", self)
        hint.setWordWrap(True)
        hint.setStyleSheet(f"color: {_card_theme()['hint']}; font-style: italic;")
        outer.addWidget(hint)

        self._populate()

    def _all_entries(self) -> list[dict]:
        managed = self.managed_config.get("projects", [])
        managed_names = {p["name"] for p in managed}
        entries = list(managed)
        for ap in self.api_projects:
            name = ap.get("name", "")
            if name not in managed_names:
                entries.append({"name": name, "path": ap.get("local_path", ""), "pinned": False, "_api": True})
        return entries

    def _populate(self) -> None:
        self._menu_refs.clear()
        while self.rows_layout.count():
            item = self.rows_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        self.rows.clear()

        tc = _card_theme()
        entries = self._all_entries()
        for index, entry in enumerate(entries):
            name = entry.get("name", "?")
            path = entry.get("path", "")
            is_pinned = entry.get("pinned", False)
            is_api = entry.get("_api", False)
            is_dirty = name in self.dirty_projects

            row = QWidget(self)
            row_layout = QVBoxLayout(row)
            row_layout.setContentsMargins(10, 10, 10, 10)
            row_layout.setSpacing(8)
            row.setStyleSheet(
                f"background: {tc['pinned_bg']}; border-radius: 6px;" if is_pinned
                else f"background: {tc['card_bg']}; border: 1px solid {tc['border']}; border-radius: 6px;"
            )

            top_row = QHBoxLayout()
            top_row.setSpacing(8)
            info = QVBoxLayout()
            name_text = f"{name} ●" if is_dirty else name
            name_label = QLabel(name_text, row)
            name_font = name_label.font()
            name_font.setBold(is_pinned)
            name_label.setFont(name_font)
            if is_dirty:
                name_label.setStyleSheet(f"color: {tc['dirty']};")
            path_label = QLabel(path, row)
            path_label.setWordWrap(True)
            path_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
            path_label.setStyleSheet(f"color: {tc['muted']}; font-size: 11px;")
            info.addWidget(name_label)
            info.addWidget(path_label)
            top_row.addLayout(info, stretch=1)

            meta = QVBoxLayout()
            meta.setSpacing(4)

            if is_dirty:
                dirty_tag = QLabel("dirty", row)
                dirty_tag.setStyleSheet(
                    f"color: {tc['dirty']}; font-size: 10px; font-weight: bold; padding: 0 4px;"
                )
                meta.addWidget(dirty_tag, alignment=Qt.AlignRight)
            elif is_api:
                api_tag = QLabel("API", row)
                api_tag.setStyleSheet(f"color: {tc['api_tag']}; font-size: 10px; padding: 0 4px;")
                meta.addWidget(api_tag, alignment=Qt.AlignRight)

            count = self.usage.get(name, 0)
            if count:
                meta.addWidget(QLabel(f"{count}x", row), alignment=Qt.AlignRight)
            top_row.addLayout(meta)
            row_layout.addLayout(top_row)

            primary_actions = QHBoxLayout()
            primary_actions.setSpacing(6)

            pin_btn = QPushButton("📌" if is_pinned else "Pin", row)
            if is_pinned:
                pin_btn.setIcon(get_pin_logo())
                pin_btn.setText("")
            else:
                pin_btn.setIcon(QIcon())
            pin_btn.setToolTip("Pin/Unpin project")
            pin_btn.clicked.connect(lambda _, n=name: self.toggle_pin(n))
            primary_actions.addWidget(pin_btn)

            if not is_api:
                edit_btn = QPushButton("Edit", row)
                edit_btn.clicked.connect(lambda _, n=name: self.edit_project(n))
                primary_actions.addWidget(edit_btn)

                remove_btn = QPushButton("Remove", row)
                remove_btn.clicked.connect(lambda _, n=name: self.remove_project(n))
                primary_actions.addWidget(remove_btn)
            else:
                primary_actions.addStretch(1)
            row_layout.addLayout(primary_actions)

            secondary_actions = QHBoxLayout()
            secondary_actions.setSpacing(6)

            vscode_btn = QPushButton("VSCode", row)
            vscode_btn.clicked.connect(lambda _, p=path: self._handle_vscode(p))
            secondary_actions.addWidget(vscode_btn)

            agent_menu = QToolButton(row)
            agent_menu.setText("Agent")
            agent_menu.setPopupMode(QToolButton.InstantPopup)

            agent_dropdown = QMenu(agent_menu)
            for agent in AGENTS:
                action = QAction(agent.name, agent_dropdown)
                action.triggered.connect(lambda _, a=agent, p=path, n=name: self._handle_launch(p, a, n))
                agent_dropdown.addAction(action)
                self._menu_refs.append(action)
            agent_menu.setMenu(agent_dropdown)
            self._menu_refs.append(agent_dropdown)
            secondary_actions.addWidget(agent_menu)
            secondary_actions.addStretch(1)
            row_layout.addLayout(secondary_actions)

            self.rows_layout.addWidget(row, index // 2, index % 2)
            self.rows.append((row, name.lower()))
        self.rows_layout.setColumnStretch(0, 1)
        self.rows_layout.setColumnStretch(1, 1)

    def filter_rows(self, text: str) -> None:
        query = text.strip().lower()
        for row, search_text in self.rows:
            row.setVisible(not query or query in search_text)

    def add_project(self) -> None:
        dialog = ProjectEditDialog(self)
        if dialog.exec() != QDialog.Accepted:
            return
        name, path = dialog.result()
        if not name or not path:
            QMessageBox.warning(self, APP_NAME, "Both name and path are required.")
            return
        managed = self.managed_config.get("projects", [])
        if any(p["name"] == name for p in managed):
            QMessageBox.warning(self, APP_NAME, f"Project '{name}' already exists.")
            return
        managed.append({"name": name, "path": path, "pinned": False})
        self.managed_config["projects"] = managed
        save_projects_config(self.managed_config)
        self._populate()

    def edit_project(self, name: str) -> None:
        managed = self.managed_config.get("projects", [])
        entry = next((p for p in managed if p["name"] == name), None)
        if not entry:
            return
        dialog = ProjectEditDialog(self, entry["name"], entry["path"])
        if dialog.exec() != QDialog.Accepted:
            return
        new_name, new_path = dialog.result()
        if not new_name or not new_path:
            QMessageBox.warning(self, APP_NAME, "Both name and path are required.")
            return
        if new_name != name and any(p["name"] == new_name for p in managed):
            QMessageBox.warning(self, APP_NAME, f"Project '{new_name}' already exists.")
            return
        entry["name"] = new_name
        entry["path"] = new_path
        save_projects_config(self.managed_config)
        self._populate()

    def remove_project(self, name: str) -> None:
        reply = QMessageBox.question(
            self, APP_NAME, f"Remove '{name}' from managed projects?",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No
        )
        if reply != QMessageBox.Yes:
            return
        managed = self.managed_config.get("projects", [])
        self.managed_config["projects"] = [p for p in managed if p["name"] != name]
        save_projects_config(self.managed_config)
        self._populate()

    def toggle_pin(self, name: str) -> None:
        managed = self.managed_config.get("projects", [])
        entry = next((p for p in managed if p["name"] == name), None)
        if entry:
            entry["pinned"] = not entry.get("pinned", False)
        else:
            api_entry = next((p for p in self.api_projects if p.get("name") == name), None)
            if api_entry:
                managed.append({
                    "name": name,
                    "path": api_entry.get("local_path", ""),
                    "pinned": True,
                })
        self.managed_config["projects"] = managed
        save_projects_config(self.managed_config)
        self._populate()

    def _handle_launch(self, project_path: str, agent: Agent, project_name: str) -> None:
        if self.on_launch(project_path, agent, project_name):
            self.hide()
            return
        QMessageBox.warning(self, APP_NAME, f"{agent.name} command not found.")

    def _handle_vscode(self, project_path: str) -> None:
        if self.on_vscode(project_path):
            self.hide()
            return
        QMessageBox.warning(self, APP_NAME, "VSCode (code) command not found.")


class RaiosTray(QObject):
    def __init__(self, app: QApplication):
        super().__init__()
        self.app = app
        self.state = TrayState(projects=[], usage={}, health={})
        self._manage_dialog: ProjectManagerDialog | None = None
        self._memory_dialog: MemoryBrowserDialog | None = None
        self._task_dialog: TaskListDialog | None = None
        self._fetching = False

        # Native AppIndicator3 icon
        self._indicator = AppIndicator3.Indicator.new(
            "raios-tray",
            "utilities-system-monitor",
            AppIndicator3.IndicatorCategory.APPLICATION_STATUS,
        )
        self._indicator.set_status(AppIndicator3.IndicatorStatus.ACTIVE)
        self._indicator.set_title(APP_NAME)

        # GTK menu
        self._gtk_menu = Gtk.Menu()
        self._indicator.set_menu(self._gtk_menu)

        # Pump GTK events every 50 ms so GTK menu callbacks fire on Qt's main thread
        self._gtk_pump = QTimer(self)
        self._gtk_pump.timeout.connect(self._pump_gtk)
        self._gtk_pump.start(50)

        # Data refresh
        self.refresh_timer = QTimer(self)
        self.refresh_timer.setInterval(REFRESH_SECONDS * 1000)
        self.refresh_timer.timeout.connect(self.refresh)

        self.rebuild_menu()
        self.refresh_timer.start()
        QTimer.singleShot(0, self.refresh)

    # ── GTK helpers ──────────────────────────────────────────────────────────

    def _pump_gtk(self) -> None:
        ctx = GLib.MainContext.default()
        while ctx.pending():
            ctx.iteration(False)

    def _gtk_item(self, label: str, cb=None, sensitive: bool = True) -> Gtk.MenuItem:
        item = Gtk.MenuItem(label=label)
        item.set_sensitive(sensitive)
        if cb:
            item.connect("activate", lambda _: cb())
        return item

    def _notify(self, message: str) -> None:
        if _GTK_NOTIFY:
            try:
                n = _GtkNotify.Notification.new(APP_NAME, message, "utilities-system-monitor")
                n.show()
            except Exception:
                pass

    # ── icon ─────────────────────────────────────────────────────────────────

    def _update_icon(self, dirty_count: int) -> None:
        if dirty_count > 0:
            self._indicator.set_icon_full(
                "software-update-urgent",
                f"R-AI-OS – {dirty_count} dirty",
            )
            self._indicator.set_title(f"{APP_NAME}  ●{dirty_count}")
        else:
            self._indicator.set_icon_full("utilities-system-monitor", APP_NAME)
            self._indicator.set_title(APP_NAME)

    # ── refresh ───────────────────────────────────────────────────────────────

    def refresh(self) -> None:
        if self._fetching:
            return
        self._fetching = True
        try:
            self._apply_state(fetch_state())
        finally:
            self._fetching = False

    def _apply_state(self, state: TrayState) -> None:
        self.state = state
        dirty = len(state.dirty_projects) if state.online else 0
        self._update_icon(dirty)
        self.rebuild_menu()

    # ── menu ──────────────────────────────────────────────────────────────────

    def rebuild_menu(self) -> None:
        for child in self._gtk_menu.get_children():
            self._gtk_menu.remove(child)

        self._gtk_menu.append(self._gtk_item("R-AI-OS / K-AI-RA", sensitive=False))
        self._gtk_menu.append(Gtk.SeparatorMenuItem())

        if not self.state.projects and not self.state.health:
            self._gtk_menu.append(self._gtk_item("Loading...", sensitive=False))

        elif not self.state.online:
            self._gtk_menu.append(self._gtk_item("aiosd offline", sensitive=False))
            self._gtk_menu.append(self._gtk_item("Start aiosd", self.toggle_daemon))
            cached = self.state.projects or []
            if cached:
                self._gtk_menu.append(Gtk.SeparatorMenuItem())
                self._gtk_menu.append(self._gtk_item(f"Cached Projects ({len(cached)})", sensitive=False))
                self._gtk_menu.append(self._gtk_item("Open Project Manager...", self.open_manage_projects))

        else:
            self._gtk_menu.append(self._gtk_item("Daemon", sensitive=False))
            self._gtk_menu.append(self._gtk_item(
                f"aiosd  CPU {self.state.aiosd_cpu:.1f}%  {self.state.aiosd_ram_mb:.0f} MB",
                sensitive=False,
            ))
            if (self.state.health or {}).get("needs_human_approval"):
                self._gtk_menu.append(self._gtk_item("Human approval required", sensitive=False))
            self._gtk_menu.append(self._gtk_item("Stop aiosd", self.toggle_daemon))

            if self.state.dirty_projects:
                self._gtk_menu.append(self._gtk_item(
                    f"● {len(self.state.dirty_projects)} dirty projects", sensitive=False,
                ))

            self._gtk_menu.append(Gtk.SeparatorMenuItem())

            managed_config = load_projects_config()
            pinned = [p for p in managed_config.get("projects", []) if p.get("pinned")]
            project_count = len(self.state.projects or [])
            lbl = "Cached Projects" if self.state.projects_from_cache else "Projects"
            self._gtk_menu.append(self._gtk_item(f"{lbl}: {project_count}", self.open_manage_projects))
            if pinned:
                self._gtk_menu.append(self._gtk_item(f"Pinned: {len(pinned)}", self.open_manage_projects))
            self._gtk_menu.append(self._gtk_item("Open Project Manager...", self.open_manage_projects))

        self._gtk_menu.append(Gtk.SeparatorMenuItem())
        self._gtk_menu.append(self._gtk_item("Manage Projects...", self.open_manage_projects))

        # ── Memory section ────────────────────────────────────────────────────
        self._gtk_menu.append(Gtk.SeparatorMenuItem())
        mem_items = self.state.mem_items
        if mem_items:
            latest = mem_items[0]
            itype = latest.get("item_type", "?")
            title = latest.get("title", latest.get("slug", "?"))
            label = f"✦ [{itype}] {title[:48]}"
            self._gtk_menu.append(self._gtk_item(label, sensitive=False))
            self._gtk_menu.append(
                self._gtk_item(f"Browse Memory ({len(mem_items)})…", self.open_memory_browser)
            )
        else:
            self._gtk_menu.append(self._gtk_item("Memory (empty)", self.open_memory_browser))

        # ── Tasks section ─────────────────────────────────────────────────────
        self._gtk_menu.append(Gtk.SeparatorMenuItem())
        pending = self.state.tasks
        if pending:
            self._gtk_menu.append(self._gtk_item(
                f"☑ {len(pending)} task(s) pending", self.open_task_list
            ))
        else:
            self._gtk_menu.append(self._gtk_item("Tasks (empty)", self.open_task_list))
        self._gtk_menu.append(self._gtk_item("+ Add Task...", self.open_quick_add_task))

        self._gtk_menu.append(Gtk.SeparatorMenuItem())
        self._gtk_menu.append(self._gtk_item("aiosd Settings", self.open_settings))
        self._gtk_menu.append(self._gtk_item("Open Config Directory", self.open_config_directory))
        self._gtk_menu.append(self._gtk_item("Open raios", self.open_raios_cli))
        self._gtk_menu.append(self._gtk_item("Refresh", self.refresh))
        self._gtk_menu.append(Gtk.SeparatorMenuItem())
        self._gtk_menu.append(self._gtk_item("Quit", self.app.quit))

        self._gtk_menu.show_all()

    # ── dialogs ───────────────────────────────────────────────────────────────

    def open_manage_projects(self) -> None:
        if self._manage_dialog and self._manage_dialog.isVisible():
            self._manage_dialog.raise_()
            self._manage_dialog.activateWindow()
            return
        managed_config = load_projects_config()
        dialog = ProjectManagerDialog(
            None, self.state.projects or [], self.state.usage or {},
            managed_config, self._launch_agent, launch_vscode,
            dirty_projects=self.state.dirty_projects,
        )
        self._manage_dialog = dialog
        dialog.finished.connect(self._on_manage_closed)
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def _on_manage_closed(self) -> None:
        self._manage_dialog = None
        self.refresh()

    def _launch_agent(self, project_path: str, agent: Agent, project_name: str) -> bool:
        launched = launch_agent(project_path, agent, project_name)
        if launched:
            self.state.usage = load_usage()
            self.rebuild_menu()
        return launched

    def open_settings(self) -> None:
        dialog = SettingsDialog()
        dialog.finished.connect(lambda result: self._on_settings_closed(dialog, result))
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def _on_settings_closed(self, dialog: SettingsDialog, result: int) -> None:
        if result != QDialog.Accepted:
            self.refresh()
            return
        if dialog.restart_requested:
            ok, message = restart_aiosd()
            self._notify(message)
            if not ok:
                QMessageBox.warning(None, APP_NAME, message)
        else:
            self._notify("Config saved. Restart aiosd to apply worker changes.")
        self.refresh()

    def toggle_daemon(self) -> None:
        ok, message = toggle_aiosd()
        self._notify(message)
        if not ok:
            QMessageBox.warning(None, APP_NAME, message)
        self.refresh()

    def open_raios_cli(self) -> None:
        if not open_terminal(str(Path.home()), "raios"):
            self._notify("Unable to open terminal for raios.")

    def open_config_directory(self) -> None:
        if not open_in_file_manager(CONFIG_DIR):
            self._notify("Unable to open config directory.")

    def open_memory_browser(self) -> None:
        if self._memory_dialog and self._memory_dialog.isVisible():
            self._memory_dialog.raise_()
            self._memory_dialog.activateWindow()
            return
        items = load_mem_items()
        dialog = MemoryBrowserDialog(items, parent=None)
        self._memory_dialog = dialog
        dialog.finished.connect(lambda _: setattr(self, "_memory_dialog", None))
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def open_quick_add_task(self) -> None:
        projects = self.state.projects or []
        dlg = QuickAddTaskDialog(projects, parent=None)
        if dlg.exec() == QDialog.Accepted:
            self.state.tasks = load_tasks()
            self.rebuild_menu()
            self._notify("Task added.")

    def open_task_list(self) -> None:
        if self._task_dialog and self._task_dialog.isVisible():
            self._task_dialog.raise_()
            self._task_dialog.activateWindow()
            return
        projects = self.state.projects or []
        dialog = TaskListDialog(self.state.tasks, projects, parent=None)
        self._task_dialog = dialog
        dialog.finished.connect(self._on_task_dialog_closed)
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def _on_task_dialog_closed(self) -> None:
        self._task_dialog = None
        self.state.tasks = load_tasks()
        self.rebuild_menu()


def validate_environment() -> str | None:
    return None


def _apply_dark_palette(app: QApplication) -> None:
    pal = QPalette()
    c = QColor
    pal.setColor(QPalette.Window,          c("#252836"))
    pal.setColor(QPalette.WindowText,      c("#e0e0e6"))
    pal.setColor(QPalette.Base,            c("#1e2030"))
    pal.setColor(QPalette.AlternateBase,   c("#252836"))
    pal.setColor(QPalette.Text,            c("#e0e0e6"))
    pal.setColor(QPalette.BrightText,      c("#ffffff"))
    pal.setColor(QPalette.Button,          c("#2e3140"))
    pal.setColor(QPalette.ButtonText,      c("#e0e0e6"))
    pal.setColor(QPalette.Highlight,       c("#5c6bc0"))
    pal.setColor(QPalette.HighlightedText, c("#ffffff"))
    pal.setColor(QPalette.Mid,             c("#3e4257"))
    pal.setColor(QPalette.Dark,            c("#1a1c26"))
    pal.setColor(QPalette.Shadow,          c("#111320"))
    pal.setColor(QPalette.PlaceholderText, c("#8b8fa8"))
    pal.setColor(QPalette.Link,            c("#7b8fff"))
    pal.setColor(QPalette.Disabled, QPalette.Text,       c("#555770"))
    pal.setColor(QPalette.Disabled, QPalette.ButtonText, c("#555770"))
    pal.setColor(QPalette.Disabled, QPalette.WindowText, c("#555770"))
    app.setPalette(pal)


def main() -> int:
    # GTK must be initialized before QApplication grabs the display
    Gtk.init([])
    if _GTK_NOTIFY:
        _GtkNotify.init(APP_NAME)

    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setApplicationName(APP_NAME)
    app.setStyle("Fusion")

    if _is_dark_mode():
        _apply_dark_palette(app)

    RaiosTray(app)
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
