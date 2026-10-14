from pathlib import Path
import unittest


SERVICE_FILE = Path(__file__).with_name("raios-tray.service")
DESKTOP_FILE = Path(__file__).with_name("raios-tray.desktop")


class TrayServiceLifecycleTests(unittest.TestCase):
    def test_service_is_owned_by_the_graphical_session_target(self) -> None:
        content = SERVICE_FILE.read_text(encoding="utf-8")
        unit_section, install_section = content.split("[Install]", maxsplit=1)

        self.assertIn("After=graphical-session.target", unit_section)
        self.assertIn("PartOf=graphical-session.target", unit_section)
        self.assertIn("WantedBy=graphical-session.target", install_section)
        self.assertNotIn("WantedBy=default.target", install_section)

    def test_service_installs_the_portal_desktop_identity(self) -> None:
        content = SERVICE_FILE.read_text(encoding="utf-8")
        desktop_entry = DESKTOP_FILE.read_text(encoding="utf-8")

        self.assertIn("ExecStartPre=/usr/bin/install -Dm644", content)
        self.assertIn("raios-tray.desktop", content)
        self.assertIn("Name=R-AI-OS Tray", desktop_entry)
        self.assertIn("Exec=/home/alaz/dev/core/R-AI-OS/tools/raios-tray/.venv/bin/python", desktop_entry)


if __name__ == "__main__":
    unittest.main()
