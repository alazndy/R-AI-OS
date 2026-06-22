#!/usr/bin/env python3
"""R-AI-OS system tray for Linux, macOS, and Windows."""

from __future__ import annotations

import json
import os
import platform
import shlex
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10
    import tomli as tomllib

import psutil
from PySide6.QtCore import QObject, QThread, QTimer, Qt, Signal
from PySide6.QtGui import QAction, QCursor, QDesktopServices, QIcon
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QStyle,
    QSystemTrayIcon,
    QVBoxLayout,
    QWidget,
)
from PySide6.QtCore import QUrl

API_BASE = "http://127.0.0.1:42071"
REFRESH_SECONDS = 15
MAX_PROJECTS = 10
APP_NAME = "R-AI-OS Tray"

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


AGENTS: tuple[Agent, ...] = (
    Agent("Claude", ("claude", "/home/alaz/.local/bin/claude")),
    Agent("Gemini", ("gemini", "/home/linuxbrew/.linuxbrew/bin/gemini")),
    Agent("Codex", ("codex", "/home/alaz/.local/bin/codex")),
    Agent("AGY", ("agy", "/home/alaz/.local/bin/agy")),
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
TOKEN_CANDIDATES = (
    CONFIG_DIR / ".session_token",
    CONFIG_DIR / ".ipc_token",
)
USAGE_PATH = CONFIG_DIR / "tray-usage.json"
CACHE_PATH = CONFIG_DIR / "tray-projects-cache.json"


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
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError):
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
        return QDesktopServices.openUrl(QUrl.fromLocalFile(str(path)))
    except OSError:
        return False


class FetchWorker(QObject):
    finished = Signal(object)

    def run(self) -> None:
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

        aiosd_cpu, aiosd_ram = proc_stats("aiosd")
        state = TrayState(
            online=health is not None,
            health=health or {},
            projects=projects,
            projects_from_cache=projects_from_cache,
            usage=usage,
            aiosd_cpu=aiosd_cpu,
            aiosd_ram_mb=aiosd_ram,
            error="" if health is not None else "R-AI-OS API unreachable",
        )
        self.finished.emit(state)


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
        if self.mode == "file":
            path, _ = QFileDialog.getOpenFileName(self, "Choose file", current)
        else:
            path = QFileDialog.getExistingDirectory(self, "Choose directory", current)
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
        note.setStyleSheet("color: #666;")
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
        hint.setStyleSheet("color: #666;")
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


class ProjectsDialog(QDialog):
    def __init__(self, parent: QWidget | None, projects: list[dict], usage: dict, on_launch: Callable[[str, Agent, str], bool]):
        super().__init__(parent)
        self.projects = projects
        self.usage = usage
        self.on_launch = on_launch
        self.rows: list[tuple[QWidget, str]] = []

        self.setWindowTitle(f"K-AI-RA Projects ({len(projects)})")
        self.setMinimumSize(760, 560)
        self.setModal(False)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 12, 12, 12)
        layout.setSpacing(10)

        self.search = QLineEdit(self)
        self.search.setPlaceholderText("Search projects...")
        self.search.textChanged.connect(self.filter_rows)
        layout.addWidget(self.search)

        scroll = QScrollArea(self)
        scroll.setWidgetResizable(True)
        container = QWidget(scroll)
        self.rows_layout = QVBoxLayout(container)
        self.rows_layout.setContentsMargins(0, 0, 0, 0)
        self.rows_layout.setSpacing(8)
        scroll.setWidget(container)
        layout.addWidget(scroll)

        self._populate()

    def _populate(self) -> None:
        for project in self.projects:
            name = project.get("name") or Path(project.get("local_path", "")).name or "unknown"
            path = project.get("local_path", "")
            count = self.usage.get(name, 0)

            row = QWidget(self)
            row_layout = QHBoxLayout(row)
            row_layout.setContentsMargins(8, 8, 8, 8)

            info = QVBoxLayout()
            name_label = QLabel(name, row)
            path_label = QLabel(path, row)
            path_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
            path_label.setStyleSheet("color: #666;")
            info.addWidget(name_label)
            info.addWidget(path_label)
            row_layout.addLayout(info, stretch=1)

            if count:
                row_layout.addWidget(QLabel(f"Used {count}x", row))

            for agent in AGENTS:
                button = QPushButton(agent.name, row)
                button.clicked.connect(
                    lambda _=False, p=path, a=agent, n=name: self._handle_launch(p, a, n)
                )
                row_layout.addWidget(button)

            self.rows_layout.addWidget(row)
            self.rows.append((row, name.lower()))

        self.rows_layout.addStretch(1)

    def filter_rows(self, text: str) -> None:
        query = text.strip().lower()
        for row, search_text in self.rows:
            row.setVisible(not query or query in search_text)

    def _handle_launch(self, project_path: str, agent: Agent, project_name: str) -> None:
        if self.on_launch(project_path, agent, project_name):
            self.hide()
            return
        QMessageBox.warning(self, APP_NAME, f"{agent.name} command not found on this machine.")


