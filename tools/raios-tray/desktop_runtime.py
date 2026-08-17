"""Desktop-session capability detection kept independent from Qt and GTK."""

from __future__ import annotations

import os
from dataclasses import dataclass
from enum import Enum
from typing import Mapping


class DesktopKind(str, Enum):
    BUDGIE = "budgie"
    GNOME = "gnome"
    PLASMA = "plasma"
    OTHER = "other"


@dataclass(frozen=True)
class DesktopSession:
    kind: DesktopKind
    name: str
    is_wayland: bool


def detect_desktop_session(environ: Mapping[str, str] | None = None) -> DesktopSession:
    env = os.environ if environ is None else environ
    desktop = env.get("XDG_CURRENT_DESKTOP", "").lower()
    session_type = env.get("XDG_SESSION_TYPE", "").lower()

    if "budgie" in desktop:
        kind = DesktopKind.BUDGIE
    elif "plasma" in desktop or "kde" in desktop:
        kind = DesktopKind.PLASMA
    elif "gnome" in desktop or "ubuntu" in desktop:
        kind = DesktopKind.GNOME
    else:
        kind = DesktopKind.OTHER

    return DesktopSession(kind=kind, name=desktop or "unknown", is_wayland=session_type == "wayland")


def tray_host_guidance(session: DesktopSession) -> str:
    if session.kind is DesktopKind.BUDGIE:
        return "Enable Budgie's StatusNotifier/Indicator applet, then restart raios-tray."
    if session.kind is DesktopKind.GNOME:
        return "Enable a GNOME StatusNotifier/AppIndicator host, then restart raios-tray."
    if session.kind is DesktopKind.PLASMA:
        return "Enable Plasma's System Tray widget, then restart raios-tray."
    return "Enable a StatusNotifier-compatible system tray, then restart raios-tray."
