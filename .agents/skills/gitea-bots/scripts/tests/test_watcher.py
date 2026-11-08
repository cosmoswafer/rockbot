"""End-to-end behavioural tests for the polling watcher (fake client, no net)."""

import tempfile
import threading
import unittest
from pathlib import Path

from gitea_bots.clock import now_utc, shift_iso
from gitea_bots.errors import GiteaError
from gitea_bots.rules import STAGE2_MARKER, STAGE3_MARKER
from tests.fakes import BASE, FakeClient, Harness, comment, issue


class _TrackingCommentsClient(FakeClient):
    """Records the maximum number of simultaneous comment fetches."""

    def __init__(self, expected):
        super().__init__()
        self.expected = expected
        self.in_flight = 0
        self.max_in_flight = 0
        self._cv = threading.Condition()

    def get(self, path, params=None):
        if path.endswith("/comments"):
            with self._cv:
                self.in_flight += 1
                self.max_in_flight = max(self.max_in_flight, self.in_flight)
                if self.in_flight >= self.expected:
                    self._cv.notify_all()
                else:
                    self._cv.wait_for(
                        lambda: self.in_flight >= self.expected, timeout=0.2
                    )
            try:
                return super().get(path, params)
            finally:
                with self._cv:
                    self.in_flight -= 1
        return super().get(path, params)


class _FailCommentsClient(FakeClient):
    """Raises ``GiteaError`` for one issue's comment fetch once armed."""

    def __init__(self, fail_num):
        super().__init__()
        self.fail_num = fail_num
        self.fail = False

    def get(self, path, params=None):
        if self.fail and path.endswith(f"/issues/{self.fail_num}/comments"):
            raise GiteaError("boom")
        return super().get(path, params)


def _tracked(number, **overrides):
    """A per-issue snapshot as stored in the ``issues`` table."""
    snap = {
        "updated_at": "2026-11-08T00:00:10Z",
        "title": "Tracked",
        "state": "open",
        "labels": "",
        "assignees": "",
        "milestone": "",
        "last_comment_id": 0,
        "last_assignee_event_id": 0,
    }
    snap.update(overrides)
    return {str(number): snap}


class WatcherTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    # --- baseline / reporting ------------------------------------------------
    def test_baseline_report_existing(self):
        self.h.client.issues = [
            issue(1, "Other issue"),
            issue(2, "Bot issue", assignees=["testbot"]),
        ]
        rc = self.h.watcher.check(1)
        self.assertEqual(rc, 0)
        self.assertIn('NEW ISSUE #1 [open] "Other issue"', self.h.stdout)
        self.assertIn('AUTO PLAN #2 "Bot issue"', self.h.stdout)

    def test_baseline_is_silent_without_report_existing(self):
        self.h.client.issues = [issue(1, "Other issue")]
        self.h.watcher.check(0)
        self.assertNotIn("NEW ISSUE #1", self.h.stdout)
        self.assertIn("check complete", self.h.stdout)

    def test_second_check_does_not_rereport(self):
        self.h.client.issues = [
            issue(1, "Other issue"),
            issue(2, "Bot issue", assignees=["testbot"]),
        ]
        self.h.watcher.check(1)
        self.h.reset_output()
        self.h.client.incremental_issues = []  # incremental window empty
        self.h.watcher.check(0)
        self.assertNotIn("NEW ISSUE #1", self.h.stdout)
        self.assertNotIn("NEW ISSUE #2", self.h.stdout)

    # --- AUTO PLAN -----------------------------------------------------------
    def test_incremental_auto_plan_emitted_once(self):
        self.h.client.issues = [issue(2, "Bot issue", assignees=["testbot"])]
        self.h.client.incremental_issues = []
        self.h.watcher.check(0)  # silent baseline, records #2
        self.h.reset_output()
        self.h.client.incremental_issues = [
            issue(2, "Bot issue", assignees=["testbot"])
        ]
        self.h.watcher.check(0)
        self.assertIn('AUTO PLAN #2 "Bot issue"', self.h.stdout)
        self.h.reset_output()
        self.h.watcher.check(0)
        self.assertNotIn("AUTO PLAN #2", self.h.stdout)

    def test_tasks_or_stage2_marker_suppresses_auto_plan(self):
        self.h.client.issues = [
            issue(5, "Has tasks",
                  body="## Summary\n\nX.\n\n## Tasks\n\n- [ ] t\n",
                  assignees=["testbot"]),
            issue(6, "Has marker", assignees=["testbot"]),
        ]
        self.h.client.comments[6] = [
            comment(500, f"plan\n{STAGE2_MARKER}", "testbot", 6)
        ]
        self.h.watcher.check(1)
        self.assertNotIn("AUTO PLAN #5", self.h.stdout)
        self.assertNotIn("AUTO PLAN #6", self.h.stdout)

    def test_auto_plan_disabled(self):
        h = Harness(self.tmp.name, auto_plan=False)
        h.client.issues = [issue(2, "Bot issue", assignees=["testbot"])]
        h.watcher.check(1)
        self.assertNotIn("AUTO PLAN #2", h.stdout)

    # --- AUTO PLAN eligibility (#150) --------------------------------------
    def test_polling_never_plans_unassigned_issues(self):
        self.h.client.issues = [
            issue(7, "Filed unassigned", author="testbot"),
            issue(8, "Someone else's", author="alice"),
            issue(9, "Owned by a human", author="testbot", assignees=["alice"]),
            issue(2, "Assigned", assignees=["testbot"]),
        ]
        self.h.watcher.check(1)
        self.assertNotIn("AUTO PLAN #7", self.h.stdout)
        self.assertNotIn("AUTO PLAN #8", self.h.stdout)
        self.assertNotIn("AUTO PLAN #9", self.h.stdout)
        self.assertIn('AUTO PLAN #2 "Assigned"', self.h.stdout)

    def test_catch_up_only_plans_assigned_issues(self):
        self.h.client.issues = [
            issue(7, "Self-authored unassigned", author="testbot"),
            issue(8, "Other author unassigned", author="alice"),
            issue(9, "Self-authored owned", author="testbot", assignees=["alice"]),
            issue(10, "Assigned", assignees=["testbot"]),
        ]
        self.h.watcher.catch_up_assigned()
        self.assertNotIn("AUTO PLAN #7", self.h.stdout)
        self.assertIn("AUTO PLAN #10", self.h.stdout)
        self.assertNotIn("AUTO PLAN #8", self.h.stdout)
        self.assertNotIn("AUTO PLAN #9", self.h.stdout)
        self.assertIn("eligible: 1", self.h.stdout)

    def test_scan_unplanned_only_lists_assigned_issues(self):
        self.h.client.issues = [
            issue(7, "Self-authored unassigned", author="testbot"),
            issue(8, "Other author unassigned", author="alice"),
            issue(9, "Self-authored owned", author="testbot", assignees=["alice"]),
            issue(10, "Assigned", assignees=["testbot"]),
        ]
        rc = self.h.watcher.scan_unplanned()
        self.assertEqual(rc, 0)
        self.assertNotIn("AUTO PLAN #7", self.h.stdout)
        self.assertIn('AUTO PLAN #10 "Assigned"', self.h.stdout)
        self.assertNotIn("AUTO PLAN #8", self.h.stdout)
        self.assertNotIn("AUTO PLAN #9", self.h.stdout)
        self.assertIn("1 unplanned plan-eligible issue(s)", self.h.stdout)

    def test_unassigned_eligibility_toggle_no_longer_exists(self):
        from gitea_bots.config import Config
        self.assertFalse(hasattr(Config(), "auto_plan_unassigned"))

    # --- comments / mentions ------------------------------------------------
    def _baseline_then_incremental(self, comments, updated_at="2026-11-08T00:01:00Z"):
        self.h.client.issues = [issue(1, "Other issue")]
        self.h.client.incremental_issues = []
        self.h.watcher.check(0)
        self.h.reset_output()
        self.h.client.incremental_issues = [
            issue(1, "Other issue", updated_at=updated_at)
        ]
        self.h.client.comments[1] = comments
        self.h.watcher.check(0)

    def _use_client(self, client):
        self.h.client = client
        self.h.status.client = client
        self.h.watcher.client = client

    def test_directed_mention(self):
        self._baseline_then_incremental(
            [comment(3001, "@testbot Did you fix it already?", "alice", 1)]
        )
        self.assertIn("DIRECTED COMMENT on #1 (comment 3001) by alice", self.h.stdout)
        self.assertIn("Did you fix it already?", self.h.stdout)
        self.assertNotIn("body/other", self.h.stdout)  # no redundant issue event

    def test_command_words_without_mention_are_not_surfaced(self):
        # #134: a comment that spells out a former trigger phrase but does not
        # at-mention the bot is ordinary chat, not a request.
        self._baseline_then_incremental(
            [comment(3002, "/implement execute dev flow", "alice", 1)]
        )
        self.assertNotIn("DIRECTED COMMENT on #1 (comment 3002)", self.h.stdout)
        self.assertNotIn("TRIGGER COMMENT on #1 (comment 3002)", self.h.stdout)

    def test_self_authored_mention_is_surfaced(self):
        # #136: the bot and human share one account, so a self-authored
        # at-mention is a human reply and must be delivered.
        self._baseline_then_incremental(
            [comment(3003, "@testbot status?", "testbot", 1)]
        )
        self.assertIn("DIRECTED COMMENT on #1 (comment 3003)", self.h.stdout)

    def test_dedicated_bot_self_authored_mention_is_dropped(self):
        h = Harness(self.tmp.name, shared_login=False)
        h.client.issues = [issue(1, "Other issue")]
        h.client.incremental_issues = []
        h.watcher.check(0)
        h.reset_output()
        h.client.incremental_issues = [issue(1, "Other issue")]
        h.client.comments[1] = [comment(3005, "@testbot hi", "testbot", 1)]
        h.watcher.check(0)
        self.assertNotIn("DIRECTED COMMENT on #1 (comment 3005)", h.stdout)

    def test_bot_output_mention_is_not_directed(self):
        from gitea_bots.rules import STAGE2_MARKER
        self._baseline_then_incremental(
            [comment(3006, f"@testbot\n{STAGE2_MARKER}", "testbot", 1)]
        )
        self.assertNotIn("DIRECTED COMMENT on #1 (comment 3006)", self.h.stdout)

    def test_all_comments_mode(self):
        h = Harness(self.tmp.name, all_comments=True)
        h.client.issues = [issue(1, "Other issue")]
        h.client.incremental_issues = []
        h.watcher.check(0)
        h.reset_output()
        h.client.incremental_issues = [issue(1, "Other issue")]
        h.client.comments[1] = [comment(42, "just chatting", "alice", 1)]
        h.watcher.check(0)
        self.assertIn("NEW COMMENT on #1 (comment 42) by alice", h.stdout)

    # --- concurrent prefetch (#138) -----------------------------------------
    def test_comment_prefetch_runs_concurrently(self):
        client = _TrackingCommentsClient(expected=2)
        self._use_client(client)
        client.issues = [issue(1, "A"), issue(2, "B")]
        client.incremental_issues = []
        self.h.watcher.check(0)
        self.assertEqual(client.max_in_flight, 2)

    def test_comment_prefetch_is_bounded(self):
        self.h.config.max_concurrency = 1
        client = _TrackingCommentsClient(expected=99)
        self._use_client(client)
        client.issues = [issue(1, "A"), issue(2, "B")]
        client.incremental_issues = []
        self.h.watcher.check(0)
        self.assertEqual(client.max_in_flight, 1)

    def test_failed_fetch_keeps_other_events_and_holds_watermark(self):
        client = _FailCommentsClient(fail_num=2)
        self._use_client(client)
        client.issues = [issue(1, "One"), issue(2, "Two")]
        client.incremental_issues = []
        self.h.watcher.check(0)  # baseline
        before = self.h.state._read()["last_poll"]
        client.fail = True
        updated = "2026-11-08T00:01:00Z"
        client.incremental_issues = [
            issue(1, "One", updated_at=updated),
            issue(2, "Two", updated_at=updated),
        ]
        client.comments[1] = [comment(100, "@testbot status?", "alice", 1)]
        self.h.watcher.check(0)
        self.assertIn("DIRECTED COMMENT on #1", self.h.stdout)
        pending = self.h.inbox.pending_file.read_text(encoding="utf-8")
        self.assertIn("DIRECTED COMMENT on #1", pending)
        # A failed fetch keeps the old watermark so the events retry.
        self.assertEqual(self.h.state._read()["last_poll"], before)

    # --- timeline re-assignment (#119) ---------------------------------------
    def test_unplanned_reassignment_emits_auto_plan(self):
        self.h.state.write({
            "version": 2, "repo": "Atom/example", "api": "https://gitea.test/api/v1",
            "last_poll": shift_iso(now_utc(), -600),
            "issues": _tracked(20, title="Reassign me", assignees="testbot",
                               last_assignee_event_id=100),
        })
        self.h.client.incremental_issues = [
            issue(20, "Reassign me", assignees=["testbot"])
        ]
        self.h.client.timeline[20] = [
            {"id": 101, "type": "assignees", "assignee": {"login": "testbot"},
             "removed_assignee": True},
            {"id": 102, "type": "assignees", "assignee": {"login": "testbot"},
             "removed_assignee": False},
        ]
        self.h.watcher.check(0)
        self.assertIn("AUTO PLAN #20", self.h.stdout)

    def test_planned_reassignment_emits_reassigned(self):
        self.h.state.write({
            "version": 2, "repo": "Atom/example", "api": "https://gitea.test/api/v1",
            "last_poll": shift_iso(now_utc(), -600),
            "issues": _tracked(21, title="Planned reassign", assignees="testbot",
                               last_comment_id=900, last_assignee_event_id=200,
                               auto_plan_at="2026-11-07T00:00:00Z"),
        })
        self.h.client.incremental_issues = [
            issue(21, "Planned reassign",
                  body="## Summary\n\nHas plan.\n\n## Tasks\n\n- [ ] x\n",
                  assignees=["testbot"])
        ]
        self.h.client.comments[21] = [
            comment(900, f"## Findings\n\nPlan.\n\n{STAGE2_MARKER}", "testbot", 21)
        ]
        self.h.client.timeline[21] = [
            {"id": 201, "type": "assignees", "assignee": {"login": "testbot"},
             "removed_assignee": True},
            {"id": 202, "type": "assignees", "assignee": {"login": "testbot"},
             "removed_assignee": False},
        ]
        self.h.watcher.check(0)
        self.assertIn("REASSIGNED #21", self.h.stdout)
        self.assertNotIn("AUTO PLAN #21", self.h.stdout)

    # --- catch-up after a state-schema upgrade ------------------------------
    def test_catchup_resurfaces_recent_directed_comment(self):
        recent = shift_iso(now_utc(), -300)
        recent_plus = shift_iso(now_utc(), -240)
        self.h.state.write({
            "version": 1, "repo": "Atom/example", "api": "https://gitea.test/api/v1",
            "last_poll": shift_iso(now_utc(), -600),
            "issues": _tracked(30, title="Mention me", assignees="testbot",
                               last_comment_id=4000),
        })
        # prev updated_at must match the incremental issue to avoid a body/other event
        self.h.state._read()  # sanity: state is readable
        self.h.client.incremental_issues = [
            issue(30, "Mention me", assignees=["testbot"],
                  updated_at=recent_plus)
        ]
        self.h.client.comments[30] = [
            comment(3999, "@testbot status?", "alice", 30, created_at=recent)
        ]
        self.h.watcher.check(0)
        self.assertIn(
            "DIRECTED COMMENT (catch-up) on #30 (comment 3999) by alice",
            self.h.stdout,
        )

    # --- diagnostics / state compatibility -----------------------------------
    def test_scan_unplanned(self):
        self.h.client.issues = [
            issue(2, "Bot issue", assignees=["testbot"]),
            issue(1, "Other issue"),
            issue(40, "Body plan",
                  body="## Summary\n\nPlanned.\n\n## Tasks\n\n- [ ] a\n",
                  assignees=["testbot"]),
        ]
        rc = self.h.watcher.scan_unplanned()
        self.assertEqual(rc, 0)
        self.assertIn('AUTO PLAN #2 "Bot issue"', self.h.stdout)
        self.assertNotIn('AUTO PLAN #1', self.h.stdout)
        self.assertNotIn('AUTO PLAN #40', self.h.stdout)
        self.assertIn("WARN scan-unplanned: #40 has a body plan", self.h.stderr)

    def test_legacy_state_shape_is_accepted(self):
        self.h.state.write({
            "version": 2,
            "repo": "Atom/example",
            "api": "https://gitea.test/api/v1",
            "initialized_at": BASE,
            "last_poll": None,
            "issues": {},
        })
        loaded = self.h.state.load_or_init()
        self.assertEqual(loaded["repo"], "Atom/example")
        self.assertTrue(self.h.state.is_baseline(loaded))
        self.assertEqual(
            len(list(Path(self.tmp.name).glob("state.db.repo.*"))), 0
        )
        self.assertEqual(
            len(list(Path(self.tmp.name).glob("state.db.corrupt.*"))), 0
        )


if __name__ == "__main__":
    unittest.main()
