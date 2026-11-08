"""Tests for the workflow status adapter and task-list sync."""

import tempfile
import unittest
from pathlib import Path

from gitea_bots.rules import STAGE2_MARKER, STAGE3_MARKER
from tests.fakes import Harness, comment, issue


class StatusReconcileTests(unittest.TestCase):
    """The workflow adapter turns stage markers into the shared label set."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_reconcile_derives_responded_from_stage2_marker(self):
        self.h.client.issues = [issue(8)]
        self.h.client.comments[8] = [
            comment(800, f"## Findings\n\nPlan.\n\n{STAGE2_MARKER}", "testbot", 8)
        ]
        changed, text, expect = self.h.status.reconcile(8)
        self.assertEqual(expect, frozenset({"responded"}))
        self.assertTrue(changed)
        self.assertIn("→ responded", text)

    def test_reconcile_derives_responded_and_committed_from_stage3_marker(self):
        self.h.client.issues = [issue(8)]
        self.h.client.comments[8] = [
            comment(800, f"Done.\n\n{STAGE3_MARKER}", "testbot", 8)
        ]
        _, _, expect = self.h.status.reconcile(8)
        self.assertEqual(expect, frozenset({"responded", "committed"}))

    def test_startup_reconcile_logs_progress(self):
        h = Harness(self.tmp.name, start_reconcile=True)
        h.client.issues = [issue(8, assignees=["testbot"])]
        h.client.comments[8] = [
            comment(800, f"## Findings\n\nPlan.\n\n{STAGE2_MARKER}", "testbot", 8)
        ]
        h.watcher.startup_label_reconcile()
        self.assertIn("startup label reconcile: #8 → responded", h.stdout)
        self.assertIn("checked 1 issue(s), updated 1, failed 0", h.stdout)


class TaskSyncTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)
        self.plan = Path(self.tmp.name) / "plan.md"
        self.plan.write_text(
            "## Findings\n\nstuff\n\n## Tasks\n\n- [ ] one\n- [x] two\n\n"
            "## Code sketch\n\n```\ncode\n```\n",
            encoding="utf-8",
        )

    def tearDown(self):
        self.tmp.cleanup()

    def test_sync_writes_body_and_is_idempotent(self):
        self.h.client.issues = [issue(5, body="## Summary\n\nHi.\n")]
        self.h.tasks.sync(5, self.plan)
        patches = [w for w in self.h.client.writes if w[0] == "PATCH"]
        self.assertEqual(len(patches), 1)
        body = patches[0][2]["body"]
        self.assertIn("## Summary", body)
        self.assertIn("- [ ] one", body)
        self.assertIn("- [x] two", body)
        # Second run is a no-op.
        self.h.tasks.sync(5, self.plan)
        self.assertEqual(len([w for w in self.h.client.writes if w[0] == "PATCH"]), 1)
        self.assertIn("already in sync", self.h.stdout)

    def test_sync_mirrors_tasks_into_db(self):
        self.h.client.issues = [issue(5, body="## Summary\n\nHi.\n")]
        self.h.tasks.sync(5, self.plan)
        rows = self.h.db.tasks(5)
        self.assertEqual([row["text"] for row in rows], ["one", "two"])
        self.assertEqual([row["done"] for row in rows], [0, 1])

    def test_no_checklist_does_nothing(self):
        empty = Path(self.tmp.name) / "empty.md"
        empty.write_text("## Tasks\n\nnothing\n", encoding="utf-8")
        self.h.client.issues = [issue(5)]
        self.h.tasks.sync(5, empty)
        self.assertEqual(self.h.client.writes, [])
        self.assertIn("nothing to sync", self.h.stdout)

    def test_missing_plan_raises(self):
        from gitea_bots.errors import GiteaError
        with self.assertRaises(GiteaError):
            self.h.tasks.sync(5, Path(self.tmp.name) / "nope.md")


if __name__ == "__main__":
    unittest.main()
