"""Offline tests for the end-to-end watcher probe.

The real server is only contacted by ``tests/test_live_e2e_gitea.py`` (opt-in)
or ``scripts/probe-e2e.py``; this module proves the sequence, the guards, the
cleanup/failure handling and the event parser against the shared ``FakeClient``.
"""

from __future__ import annotations

import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from gitea_bots import e2e_probe
from gitea_bots.e2e_probe import (
    PROBE_STATE,
    E2EProbe,
    parse_event_tags,
    report_to_dict,
    write_report_artifact,
)
from gitea_bots.errors import GiteaError
from gitea_bots.live_probe import Check, ProbeReport, ScenarioResult
from gitea_bots.locks import FileLock
from gitea_bots.log import Logger
from tests.fakes import FakeClient, make_config


class ForbiddenDeleteClient(FakeClient):
    """A fake whose token may not delete issues (non-admin deployment)."""

    def delete(self, path, payload=None):
        raise GiteaError(f"HTTP 403 from DELETE {path} — forbidden")


class TitleMismatchClient(FakeClient):
    """Pretend the server rewrote the title, so the marker guard must abort."""

    def post(self, path, payload=None, params=None):
        result = super().post(path, payload=payload, params=params)
        if path.endswith("/issues"):
            self.issues[-1]["title"] = "unexpected title"
        return result


