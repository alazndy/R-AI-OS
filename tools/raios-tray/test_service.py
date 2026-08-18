from pathlib import Path
import unittest


SERVICE_FILE = Path(__file__).with_name("raios-tray.service")


class TrayServiceLifecycleTests(unittest.TestCase):
    def test_service_is_owned_by_the_graphical_session_target(self) -> None:
        content = SERVICE_FILE.read_text(encoding="utf-8")
        unit_section, install_section = content.split("[Install]", maxsplit=1)

        self.assertIn("After=graphical-session.target", unit_section)
        self.assertIn("PartOf=graphical-session.target", unit_section)
        self.assertIn("WantedBy=graphical-session.target", install_section)
        self.assertNotIn("WantedBy=default.target", install_section)


if __name__ == "__main__":
    unittest.main()
