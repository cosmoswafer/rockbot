"""Offline tests for the live probe's report assembly and scenario logic.

The scenarios are exercised against the shared ``FakeClient`` with canned API
shapes, so this module proves the probe's pass/fail propagation without network
access. The real server is only contacted by ``tests/test_live_gitea.py`` (opt-in)
or ``scripts/probe-live.py``.
"""

from __future__ import annotations

import io
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from gitea_bots.live_probe import (
    PROBE_STATE,
    LiveProbe,
    ReadOnlyClient,
    build_probe,
)
from gitea_bots.log import Logger
from tests.fakes import FakeClient, comment, issue, make_config


class LiveProbeReportTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.config = make_config(self.root / "state")
        self.logger = Logger(verbose=False, out=io.StringIO(), err=io.StringIO())
        self.client = FakeClient()

    def probe(self, issue_limit=10):
        return LiveProbe(
            self.config, self.client, self.logger, issue_limit=issue_limit
        )

    def _populate_passing_scenarios(self):
        # #1 is an AUTO PLAN candidate: assigned, Summary-only, no comments.
        # #2 carries the comment/timeline shapes the other scenarios read.
        self.client.issues = [
            issue(1, title="Plan me", assignees=["testbot"]),
            issue(2, title="With comments"),
        ]
        self.client.comments = {
            2: [
                comment(10, "please @testbot take a look", author="alice", num=2),
                comment(
                    11,
                    "<!-- gitea-bots: stage=2 -->\nbot output",
                    author="saru",
                    num=2,
                ),
                comment(12, "```\n@testbot\n```", author="alice", num=2),
                comment(13, "plain update, no mention", author="alice", num=2),
            ]
        }
        self.client.timeline = {
            2: [
                {"id": 20, "type": "assignees",
                 "assignee": {"login": "testbot"}},
                {"id": 21, "type": "comment"},
            ]
        }
        self.client.labels = [
            {"id": 1, "name": "👀 seeing", "color": "1d76db",
             "description": "picked up"},
        ]

    def test_report_assembles_passing_scenarios(self):
        self._populate_passing_scenarios()

        report = self.probe().run()

        self.assertEqual(
            [scenario.name for scenario in report.scenarios],
            [
                "AUTO PLAN baseline poll",
                "DIRECTED COMMENT classification",
                "REASSIGNED timeline shape",
                "status label shape",
            ],
        )
        self.assertTrue(report.ok, report.render())
        self.assertIn("4/4 scenario(s) passed", report.render())
        self.assertIn(
            "1 directed mention(s)", " ".join(report.scenarios[1].notes)
        )
        self.assertIn(
            "1 assignee event(s)", " ".join(report.scenarios[2].notes)
        )

    def test_failed_check_fails_the_report(self):
        self.client.issues = []

        report = self.probe().run()

        auto_plan = report.scenarios[0]
        self.assertFalse(auto_plan.ok)
        self.assertFalse(report.ok)
        self.assertIn("[FAIL] AUTO PLAN baseline poll", report.render())
        checks = {check.name: check for check in auto_plan.checks}
        self.assertTrue(checks["baseline poll succeeds"].ok)
        self.assertFalse(checks["open issues were scanned"].ok)
        self.assertFalse(checks["baseline poll emitted events"].ok)
        self.assertFalse(checks["issue-level events present"].ok)

    def test_directed_and_timeline_classification_on_canned_data(self):
        self.client.issues = [issue(1)]
        self.client.comments = {
            1: [comment(10, "please @testbot look", author="alice", num=1)]
        }
        self.client.timeline = {
            1: [{"id": 5, "type": "assignees",
                 "assignee": {"login": "testbot"}}]
        }

        report = self.probe().run()

        directed = report.scenarios[1]
        timeline = report.scenarios[2]
        self.assertTrue(directed.ok, directed.checks)
        self.assertTrue(timeline.ok, timeline.checks)
        self.assertIn("1 comment(s) scanned", " ".join(directed.notes))
        self.assertIn("1 directed mention(s)", " ".join(directed.notes))
        self.assertIn("1 timeline event(s) scanned", " ".join(timeline.notes))
        self.assertIn("1 assignee event(s)", " ".join(timeline.notes))

    def test_malformed_timeline_event_fails_its_check(self):
        self.client.issues = [issue(1)]
        self.client.timeline = {1: [{"id": "not-an-int", "type": "assignees"}]}

        report = self.probe().run()

        timeline = report.scenarios[2]
        self.assertFalse(timeline.ok)
        check = next(
            check for check in timeline.checks
            if check.name == "timeline events have the expected shape"
        )
        self.assertFalse(check.ok)
        self.assertIn("unexpected shape", check.detail)

    def test_scenario_error_is_collected_without_aborting(self):
        class ExplodingClient(FakeClient):
            def get_all(self, path, params=None, limit=50, max_pages=20):
                if path.endswith("/labels"):
                    raise RuntimeError("label endpoint exploded")
                return super().get_all(path, params, limit, max_pages)

        self.client = ExplodingClient()
        self.client.issues = [issue(1)]
        report = self.probe().run()

        self.assertEqual(len(report.scenarios), 4)
        labels = report.scenarios[3]
        self.assertFalse(labels.ok)
        self.assertEqual(labels.error, "label endpoint exploded")
        self.assertIn("label endpoint exploded", report.render())

    def test_read_only_client_refuses_writes(self):
        client = ReadOnlyClient("https://gitea.test/api/v1", "token")
        with self.assertRaises(AssertionError):
            client.request("POST", "/repos/Atom/example/issues")

    def test_build_probe_applies_the_read_only_overrides(self):
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
            "gitea_bots.live_probe.ReadOnlyClient", FakeLoginClient
        ):
            probe = build_probe(self.config, self.logger, issue_limit=3)

        self.assertFalse(self.config.notify_enabled)
        self.assertFalse(self.config.spawn_workers)
        self.assertFalse(self.config.auto_plan_workers)
        self.assertFalse(self.config.start_reconcile)
        self.assertFalse(self.config.all_comments)
        self.assertFalse(self.config.verbose)
        self.assertEqual(self.config.self_login, "testbot")
        self.assertEqual(probe.issue_limit, 3)
        self.assertEqual(
            self.config.state_dir,
            (self.config.repo_root / PROBE_STATE).resolve(),
        )


if __name__ == "__main__":
    unittest.main()
