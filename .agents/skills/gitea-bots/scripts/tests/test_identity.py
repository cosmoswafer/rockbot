"""Fingerprint tests: code mtimes + effective config, and loop/status drift."""

import os
import tempfile
import unittest
from dataclasses import fields
from pathlib import Path

from gitea_bots.config import Config
from gitea_bots.identity import (
    FINGERPRINT_CONFIG_FIELDS,
    FINGERPRINT_EXCLUDED_FIELDS,
    Fingerprint,
    unclassified_config_fields,
)
from tests.fakes import Harness, make_config


class FingerprintUnitTests(unittest.TestCase):
    # --- classification ------------------------------------------------------
    def test_every_config_field_is_classified(self):
        self.assertEqual(unclassified_config_fields(), set())
        self.assertEqual(
            {field.name for field in fields(Config)},
            set(FINGERPRINT_CONFIG_FIELDS) | set(FINGERPRINT_EXCLUDED_FIELDS),
        )
        self.assertFalse(
            set(FINGERPRINT_CONFIG_FIELDS) & set(FINGERPRINT_EXCLUDED_FIELDS)
        )

    # --- capture / diff ------------------------------------------------------
    def test_diff_names_changed_file_and_config_key(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            script = base / "watch-issues.py"
            module = base / "gitea_bots" / "config.py"
            module.parent.mkdir()
            script.write_text("", encoding="utf-8")
            module.write_text("", encoding="utf-8")
            config = make_config(tmp)

            before = Fingerprint.capture([script, module], config, base_dir=base)
            self.assertEqual(before.diff(before), [])

            stat = script.stat()
            os.utime(script, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))
            config.auto_plan = not config.auto_plan
            after = Fingerprint.capture([script, module], config, base_dir=base)

            self.assertEqual(
                before.diff(after),
                ["config:auto_plan", "file:watch-issues.py"],
            )

    def test_json_roundtrip_is_deterministic(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / "watch-issues.py"
            script.write_text("", encoding="utf-8")
            config = make_config(tmp)
            fingerprint = Fingerprint.capture(
                [script], config, base_dir=Path(tmp)
            )
            self.assertEqual(
                fingerprint.to_json(),
                Fingerprint.capture([script], config, base_dir=Path(tmp)).to_json(),
            )
            self.assertEqual(
                fingerprint, Fingerprint.from_json(fingerprint.to_json())
            )

    def test_json_excludes_the_token_and_raw_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = make_config(tmp)
            config.token = "secret-token"
            config.env = {"GITEA_TOKEN": "secret-token"}
            raw = Fingerprint.capture([], config).to_json()
            self.assertNotIn("secret-token", raw)
            self.assertNotIn("token", raw)

    def test_from_json_rejects_missing_or_malformed_data(self):
        self.assertIsNone(Fingerprint.from_json(""))
        self.assertIsNone(Fingerprint.from_json("not json"))
        self.assertIsNone(Fingerprint.from_json("{}"))
        self.assertIsNone(Fingerprint.from_json('{"files": [], "config": {}}'))


class WatcherFingerprintTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.script = Path(self.tmp.name) / "watch-issues.py"
        self.script.write_text("# watcher\n", encoding="utf-8")
        self.h = Harness(self.tmp.name, script_path=self.script)

    def _set_running(self):
        self.h.watcher._pid_is_watcher = staticmethod(lambda pid: True)
        self.h.config.pid_file.write_text("12345\n", encoding="utf-8")

    def _rewrite_meta(self, **overrides):
        meta = self.h.watcher._read_meta()
        meta.update({key: str(value) for key, value in overrides.items()})
        self.h.config.meta_file.write_text(
            "".join(f"{key}={value}\n" for key, value in meta.items()),
            encoding="utf-8",
        )

    # --- loop drift ----------------------------------------------------------
    def test_clean_poll_is_silent(self):
        self.h.watcher._write_meta()
        self.h.watcher.poll_once(0, notify_armed=False)
        self.h.watcher._check_fingerprint()
        self.assertNotIn("WARN", self.h.stderr)

    def test_config_drift_warns_once_and_names_the_key(self):
        self.h.watcher._write_meta()
        self.h.config.auto_plan = not self.h.config.auto_plan

        self.h.watcher._check_fingerprint()
        self.assertIn("watcher config drift: config:auto_plan", self.h.stderr)
        self.assertIn("stop", self.h.stderr)

        self.h.reset_output()
        self.h.watcher._check_fingerprint()
        self.assertNotIn("WARN", self.h.stderr)

        self.h.config.interval += 1
        self.h.watcher._check_fingerprint()
        self.assertIn("config:interval", self.h.stderr)

    def test_file_drift_warns_once_and_names_the_file(self):
        self.h.watcher._write_meta()
        stat = self.script.stat()
        os.utime(
            self.script,
            ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000),
        )

        self.h.watcher._check_fingerprint()
        self.assertIn("file:watch-issues.py", self.h.stderr)

        self.h.reset_output()
        self.h.watcher._check_fingerprint()
        self.assertNotIn("WARN", self.h.stderr)

    def test_meta_never_contains_the_token(self):
        self.h.watcher._write_meta()
        raw = self.h.config.meta_file.read_text(encoding="utf-8")
        self.assertIn("fingerprint=", raw)
        self.assertNotIn("test-token", raw)

    # --- status --------------------------------------------------------------
    def test_status_is_clean_after_a_doc_only_commit(self):
        self._set_running()
        self.h.watcher._write_meta()
        self._rewrite_meta(script_commit="deadbee")

        self.h.watcher.status()
        self.assertIn("watcher: running", self.h.stdout)
        self.assertNotIn("STALE", self.h.stdout)
        self.assertNotIn("stale-check: unavailable", self.h.stdout)

    def test_status_reports_fingerprint_drift(self):
        self._set_running()
        self.h.watcher._write_meta()
        self.h.config.auto_plan = not self.h.config.auto_plan

        self.h.watcher.status()
        self.assertIn("STALE", self.h.stdout)
        self.assertIn("config:auto_plan", self.h.stdout)

    def test_status_without_a_fingerprint_is_unavailable(self):
        self._set_running()
        self.h.config.meta_file.write_text(
            "pid=12345\nstarted_at=2026-11-08T00:00:00Z\n", encoding="utf-8"
        )

        self.h.watcher.status()
        self.assertIn("stale-check: unavailable", self.h.stdout)
        self.assertNotIn("STALE:", self.h.stdout)

    # --- status: notification model -----------------------------------------
    def test_meta_records_the_resolved_model(self):
        self.h.config.model = "provider/model#variant"
        self.h.watcher._write_meta()
        raw = self.h.config.meta_file.read_text(encoding="utf-8")
        self.assertIn("model=provider/model#variant", raw)
        self.assertNotIn("test-token", raw)

    def test_status_reports_the_model_recorded_in_meta(self):
        self._set_running()
        self.h.watcher._write_meta()
        self._rewrite_meta(model="provider/model#variant")

        self.h.watcher.status()
        self.assertIn("notification model: provider/model#variant", self.h.stdout)

    def test_status_falls_back_to_the_explicit_model_when_stopped(self):
        self.h.config.model = "provider/explicit"

        self.h.watcher.status()
        self.assertIn("watcher: stopped", self.h.stdout)
        self.assertIn("notification model: provider/explicit", self.h.stdout)

    def test_status_falls_back_to_the_last_logged_model(self):
        self.h.config.effective_log_file.write_text(
            "notification model: provider/logged (session: ses_x)\n",
            encoding="utf-8",
        )

        self.h.watcher.status()
        self.assertIn("notification model: provider/logged", self.h.stdout)

    def test_status_says_unresolved_when_no_model_is_known(self):
        self.h.watcher.status()
        self.assertIn("notification model: unresolved", self.h.stdout)


if __name__ == "__main__":
    unittest.main()