class E2EProbeTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.config = make_config(
            self.root / "state",
            repo_root=self.root,
            self_login="testbot",
            auto_plan=True,
            shared_login=True,
        )
        self.logger = Logger(verbose=False, out=io.StringIO(), err=io.StringIO())

    def probe(self, client=None, **kwargs):
        return E2EProbe(
            self.config,
            client or FakeClient(raise_missing_issue=True),
            self.logger,
            lock_path=self.root / "lock",
            production_state_dir=self.root / "production",
            **kwargs,
        )

    # ------------------------------------------------------------- happy path
    def test_full_sequence_emits_expected_events_labels_and_cleans_up(self):
        client = FakeClient(raise_missing_issue=True)
        probe = self.probe(client)

        report = probe.run()
        rendered = report.render()

        self.assertTrue(report.ok, rendered)
        self.assertTrue(probe.cleaned_up, rendered)
        self.assertEqual(probe.leftover_urls, [])
        self.assertEqual(len(probe.created), 8)
        self.assertEqual(len(probe.fixtures), 8)

        # Events are parsed per fixture, not from a bare substring.
        self.assertIn("AUTO PLAN", probe.poll1_tags[probe.fixtures["A"]])
        self.assertIn(
            "DIRECTED COMMENT", probe.poll1_tags[probe.fixtures["E"]]
        )
        self.assertIn(
            "DIRECTED COMMENT", probe.poll1_tags[probe.fixtures["I"]]
        )
        self.assertNotIn(
            "DIRECTED COMMENT", probe.poll1_tags[probe.fixtures["F"]]
        )

        # The labels scenario asserted the real projected sets while the
        # fixtures still existed (cleanup runs afterwards).
        labels = probe.scenarios["labels"]
        self.assertTrue(labels.ok, labels.checks)
        self.assertEqual(len(labels.checks), 16)

        # Cleanup really removed every dummy (and the temporary labels).
        self.assertEqual(client.issues, [])
        self.assertEqual(client.labels, [])
        deleted = {
            path
            for method, path, _payload in client.writes
            if method == "DELETE"
        }
        for number in probe.created:
            self.assertIn(
                f"/repos/Atom/example/issues/{number}", deleted
            )

    def test_report_contains_one_scenario_per_case(self):
        probe = self.probe()
        report = probe.run()

        self.assertTrue(report.ok, report.render())
        names = [scenario.name for scenario in report.scenarios]
        for key in ("A", "B", "C", "D", "E", "F", "G", "H", "I"):
            self.assertTrue(
                any(name.startswith(f"case {key}:") for name in names),
                f"missing case {key} in {names}",
            )
        self.assertIn("baseline poll is silent", names)
        self.assertIn("second poll is idempotent", names)
        self.assertIn("real status label reconcile", names)
        self.assertIn("cleanup", names)

    # ------------------------------------------------------------------ guards
    def test_refuses_when_the_production_watcher_is_live(self):
        client = FakeClient(raise_missing_issue=True)
        probe = self.probe(client)
        with mock.patch.object(
            E2EProbe, "_watcher_running", return_value=True
        ):
            report = probe.run()

        self.assertFalse(report.ok)
        self.assertIn("production watcher is stopped", report.render())
        self.assertIn("watch-issues.py stop", report.render())
        self.assertEqual(client.writes, [])
        self.assertEqual(probe.created, [])

    def test_refuses_an_overlapping_run_without_touching_anything(self):
        client = FakeClient(raise_missing_issue=True)
        probe = self.probe(client)
        holder = FileLock(self.root / "lock")
        self.assertTrue(holder.acquire())
        try:
            report = probe.run()
        finally:
            holder.release()

        self.assertFalse(report.ok)
        self.assertIn("another end-to-end probe holds", report.render())
        self.assertEqual(client.writes, [])
        self.assertEqual(probe.created, [])

    def test_target_guard_refuses_before_the_dummy_exists(self):
        probe = self.probe()
        with self.assertRaises(AssertionError):
            probe._issue_path(1)

    def test_title_marker_mismatch_aborts_and_still_cleans_up(self):
        client = TitleMismatchClient(raise_missing_issue=True)
        probe = self.probe(client)

        report = probe.run()

        self.assertFalse(report.ok)
        self.assertIn("title marker mismatch", report.render())
        self.assertTrue(probe.cleaned_up, report.render())
        self.assertEqual(client.issues, [])
        self.assertEqual(len(probe.created), 1)

    # -------------------------------------------------------------- failures
    def test_cleanup_still_deletes_after_a_mid_run_failure(self):
        class FailingProbe(E2EProbe):
            def _idempotence(self):
                raise RuntimeError("idempotence exploded")

        client = FakeClient(raise_missing_issue=True)
        probe = FailingProbe(
            self.config,
            client,
            self.logger,
            lock_path=self.root / "lock",
            production_state_dir=self.root / "production",
        )

        report = probe.run()

        self.assertFalse(report.ok)
        self.assertIn("idempotence exploded", report.render())
        self.assertTrue(probe.cleaned_up, report.render())
        self.assertEqual(client.issues, [])
        self.assertEqual(client.labels, [])

    def test_forbidden_delete_neutralises_every_dummy_and_fails_loudly(self):
        client = ForbiddenDeleteClient(raise_missing_issue=True)
        probe = self.probe(client)

        report = probe.run()
        rendered = report.render()

        self.assertFalse(report.ok)
        self.assertFalse(probe.cleaned_up)
        self.assertEqual(len(probe.leftover_urls), len(probe.created))
        self.assertIn("leftover", rendered)
        for issue in client.issues:
            self.assertEqual(issue["state"], "closed")
            self.assertEqual(issue["assignees"], [])
            self.assertEqual(issue["labels"], [])

    # ---------------------------------------------------------------- helpers
    def test_parse_event_tags_is_anchored_and_ignores_quoted_text(self):
        text = "\n".join([
            '2026-11-08T10:00:00Z AUTO PLAN #7 "title" by alice — https://x/7',
            "2026-11-08T10:00:01Z DIRECTED COMMENT on #8 (comment 3) "
            "by alice — https://x/8",
            "2026-11-08T10:00:02Z DIRECTED COMMENT (catch-up) on #9 "
            "(comment 4) by alice — https://x/9",
            "2026-11-08T10:00:03Z DIRECTED COMMENT on #11 (issue body) "
            "by alice — https://x/11",
            '2026-11-08T10:00:03Z NEW ISSUE #10 "AUTO PLAN #11 in the title" '
            "by alice — https://x/10",
            "    | 2026-11-08T10:00:00Z AUTO PLAN #12 from a quoted body",
            "AUTO PLAN #13 without a timestamp",
        ])

        tags = parse_event_tags(text)

        self.assertEqual(tags[7], ["AUTO PLAN"])
        self.assertEqual(tags[8], ["DIRECTED COMMENT"])
        self.assertEqual(tags[9], ["DIRECTED COMMENT"])
        self.assertEqual(tags[11], ["DIRECTED COMMENT"])
        self.assertEqual(tags[10], ["NEW ISSUE"])
        self.assertNotIn(12, tags)
        self.assertNotIn(13, tags)

    def test_report_artifact_writes_log_and_json(self):
        report = ProbeReport(scenarios=[
            ScenarioResult("s", checks=[Check("c", True, "ok")])
        ])

        paths = write_report_artifact(report, self.config)

        self.assertTrue(paths["log"].is_file())
        self.assertTrue(paths["json"].is_file())
        self.assertIn("[PASS] s", paths["log"].read_text(encoding="utf-8"))
        data = json.loads(paths["json"].read_text(encoding="utf-8"))
        self.assertTrue(data["ok"])
        self.assertEqual(data["scenarios"][0]["checks"][0]["name"], "c")
        self.assertEqual(report_to_dict(report)["scenarios"][0]["name"], "s")
        self.assertTrue(str(paths["log"]).startswith(str(self.root)))
        self.assertIn("tmp", paths["log"].parts)

    def test_build_e2e_probe_applies_overrides_and_resolves_login(self):
        self.config.notify_enabled = True
        self.config.spawn_workers = True
        self.config.auto_plan_workers = True
        self.config.start_reconcile = True
        self.config.all_comments = True
        self.config.verbose = True
        self.config.auto_plan = False
        self.config.shared_login = False

        class FakeLoginClient:
            def __init__(self, *args, **kwargs):
                pass

            def resolve_login(self):
                return "testbot"

        with mock.patch.object(
            e2e_probe, "GiteaClient", FakeLoginClient
        ):
            probe = e2e_probe.build_e2e_probe(self.config, self.logger)

        self.assertFalse(self.config.notify_enabled)
        self.assertFalse(self.config.spawn_workers)
        self.assertFalse(self.config.auto_plan_workers)
        self.assertFalse(self.config.start_reconcile)
        self.assertFalse(self.config.all_comments)
        self.assertFalse(self.config.verbose)
        self.assertTrue(self.config.auto_plan)
        self.assertTrue(self.config.shared_login)
        self.assertEqual(self.config.self_login, "testbot")
        self.assertEqual(
            self.config.state_dir,
            (self.config.repo_root / PROBE_STATE).resolve(),
        )
        self.assertEqual(
            probe.production_state_dir, self.root / "state"
        )
        self.assertEqual(
            probe.lock_path, self.config.repo_root / "tmp" / "gitea-bots-live-e2e.lock"
        )


if __name__ == "__main__":
    unittest.main()
