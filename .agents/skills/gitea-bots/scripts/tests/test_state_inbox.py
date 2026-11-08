"""Tests for the SQLite state store and the pending-event inbox."""

import io
import tempfile
import unittest

from gitea_bots import EVENTS_VERSION
from gitea_bots.inbox import Inbox
from gitea_bots.log import Logger
from gitea_bots.state import StateStore
from tests.fakes import Harness, make_config


class StateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.cfg = make_config(self.tmp.name)
        self.logger = Logger(out=io.StringIO(), err=io.StringIO())
        self.store = StateStore(self.cfg, self.logger)

    def tearDown(self):
        self.tmp.cleanup()

    def test_fresh_state_is_baseline(self):
        state = self.store.load_or_init()
        self.assertTrue(self.store.is_baseline(state))
        self.assertEqual(self.store.version(state), EVENTS_VERSION)
        self.assertEqual(state["repo"], "Atom/example")

    def test_roundtrip(self):
        state = self.store.load_or_init()
        state["last_poll"] = "2026-11-08T00:00:00Z"
        state["issues"]["7"] = {"title": "X", "last_comment_id": 12}
        self.store.write(state)
        loaded = self.store.load_or_init()
        self.assertFalse(self.store.is_baseline(loaded))
        self.assertEqual(loaded["issues"]["7"]["last_comment_id"], 12)
        self.assertEqual(self.store.get_issue(loaded, 7)["title"], "X")

    def test_commit_poll_is_atomic_and_records_poll(self):
        self.store.load_or_init()
        self.store.commit_poll(
            {"7": {"title": "X", "last_comment_id": 12}},
            last_poll="2026-11-08T00:00:00Z",
            version=2, scanned=1, emitted=1, ok=True,
        )
        state = self.store.load()
        self.assertEqual(state["last_poll"], "2026-11-08T00:00:00Z")
        self.assertEqual(state["issues"]["7"]["title"], "X")
        self.assertEqual(self.store.db.count_polls(), 1)

    def test_commit_poll_without_watermark_keeps_baseline(self):
        self.store.load_or_init()
        self.store.commit_poll(
            {"7": {"title": "X"}}, version=2, scanned=1, emitted=0,
            ok=False, error="issue read failed",
        )
        state = self.store.load()
        self.assertIsNone(state["last_poll"])
        row = self.store.db.connect().execute("SELECT * FROM polls").fetchone()
        self.assertEqual(row["ok"], 0)
        self.assertEqual(row["error"], "issue read failed")

    def test_corrupt_state_is_backed_up_and_rebaselined(self):
        self.cfg.state_dir.mkdir(parents=True, exist_ok=True)
        self.store.path.write_text("not json", encoding="utf-8")
        state = self.store.load_or_init()
        self.assertTrue(self.store.is_baseline(state))
        backups = list(self.cfg.state_dir.glob("state.db.corrupt.*"))
        self.assertEqual(len(backups), 1)

    def test_repo_mismatch_is_backed_up_and_rebaselined(self):
        self.cfg.state_dir.mkdir(parents=True, exist_ok=True)
        self.store.write({
            "version": 2, "repo": "Other/repo", "api": "x",
            "last_poll": None, "issues": {},
        })
        state = self.store.load_or_init()
        self.assertEqual(state["repo"], "Atom/example")
        self.assertEqual(len(list(self.cfg.state_dir.glob("state.db.repo.*"))), 1)

    def test_reset_moves_db_aside(self):
        self.store.load_or_init()
        self.store.commit_poll(
            {"7": {"title": "X"}}, last_poll="2026-11-08T00:00:00Z",
        )
        backup = self.store.reset()
        self.assertIsNotNone(backup)
        self.assertFalse(self.store.path.exists())
        self.assertTrue(backup.exists())
        state = self.store.load_or_init()
        self.assertTrue(self.store.is_baseline(state))
        self.assertEqual(state["issues"], {})


class InboxTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def _events(self):
        return (
            "2026-11-08T00:00:00Z NEW ISSUE #9 \"X\" — https://gitea.test/Atom/example/issues/9\n"
            "    | @testbot status?\n"
        )

    def test_publish_writes_batch_view_and_events(self):
        pub = self.h.inbox.publish(
            self._events(), 1, baseline=False, notify_armed=False
        )
        self.assertTrue(pub["ok"])
        self.assertFalse(pub["notify"])
        self.assertIsNotNone(pub["batch_id"])
        batches = self.h.inbox.pending_batches()
        self.assertEqual(len(batches), 1)
        view = self.h.inbox.pending_file.read_text(encoding="utf-8")
        self.assertIn("[batch:", view)
        self.assertIn("NEW ISSUE #9", view)
        lines = self.h.inbox.db.event_lines(pub["batch_id"])
        self.assertEqual(len(lines), 2)  # event line + indented body line

    def test_ack_drains_and_generated_view_does_not_resurrect(self):
        self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=False)
        # A generated view must not be re-migrated as a legacy batch.
        self.h.inbox.migrate_legacy()
        self.assertEqual(len(self.h.inbox.pending_batches()), 1)
        self.h.inbox.ack()
        self.assertEqual(self.h.inbox.pending_batches(), [])
        self.assertEqual(self.h.inbox.pending_file.read_text(encoding="utf-8").strip(), "")
        # Second ack is a no-op.
        self.h.inbox.ack()
        self.assertIn("inbox: empty", self.h.stdout)

    def test_ack_specific_batch_keeps_others_pending(self):
        first = self.h.inbox.publish(
            self._events(), 1, baseline=False, notify_armed=False
        )["batch_id"]
        self.h.inbox.publish(
            self._events(), 1, baseline=False, notify_armed=False
        )
        self.h.inbox.ack(first)
        remaining = self.h.inbox.pending_batches()
        self.assertEqual(len(remaining), 1)
        self.assertNotEqual(remaining[0]["id"], first)

    def test_no_events_is_noop(self):
        pub = self.h.inbox.publish("", 0, baseline=False, notify_armed=True)
        self.assertTrue(pub["ok"])
        self.assertFalse(pub["notify"])
        self.assertEqual(self.h.inbox.pending_batches(), [])

    def test_notify_enabled_only_when_armed_and_not_baseline(self):
        pub = self.h.inbox.publish(self._events(), 1, baseline=True, notify_armed=True)
        self.assertFalse(pub["notify"])
        self.h.inbox.ack()
        pub = self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=True)
        self.assertTrue(pub["notify"])

    def test_notify_text_self_contained(self):
        self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=False)
        text = self.h.inbox.notify_text(1)
        self.assertIn("Load the gitea-bots skill first", text)
        self.assertIn("a human reply does", text)
        self.assertIn("work out what the human is asking for", text)
        self.assertIn("A DIRECTED COMMENT means a human at-mentioned the bot", text)
        self.assertIn("answer it on the issue", text)
        self.assertIn("following AGENTS.md", text)
        self.assertIn("The bot never writes an at-mention", text)
        self.assertIn("Never close issues", text)
        # The skill defines no command vocabulary.
        self.assertNotIn("/implement", text)
        self.assertNotIn("stage trigger", text)
        # The bot never writes a literal at-mention, even in a prompt.
        self.assertNotIn("@testbot", text)
        self.assertNotIn("human-authored", text)
        # #153: the no-worker path must tell the agent to set the pickup label.
        self.assertIn("set-status.py <N> seeing", text)

    def test_notify_text_delegate_variant(self):
        self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=False)
        text = self.h.inbox.notify_text(1, delegate=True)
        self.assertIn("pinned background worker", text)
        self.assertIn("only report the events and then ack", text)
        self.assertIn("Never close issues", text)
        # The interactive session must not do the work itself.
        self.assertNotIn("work out what the human is asking for", text)

    def test_worker_text_is_self_contained(self):
        text = self.h.inbox.worker_text(2, "EVENT LINE\n")
        self.assertIn("Load the gitea-bots skill first", text)
        self.assertIn("Work out what the human is asking for", text)
        self.assertIn("following AGENTS.md", text)
        self.assertIn("The bot never writes an at-mention", text)
        self.assertIn("Never close issues", text)
        self.assertIn("EVENT LINE", text)
        # No stage vocabulary and no command grammar in the worker prompt.
        self.assertNotIn("/implement", text)
        self.assertNotIn("stage 2", text)
        self.assertNotIn("human-authored", text)

    def test_auto_plan_worker_text_is_self_contained(self):
        text = self.h.inbox.auto_plan_worker_text(3, "AUTO PLAN #7 x\n", [7])
        self.assertIn("AUTO PLAN", text)
        self.assertIn("stage 2 ONLY", text)
        self.assertIn("#7", text)
        self.assertIn("plan-eligible", text)
        self.assertIn("assigned to the token user", text)
        # #150: eligibility is assignment-only, so the unassigned wording is gone.
        self.assertNotIn("self-authored and unassigned", text)
        self.assertIn("Load the gitea-bots skill first", text)
        self.assertIn("never stage 3", text)
        self.assertIn("The bot never writes an at-mention", text)
        self.assertIn("never close issues", text)
        self.assertIn("AUTO PLAN #7 x", text)
        self.assertNotIn("@testbot", text)
        # #153: the AUTO PLAN worker must set the pickup label first.
        self.assertIn("set-status.py <N> seeing", text)

    def test_notify_text_names_auto_plan_in_both_variants(self):
        self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=False)
        reporter = self.h.inbox.notify_text(1)
        delegate = self.h.inbox.notify_text(1, delegate=True)
        self.assertIn("AUTO PLAN", reporter)
        self.assertIn("investigate and post a plan", reporter)
        self.assertIn("AUTO PLAN", delegate)
        self.assertIn("investigation and planning only", delegate)

    def test_inbox_lists_batches(self):
        self.h.inbox.publish(self._events(), 1, baseline=False, notify_armed=False)
        self.h.inbox.inbox()
        self.assertIn("1 batch(es) unacknowledged", self.h.stdout)
        self.assertIn("NEW ISSUE #9", self.h.stdout)

    def test_legacy_pending_is_migrated(self):
        self.h.inbox.ensure_dirs()
        self.h.inbox.pending_file.write_text(
            "## 2026-11-08T00:00:00Z — 1 event(s) from Atom/example\n\n"
            "2026-11-08T00:00:00Z NEW ISSUE #9 \"X\" — url\n",
            encoding="utf-8",
        )
        self.h.inbox.migrate_legacy()
        batches = self.h.inbox.pending_batches()
        self.assertEqual(len(batches), 1)
        self.assertTrue(batches[0]["id"].startswith("legacy-"))


if __name__ == "__main__":
    unittest.main()
