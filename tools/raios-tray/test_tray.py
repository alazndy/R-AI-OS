import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


TRAY_FILE = Path(__file__).with_name("raios-tray.py")


def load_tray_module():
    spec = importlib.util.spec_from_file_location("raios_tray_under_test", TRAY_FILE)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class TrayConfigTests(unittest.TestCase):
    def test_save_preserves_unknown_top_level_sections(self) -> None:
        tray = load_tray_module()
        with tempfile.TemporaryDirectory() as directory:
            config_path = Path(directory) / "config.toml"
            config_path.write_text(
                "system_name = \"before\"\n\n[daemon]\nhealth_interval_secs = 15\n"
                "\n[bootstrap]\nenabled = true\n\n[factory]\nenabled = false\n",
                encoding="utf-8",
            )
            config = tray.default_raios_config()
            config["system_name"] = "after"
            config["daemon"]["health_interval_secs"] = 60

            tray.save_raios_config(config, config_path)

            saved = config_path.read_text(encoding="utf-8")
            self.assertIn("[bootstrap]\nenabled = true", saved)
            self.assertIn("[factory]\nenabled = false", saved)
            self.assertIn('system_name = "after"', saved)
            self.assertIn("health_interval_secs = 60", saved)

    def test_missing_key_lands_in_its_own_section_not_the_last_one(self) -> None:
        tray = load_tray_module()
        result = tray._toml_upsert(
            "[daemon]\nport = 1\n\n[factory]\nenabled = true\n",
            "daemon",
            "refresh_secs",
            "15",
        )
        head, _, tail = result.partition("[factory]")
        self.assertIn("refresh_secs = 15", head)
        self.assertNotIn("refresh_secs", tail)

    def test_a_commented_section_header_is_found_without_duplicating_the_table(self) -> None:
        tray = load_tray_module()
        result = tray._toml_upsert("[daemon] # local\nport = 1\n", "daemon", "port", "2")
        self.assertEqual(result.count("[daemon]"), 1, "a commented header must not duplicate")
        self.assertIn("port = 2", result)

    def test_a_global_key_is_inserted_before_the_first_section(self) -> None:
        tray = load_tray_module()
        result = tray._toml_upsert("[daemon] # c\nport = 1\n", None, "db_path", '"x"')
        self.assertLess(result.find("db_path"), result.find("[daemon]"))

    def test_desktop_identity_is_set_before_application_creation(self) -> None:
        source = TRAY_FILE.read_text(encoding="utf-8")
        self.assertLess(
            source.index('QApplication.setDesktopFileName("raios-tray")'),
            source.index("app = QApplication(sys.argv)"),
        )

    def test_tray_controller_is_owned_by_the_application(self) -> None:
        source = TRAY_FILE.read_text(encoding="utf-8")
        self.assertIn("super().__init__(app)", source)


class TrayTaskApiTests(unittest.TestCase):
    def test_load_tasks_uses_canonical_api_payload(self) -> None:
        tray = load_tray_module()
        payload = {
            "status": "ok",
            "tasks": [{"id": "task-1", "text": "Ship tray", "completed": False}],
        }
        with patch.object(tray, "api_get", return_value=payload) as api_get:
            tasks = tray.load_tasks("session-token")

        self.assertEqual(tasks, payload["tasks"])
        api_get.assert_called_once_with("/api/tasks", "session-token")

    def test_task_mutation_uses_typed_control_plane_command(self) -> None:
        tray = load_tray_module()
        with patch.object(
            tray,
            "api_post",
            return_value={"status": "ok", "result": {"task_id": "task-1"}},
        ) as api_post:
            ok, _ = tray.create_task("Ship tray", "/workspace/raios", "session-token")

        self.assertTrue(ok)
        path, token, payload = api_post.call_args.args
        self.assertEqual(path, "/api/v1/control/command")
        self.assertEqual(token, "session-token")
        self.assertEqual(payload["command_type"], "CreateTask")
        self.assertEqual(payload["payload"]["title"], "Ship tray")
        self.assertEqual(payload["payload"]["project_path"], "/workspace/raios")
        self.assertTrue(payload["payload"]["idempotency_key"])


class TrayDirtyStateTests(unittest.TestCase):
    def test_dirty_state_is_not_cached_after_a_worktree_change(self) -> None:
        tray = load_tray_module()
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / ".git").mkdir()
            (repo / ".git" / "HEAD").write_text("ref: refs/heads/main\n", encoding="utf-8")
            with patch.object(tray.subprocess, "run") as run:
                run.return_value.stdout = ""
                self.assertFalse(tray.check_git_dirty(str(repo)))
                run.return_value.stdout = "?? changed.txt\n"
                self.assertTrue(tray.check_git_dirty(str(repo)))


if __name__ == "__main__":
    unittest.main()
