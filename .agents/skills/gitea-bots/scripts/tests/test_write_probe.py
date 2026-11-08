"""Offline tests for the write-data probe's guarded lifecycle.

The real server is only contacted by ``tests/test_live_write_gitea.py`` (opt-in)
or ``scripts/probe-live-write.py``; this module proves the flow, the target
guard and the cleanup/failure handling against the shared ``FakeClient``.
"""

from __future__ import annotations

import io
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from gitea_bots.errors import GiteaError
from gitea_bots.log import Logger
from gitea_bots.write_probe import (
    PROBE_STATE,
    TITLE_MARKER,
    WriteProbe,
    build_write_probe,
)
from tests.fakes import FakeClient, make_config


class ForbiddenDeleteClient(FakeClient):
    """A fake whose token may not delete issues (non-admin deployment)."""

    def delete(self, path, payload=None):
        raise GiteaError(f"HTTP 403 from DELETE {path} — forbidden")


class WriteProbeTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.config = make_config(
            self.root / "state",
            repo_root=self.root,
            self_login="testbot",
        )
        self.logger = Logger(verbose=False, out=io.StringIO(), err=io.StringIO())

    def probe(self, client=None):
        return WriteProbe(
            self.config,
            client or FakeClient(),
            self.logger,
            lock_path=self.root / "lock",
        )

    def test_happy_path_creates_exercises_and_deletes_one_issue(self):
        client = FakeClient()
        client.labels = [
            {"id": 7, "name": "tooling", "color": "0e8a16", "description": ""}
        ]
        probe = self.probe(client)

        report = probe.run()
        rendered = report.render()

        self.assertTrue(report.ok, rendered)
        self.assertIn("[PASS] write-data lifecycle", rendered)
        self.assertTrue(probe.cleaned_up)
        self.assertEqual(probe.leftover_url, "")
        self.assertNotIn(
            probe.number, [issue["number"] for issue in client.issues]
        )

        methods = [method for method, _path, _payload in client.writes]
        self.assertEqual(
            methods,
            ["POST", "POST", "PUT", "PATCH", "PATCH", "DELETE"],
        )
        for method, path, _payload in client.writes:
            if method == "POST" and path.endswith("/issues"):
                continue
            self.assertIn(f"/issues/{probe.number}", path)

    def test_temp_label_is_created_and_removed_on_a_label_less_repo(self):
        client = FakeClient()
        probe = self.probe(client)

        report = probe.run()

        self.assertTrue(report.ok, report.render())
        self.assertEqual(probe.temp_label_id, 0)
        self.assertEqual(client.labels, [])
        self.assertTrue(
            any(
                method == "POST" and path.endswith("/labels")
                for method, path, _payload in client.writes
            )
        )
        self.assertTrue(
            any(
                method == "DELETE" and "/labels/" in path
                for method, path, _payload in client.writes
            )
        )

    def test_cleanup_still_deletes_after_a_mid_run_failure(self):
        class FailingProbe(WriteProbe):
            def _assign(self, scenario):
                raise RuntimeError("assign exploded")

        client = FakeClient()
        probe = FailingProbe(
            self.config, client, self.logger, lock_path=self.root / "lock"
        )

        report = probe.run()

        self.assertFalse(report.ok)
        self.assertEqual(report.scenarios[0].error, "assign exploded")
        self.assertTrue(probe.cleaned_up)
        self.assertNotIn(
            probe.number, [issue["number"] for issue in client.issues]
        )
        self.assertTrue(
            any(
                method == "DELETE" and path.endswith(f"/issues/{probe.number}")
                for method, path, _payload in client.writes
            )
        )

    def test_forbidden_delete_neutralises_the_issue_and_fails_loudly(self):
        client = ForbiddenDeleteClient()
        probe = self.probe(client)

        report = probe.run()
        rendered = report.render()

        self.assertFalse(report.ok)
        self.assertFalse(probe.cleaned_up)
        self.assertIn(f"/issues/{probe.number}", probe.leftover_url)
        self.assertIn("leftover", rendered)

        leftover = next(
            issue for issue in client.issues if issue["number"] == probe.number
        )
        self.assertEqual(leftover["state"], "closed")
        self.assertEqual(leftover["assignees"], [])
        self.assertEqual(leftover["labels"], [])
        check = next(
            check
            for check in report.scenarios[0].checks
            if check.name == "dummy issue neutralised after a forbidden delete"
        )
        self.assertFalse(check.ok)

    def test_target_guard_refuses_before_the_dummy_exists(self):
        probe = self.probe()

        with self.assertRaises(AssertionError):
            probe._target_path()

    def test_build_write_probe_applies_overrides_and_resolves_login(self):
        self.config.notify_enabled = True
        self.config.spawn_workers = True
        self.config.auto_plan_workers = True
        self.config.start_reconcile = True
        self.config.all_comments = True
        self.config.verbose = True

        class FakeLoginClient:
            def __init__(self, *args, **kwargs):
                pass

            def resolve_login(self):
                return "testbot"

        with mock.patch(
            "gitea_bots.write_probe.GiteaClient", FakeLoginClient
        ):
            probe = build_write_probe(self.config, self.logger)

        self.assertFalse(self.config.notify_enabled)
        self.assertFalse(self.config.spawn_workers)
        self.assertFalse(self.config.auto_plan_workers)
        self.assertFalse(self.config.start_reconcile)
        self.assertFalse(self.config.all_comments)
        self.assertFalse(self.config.verbose)
        self.assertEqual(self.config.self_login, "testbot")
        self.assertEqual(
            self.config.state_dir,
            (self.config.repo_root / PROBE_STATE).resolve(),
        )
        self.assertEqual(
            probe.lock_path, self.config.repo_root / PROBE_STATE / "lock"
        )
        self.assertEqual(probe.title, "")
        self.assertTrue(TITLE_MARKER)


if __name__ == "__main__":
    unittest.main()
