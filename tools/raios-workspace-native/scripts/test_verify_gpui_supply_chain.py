#!/usr/bin/env python3
"""Offline tests for the supply-chain script's decision logic (no network).

Run: python3 -m unittest tools/raios-workspace-native/scripts/test_verify_gpui_supply_chain.py
"""
import hashlib
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import verify_gpui_supply_chain as v  # noqa: E402

BODY = b"reviewed content"
GOOD = hashlib.sha256(BODY).hexdigest()
PUBLISHED = {"f.rs": BODY}


def verdict(entry, upstream=None, version="0.3.8"):
    return v.allowlist_verdict(entry, version, "f.rs", PUBLISHED, upstream or {})


class AllowlistVerdict(unittest.TestCase):
    def test_a_file_without_an_entry_is_never_allowed(self):
        self.assertEqual(verdict(None), (False, None))

    def test_a_matching_hash_is_allowed(self):
        self.assertTrue(verdict({"sha256": GOOD, "reason": "r"})[0])

    def test_changed_content_in_a_new_release_is_not_allowed(self):
        allowed, why = verdict({"sha256": "0" * 64, "reason": "r"})
        self.assertFalse(allowed)
        self.assertIn("reviewed hash", why)

    def test_an_entry_for_another_version_is_not_allowed(self):
        self.assertFalse(verdict({"sha256": GOOD, "version": "9.9.9", "reason": "r"})[0])

    def test_a_verbatim_upstream_copy_is_allowed(self):
        entry = {"copy_of": "crates/x/src/f.rs", "reason": "r"}
        self.assertTrue(verdict(entry, {"crates/x": {"src/f.rs": BODY}})[0])

    def test_a_modified_copy_is_not_allowed(self):
        entry = {"copy_of": "crates/x/src/f.rs", "reason": "r"}
        self.assertFalse(verdict(entry, {"crates/x": {"src/f.rs": b"other"}})[0])

    def test_a_copy_whose_upstream_file_is_missing_is_not_allowed(self):
        entry = {"copy_of": "crates/x/src/f.rs", "reason": "r"}
        self.assertFalse(verdict(entry, {"crates/x": {}})[0])


class Comparison(unittest.TestCase):
    def test_changed_and_added_files_are_reported(self):
        changed, added = v.compare(
            {"a": b"1", "b": b"2", "c": b"3"}, {"a": b"1", "b": b"X", "d": b"4"}
        )
        self.assertEqual(changed, ["b"])
        self.assertEqual(added, ["c"])

    def test_manifest_and_license_files_are_ignored(self):
        for name in ["Cargo.toml", "Cargo.lock", ".cargo_vcs_info.json", "LICENSE-APACHE"]:
            self.assertTrue(v.is_ignored(name), name)
        self.assertFalse(v.is_ignored("src/lib.rs"))


class Mapping(unittest.TestCase):
    def test_zed_directory_candidates_cover_nested_layouts(self):
        self.assertEqual(v.candidate_zed_dirs("gpui-pre"), ["crates/gpui"])
        derive = v.candidate_zed_dirs("gpui-pre-derive-refineable")
        self.assertIn("crates/refineable/derive_refineable", derive)
        self.assertIn("tooling/perf", v.candidate_zed_dirs("gpui-pre-perf"))


class AllowlistFile(unittest.TestCase):
    def test_an_entry_needs_exactly_one_pin(self):
        import tempfile

        with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as handle:
            handle.write('[[allow]]\ncrate = "c"\npath = "p"\nreason = "r"\n')
        with self.assertRaises(SystemExit):
            v.load_allowlist(handle.name)


if __name__ == "__main__":
    unittest.main()