class RaiosTray(QObject):
    def __init__(self, app: QApplication):
        super().__init__()
        self.app = app
        self.icon = QSystemTrayIcon(self._build_icon(), app)
        self.icon.setToolTip(APP_NAME)
        self.icon.activated.connect(self._handle_activation)
        self.state = TrayState(projects=[], usage={}, health={})
        self.projects_dialog: ProjectsDialog | None = None
        self.fetch_thread: QThread | None = None
        self.fetch_worker: FetchWorker | None = None

        self.refresh_timer = QTimer(self)
        self.refresh_timer.setInterval(REFRESH_SECONDS * 1000)
        self.refresh_timer.timeout.connect(self.refresh)

        self.rebuild_menu()
        self.icon.show()
        self.refresh_timer.start()
        self.refresh()

    def _build_icon(self) -> QIcon:
        icon = QIcon.fromTheme("utilities-system-monitor")
        if icon.isNull():
            icon = self.app.style().standardIcon(QStyle.SP_ComputerIcon)
        return icon

    def _handle_activation(self, reason: QSystemTrayIcon.ActivationReason) -> None:
        if reason in (QSystemTrayIcon.Trigger, QSystemTrayIcon.Context):
            menu = self.icon.contextMenu()
            if menu:
                menu.popup(QCursor.pos())

    def refresh(self) -> None:
        if self.fetch_thread and self.fetch_thread.isRunning():
            return

        self.fetch_thread = QThread(self)
        self.fetch_worker = FetchWorker()
        self.fetch_worker.moveToThread(self.fetch_thread)
        self.fetch_thread.started.connect(self.fetch_worker.run)
        self.fetch_worker.finished.connect(self._apply_state)
        self.fetch_worker.finished.connect(self.fetch_thread.quit)
        self.fetch_worker.finished.connect(self.fetch_worker.deleteLater)
        self.fetch_thread.finished.connect(self.fetch_thread.deleteLater)
        self.fetch_thread.start()

    def _apply_state(self, state: TrayState) -> None:
        self.state = state
        count = len(state.projects or [])
        if state.online:
            self.icon.setToolTip(f"{APP_NAME} ({count} projects)")
        else:
            self.icon.setToolTip(f"{APP_NAME} (offline)")
        self.rebuild_menu()
        if self.projects_dialog and self.projects_dialog.isVisible():
            self.projects_dialog.close()
            self.projects_dialog = ProjectsDialog(None, state.projects or [], state.usage or {}, self._launch_agent)
            self.projects_dialog.show()

    def rebuild_menu(self) -> None:
        menu = QMenu()

        header = QAction("R-AI-OS / K-AI-RA", menu)
        header.setEnabled(False)
        menu.addAction(header)
        menu.addSeparator()

        if not self.state.projects and not self.state.health:
            loading = QAction("Loading...", menu)
            loading.setEnabled(False)
            menu.addAction(loading)
        elif not self.state.online:
            offline = QAction("aiosd offline", menu)
            offline.setEnabled(False)
            menu.addAction(offline)

            start_action = QAction("Start aiosd", menu)
            start_action.triggered.connect(self.toggle_daemon)
            menu.addAction(start_action)
            self._append_cached_projects(menu)
        else:
            daemon_header = QAction("Daemon", menu)
            daemon_header.setEnabled(False)
            menu.addAction(daemon_header)

            stats = QAction(
                f"aiosd CPU {self.state.aiosd_cpu:.1f}%  {self.state.aiosd_ram_mb:.0f} MB",
                menu,
            )
            stats.setEnabled(False)
            menu.addAction(stats)

            if (self.state.health or {}).get("needs_human_approval"):
                approval = QAction("Human approval required", menu)
                approval.setEnabled(False)
                menu.addAction(approval)

            stop_action = QAction("Stop aiosd", menu)
            stop_action.triggered.connect(self.toggle_daemon)
            menu.addAction(stop_action)

            menu.addSeparator()
            section = "Recent Projects" if not self.state.projects_from_cache else "Cached Projects"
            section_action = QAction(section, menu)
            section_action.setEnabled(False)
            menu.addAction(section_action)
            self._append_project_items(menu, self.state.projects or [], self.state.usage or {})

        menu.addSeparator()

        settings_action = QAction("aiosd Settings", menu)
        settings_action.triggered.connect(self.open_settings)
        menu.addAction(settings_action)

        open_config_action = QAction("Open Config Directory", menu)
        open_config_action.triggered.connect(self.open_config_directory)
        menu.addAction(open_config_action)

        open_cli = QAction("Open raios", menu)
        open_cli.triggered.connect(self.open_raios_cli)
        menu.addAction(open_cli)

        refresh_action = QAction("Refresh", menu)
        refresh_action.triggered.connect(self.refresh)
        menu.addAction(refresh_action)

        menu.addSeparator()

        quit_action = QAction("Quit", menu)
        quit_action.triggered.connect(self.app.quit)
        menu.addAction(quit_action)

        self.icon.setContextMenu(menu)

    def _append_cached_projects(self, menu: QMenu) -> None:
        projects = self.state.projects or []
        if not projects:
            return
        menu.addSeparator()
        cached = QAction("Cached Projects", menu)
        cached.setEnabled(False)
        menu.addAction(cached)
        self._append_project_items(menu, projects, self.state.usage or {})

    def _append_project_items(self, menu: QMenu, projects: list[dict], usage: dict) -> None:
        for project in projects[:MAX_PROJECTS]:
            name = project.get("name") or Path(project.get("local_path", "")).name or "unknown"
            path = project.get("local_path", "")
            count = usage.get(name, 0)
            label = f"{name} ({count})" if count else name

            submenu = QMenu(label, menu)
            for agent in AGENTS:
                action = QAction(agent.name, submenu)
                action.triggered.connect(
                    lambda checked=False, p=path, a=agent, n=name: self._handle_agent_click(p, a, n)
                )
                submenu.addAction(action)
            menu.addMenu(submenu)

        if projects:
            menu.addSeparator()
            all_projects = QAction(f"All Projects ({len(projects)})", menu)
            all_projects.triggered.connect(self.show_projects_dialog)
            menu.addAction(all_projects)

    def show_projects_dialog(self) -> None:
        self.projects_dialog = ProjectsDialog(None, self.state.projects or [], self.state.usage or {}, self._launch_agent)
        self.projects_dialog.show()
        self.projects_dialog.raise_()
        self.projects_dialog.activateWindow()

    def _handle_agent_click(self, project_path: str, agent: Agent, project_name: str) -> None:
        if self._launch_agent(project_path, agent, project_name):
            return
        self.icon.showMessage(APP_NAME, f"{agent.name} command not found on this machine.")

    def _launch_agent(self, project_path: str, agent: Agent, project_name: str) -> bool:
        launched = launch_agent(project_path, agent, project_name)
        if launched:
            usage = load_usage()
            self.state.usage = usage
            self.rebuild_menu()
        return launched

    def open_settings(self) -> None:
        dialog = SettingsDialog()
        if dialog.exec() != QDialog.Accepted:
            return
        if dialog.restart_requested:
            ok, message = restart_aiosd()
            self.icon.showMessage(APP_NAME, message)
            if not ok:
                QMessageBox.warning(None, APP_NAME, message)
        else:
            self.icon.showMessage(APP_NAME, "Config saved. Restart aiosd to apply worker changes.")
        self.refresh()

    def toggle_daemon(self) -> None:
        ok, message = toggle_aiosd()
        self.icon.showMessage(APP_NAME, message)
        if not ok:
            QMessageBox.warning(None, APP_NAME, message)
        self.refresh()

    def open_raios_cli(self) -> None:
        opened = open_terminal(str(Path.home()), "raios")
        if not opened:
            self.icon.showMessage(APP_NAME, "Unable to open terminal for raios.")

    def open_config_directory(self) -> None:
        if not open_in_file_manager(CONFIG_DIR):
            self.icon.showMessage(APP_NAME, "Unable to open config directory.")


def validate_environment() -> str | None:
    if not QSystemTrayIcon.isSystemTrayAvailable():
        return "System tray is not available in this desktop session."
    return None


def main() -> int:
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setApplicationName(APP_NAME)

    error = validate_environment()
    if error:
        QMessageBox.critical(None, APP_NAME, error)
        return 1

    RaiosTray(app)
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
