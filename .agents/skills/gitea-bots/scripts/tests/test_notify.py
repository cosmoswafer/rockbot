"""Tests for #131: same-model shared delivery + pinned reply workers."""

import json
import subprocess
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from tests.fakes import Harness, issue

SESSION_JSON = json.dumps({
    "data": {"model": {"id": "hy4", "providerID": "opencode-go", "variant": "default"}}
})

HUMAN_REPLY = (
    "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by alice "
    "— https://gitea.test/x/issues/130\n    | @testbot please implement this\n"
)
SELF_REPLY = (
    "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
    "— https://gitea.test/x/issues/130\n    | please implement this\n"
)
SHARED_REPLY = (
    "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by testbot "
    "— https://gitea.test/x/issues/130\n    | @testbot /implement\n"
)
BODY_REPLY = (
    "2026-11-08T05:07:56Z DIRECTED COMMENT on #132 (issue body) by testbot "
    "— https://gitea.test/x/issues/132\n"
    "    | @testbot please implement this\n"
)
TWO_REPLIES = (
    "2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by alice "
    "— https://gitea.test/x/issues/130\n    | @testbot implement this\n"
    "2026-11-08T05:07:57Z DIRECTED COMMENT on #131 (comment 2717) by bob "
    "— https://gitea.test/x/issues/131\n    | @testbot review this\n"
)
AUTO_PLAN = (
    "2026-11-08T07:31:23Z AUTO PLAN #138 \"Use server time\" by saru "
    "— https://gitea.test/x/issues/138\n"
)
TWO_AUTO_PLANS = (
    "2026-11-08T07:31:23Z AUTO PLAN #138 \"Use server time\" by saru "
    "— https://gitea.test/x/issues/138\n"
    "2026-11-08T07:31:24Z AUTO PLAN #140 \"Another\" by saru "
    "— https://gitea.test/x/issues/140\n"
)
REPLY_AND_AUTO_PLAN = HUMAN_REPLY + (
    "2026-11-08T07:31:23Z AUTO PLAN #130 \"Same issue\" by saru "
    "— https://gitea.test/x/issues/130\n"
)


def fake_api_run(cmd, **kwargs):
    """Stand-in for `opencode api get|post` calls made by the Notifier."""
    if "api" in cmd and "get" in cmd and any("/api/session/" in a for a in cmd):
        return subprocess.CompletedProcess(cmd, 0, stdout=SESSION_JSON, stderr="")
    return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")


class NotifierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)
        self.h.config.notify_enabled = True
        self.h.config.notify_session = "ses_test"
        self.h.config.notify_mode = "shared"
        self.h.config.spawn_workers = True
        self.h.config.model = ""

    def tearDown(self):
        self.tmp.cleanup()

    def _notify(self, events_text="", self_login="testbot", shared_login=True):
        with patch("gitea_bots.notify.shutil.which", return_value="/usr/bin/opencode"), \
             patch("gitea_bots.notify.subprocess.run", side_effect=fake_api_run) as run, \
             patch("gitea_bots.notify.subprocess.Popen") as popen:
            self.h.notifier.notify(1, events_text=events_text,
                                   self_login=self_login, shared_login=shared_login)
        return run, popen

    @staticmethod
    def _posts(run):
        return [call for call in run.call_args_list if "post" in call.args[0]]

    def _label_writes(self):
        prefix = f"/repos/{self.h.config.repo}/issues/"
        return [
            path for method, path, _ in self.h.client.writes
            if method == "PUT" and path.startswith(prefix)
            and path.endswith("/labels")
        ]

    def test_resolve_model_from_session_api(self):
        with patch("gitea_bots.notify.subprocess.run", side_effect=fake_api_run):
            self.assertEqual(self.h.notifier.resolve_model(), "opencode-go/hy4")

    def test_explicit_model_override_skips_api(self):
        self.h.config.model = "opencode-go/beam-free"
        with patch("gitea_bots.notify.subprocess.run", side_effect=fake_api_run) as run:
            self.assertEqual(self.h.notifier.resolve_model(), "opencode-go/beam-free")
        run.assert_not_called()

    def test_shared_opt_in_posts_into_session_and_spawns_nothing(self):
        run, popen = self._notify(events_text="")
        self.assertEqual(len(self._posts(run)), 1)
        popen.assert_not_called()
        self.assertIn("notification model: opencode-go/hy4", self.h.stdout)

    def test_human_reply_spawns_pinned_worker_and_delegates(self):
        run, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        cmd = popen.call_args.args[0]
        self.assertIn("--model", cmd)
        self.assertIn("opencode-go/hy4", cmd)
        # #147: workers are non-archived (TUI-visible) by default.
        self.assertNotIn("--archived", cmd)
        # The interactive session is told to report + ack only.
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("pinned background worker", payload["text"])
        self.assertIn("only report the events and then ack", payload["text"])
        self.assertIn("spawned pinned reply worker", self.h.stdout)

    def test_worker_spawn_pins_watched_repo_as_cwd_and_pwd(self):
        # `opencode run` chdirs to $PWD, so a watcher started with a stale
        # PWD from another repo must still spawn workers in its watched repo.
        _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        kwargs = popen.call_args.kwargs
        self.assertEqual(kwargs["cwd"], str(self.h.config.repo_root))
        self.assertEqual(kwargs["env"]["PWD"], str(self.h.config.repo_root))

    def test_archived_workers_opt_in_hides_reply_worker(self):
        # #147: GITEA_BOTS_ARCHIVE_WORKERS=1 restores the old hidden worker.
        self.h.config.archive_workers = True
        _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        self.assertIn("--archived", popen.call_args.args[0])

    def test_archived_workers_opt_in_hides_auto_plan_worker(self):
        self.h.config.notify_mode = "fresh"
        self.h.config.archive_workers = True
        _, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        commands = [call.args[0] for call in popen.call_args_list]
        auto = [cmd for cmd in commands if "auto-plan work" in " ".join(cmd)]
        self.assertEqual(len(auto), 1)
        self.assertIn("--archived", auto[0])

    def test_reporter_stays_archived_regardless_of_worker_flag(self):
        # #147: only workers follow the flag; the poll-loop reporter is hidden.
        for archived in (False, True):
            self.h.config.notify_mode = "fresh"
            self.h.config.archive_workers = archived
            _, popen = self._notify(events_text="")
            self.assertIn("--archived", popen.call_args.args[0])

    def test_plain_self_authored_comment_spawns_no_worker(self):
        # A comment with no at-mention stays non-authorizing (loop avoidance).
        run, popen = self._notify(events_text=SELF_REPLY, self_login="testbot")
        popen.assert_not_called()
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertNotIn("pinned background worker", payload["text"])

    def test_shared_login_mention_spawns_worker(self):
        # #136: a human at-mentioned the bot from the shared account.
        run, popen = self._notify(events_text=SHARED_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        self.assertIn("--model", popen.call_args.args[0])
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("pinned background worker", payload["text"])
        self.assertIn("spawned pinned reply worker", self.h.stdout)

    def test_issue_body_mention_spawns_worker_and_marks_seeing(self):
        # #185: an at-mention in the issue body is the same request as one in
        # a comment, so it must spawn a reply worker and show `seeing`.
        self.h.client.issues.append(issue(132))
        run, popen = self._notify(events_text=BODY_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        prompt = popen.call_args.args[0][-1]
        self.assertIn("#132", prompt)
        self.assertIn("(issue body)", prompt)
        self.assertIn("please implement this", prompt)
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("pinned background worker", payload["text"])
        self.assertEqual(
            self._label_writes(),
            ["/repos/Atom/example/issues/132/labels"],
        )

    def test_shared_login_disabled_keeps_old_behaviour(self):
        run, popen = self._notify(events_text=SHARED_REPLY, self_login="testbot",
                                  shared_login=False)
        popen.assert_not_called()
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertNotIn("pinned background worker", payload["text"])

    def test_workers_can_be_disabled(self):
        self.h.config.spawn_workers = False
        _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        popen.assert_not_called()

    def test_human_reply_marks_issue_seeing_before_worker_spawns(self):
        # #144: a dispatched worker means a background agent picked the issue
        # up, so `seeing` is written before that worker starts.
        self.h.client.issues.append(issue(130))
        spawned = []

        def fake_worker(number, block, model):
            spawned.append((number, len(self.h.client.writes)))

        with patch.object(self.h.notifier, "_spawn_worker",
                          side_effect=fake_worker):
            self._notify(events_text=HUMAN_REPLY, self_login="testbot")

        self.assertEqual(self._label_writes(),
                         ["/repos/Atom/example/issues/130/labels"])
        number, write_count = spawned[0]
        self.assertEqual(number, 130)
        before = self.h.client.writes[:write_count]
        self.assertIn(
            ("PUT", "/repos/Atom/example/issues/130/labels"),
            [(method, path) for method, path, _ in before],
        )
        names = [label["name"] for label in self.h.client.issues[0]["labels"]]
        self.assertIn("👀 seeing", names)

    def test_each_replying_issue_gets_one_pickup_label(self):
        _, popen = self._notify(events_text=TWO_REPLIES, self_login="testbot")
        self.assertEqual(popen.call_count, 2)
        self.assertEqual(sorted(self._label_writes()), [
            "/repos/Atom/example/issues/130/labels",
            "/repos/Atom/example/issues/131/labels",
        ])

    def test_plain_comment_writes_no_pickup_label(self):
        run, popen = self._notify(events_text=SELF_REPLY, self_login="testbot")
        popen.assert_not_called()
        self.assertEqual(self._label_writes(), [])

    def test_pickup_label_is_skipped_when_workers_are_disabled(self):
        # No worker means no background agent picked the issue up, so no label.
        self.h.config.spawn_workers = False
        run, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        popen.assert_not_called()
        self.assertEqual(self._label_writes(), [])
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("set-status.py <N> seeing", payload["text"])

    def test_failed_pickup_label_warns_and_still_spawns_worker(self):
        from gitea_bots.errors import GiteaError

        with patch.object(self.h.status, "apply", side_effect=GiteaError("boom")):
            _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 1)
        self.assertIn("pickup label failed for #130", self.h.stderr)

    def test_auto_plan_marks_issue_seeing(self):
        self.h.config.notify_mode = "fresh"
        self.h.client.issues.append(issue(138))
        _, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        self.assertTrue(popen.call_count >= 1)
        self.assertEqual(self._label_writes(),
                         ["/repos/Atom/example/issues/138/labels"])

    def test_worker_prompts_include_the_pickup_label_fallback(self):
        # The label write can fail (older watcher, network); the worker still
        # knows to apply it itself before doing the work.
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        prompt = popen.call_args.args[0][-1]
        self.assertIn("set-status.py <N> seeing", prompt)

    def test_fresh_mode_without_model_refuses_model_less_spawn(self):
        self.h.config.notify_mode = "fresh"
        self.h.config.notify_session = ""  # no model can be resolved
        _, popen = self._notify(events_text="")
        popen.assert_not_called()
        self.assertIn("fresh mode requested but no model resolved", self.h.stderr)

    def test_fresh_mode_with_model_spawns_pinned_session(self):
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text="")
        self.assertEqual(popen.call_count, 1)
        self.assertIn("opencode-go/hy4", popen.call_args.args[0])

    def test_fresh_default_is_used_when_mode_is_unset(self):
        # #137: a fresh Harness picks up the new Config default.
        with tempfile.TemporaryDirectory() as tmp:
            h = Harness(tmp)
        self.assertEqual(h.config.notify_mode, "fresh")

    def test_fresh_default_with_model_spawns_one_minimal_session(self):
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text="")
        self.assertEqual(popen.call_count, 1)
        cmd = popen.call_args.args[0]
        self.assertIn("--model", cmd)
        self.assertIn("opencode-go/hy4", cmd)
        self.assertIn("--archived", cmd)

    def test_fresh_default_human_reply_runs_work_once(self):
        # #137: fresh notification + pinned worker are two cheap sessions, but
        # only the worker may do the work. The fresh notification session is
        # told to report + ack only.
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text=HUMAN_REPLY, self_login="testbot")
        self.assertEqual(popen.call_count, 2)
        commands = [call.args[0] for call in popen.call_args_list]
        workers = [cmd for cmd in commands if "reply work" in " ".join(cmd)]
        self.assertEqual(len(workers), 1)
        self.assertNotIn("--archived", workers[0])
        reporters = [cmd for cmd in commands if "reply work" not in " ".join(cmd)]
        self.assertEqual(len(reporters), 1)
        self.assertIn("--archived", reporters[0])
        prompt = reporters[0][-1]
        self.assertIn("pinned background worker", prompt)
        self.assertIn("only report the events and then ack", prompt)
        self.assertIn("spawned pinned reply worker", self.h.stdout)

    def test_two_replies_spawn_one_worker_each(self):
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text=TWO_REPLIES, self_login="testbot")
        commands = [call.args[0] for call in popen.call_args_list]
        workers = [cmd for cmd in commands if "reply work" in " ".join(cmd)]
        self.assertEqual(len(workers), 2)
        prompts = [cmd[-1] for cmd in workers]
        self.assertTrue(any("#130" in p and "implement this" in p for p in prompts))
        self.assertTrue(any("#131" in p and "review this" in p for p in prompts))
        # Each per-issue prompt carries only its own event block.
        for prompt in prompts:
            self.assertFalse("#130" in prompt and "#131" in prompt)

    def test_auto_plan_spawns_one_stage2_worker_and_delegates(self):
        # #140: AUTO PLAN authorizes investigation/planning, so it delegates
        # like a human reply that asks for a plan.
        self.h.config.notify_mode = "fresh"
        run, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        commands = [call.args[0] for call in popen.call_args_list]
        auto = [cmd for cmd in commands if "auto-plan work" in " ".join(cmd)]
        self.assertEqual(len(auto), 1)
        self.assertIn("--model", auto[0])
        self.assertIn("opencode-go/hy4", auto[0])
        self.assertNotIn("--archived", auto[0])
        self.assertIn("AUTO PLAN", auto[0][-1])
        self.assertIn("stage 2 ONLY", auto[0][-1])
        self.assertIn("#138", auto[0][-1])
        reporters = [cmd for cmd in commands if cmd not in auto]
        self.assertEqual(len(reporters), 1)
        self.assertIn("pinned background worker", reporters[0][-1])
        self.assertIn("only report the events and then ack", reporters[0][-1])
        self.assertIn("spawned pinned AUTO PLAN worker", self.h.stdout)

    def test_auto_plan_batches_multiple_issues_into_one_worker(self):
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text=TWO_AUTO_PLANS, self_login="testbot")
        commands = [call.args[0] for call in popen.call_args_list]
        auto = [cmd for cmd in commands if "auto-plan work" in " ".join(cmd)]
        self.assertEqual(len(auto), 1)
        self.assertIn("#138", auto[0][-1])
        self.assertIn("#140", auto[0][-1])

    def test_auto_plan_issue_already_answered_spawns_no_extra_worker(self):
        self.h.config.notify_mode = "fresh"
        _, popen = self._notify(events_text=REPLY_AND_AUTO_PLAN,
                                self_login="testbot")
        commands = [call.args[0] for call in popen.call_args_list]
        reply = [cmd for cmd in commands if "reply work" in " ".join(cmd)]
        auto = [cmd for cmd in commands if "auto-plan work" in " ".join(cmd)]
        self.assertEqual(len(reply), 1)
        self.assertEqual(auto, [])

    def test_auto_plan_workers_can_be_disabled(self):
        self.h.config.auto_plan_workers = False
        run, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        popen.assert_not_called()
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("AUTO PLAN", payload["text"])
        self.assertIn("investigate and post a plan", payload["text"])
        self.assertNotIn("only report the events and then ack", payload["text"])

    def test_auto_plan_workers_gated_on_auto_plan(self):
        self.h.config.auto_plan = False
        _, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        popen.assert_not_called()

    def test_spawn_workers_disabled_makes_reporter_run_auto_plan_stage(self):
        self.h.config.spawn_workers = False
        run, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        popen.assert_not_called()
        payload = json.loads(self._posts(run)[0].args[0][-1])
        self.assertIn("investigate and post a plan", payload["text"])
        self.assertNotIn("only report the events and then ack", payload["text"])

    def test_auto_plan_without_model_spawns_nothing(self):
        self.h.config.notify_mode = "shared"
        self.h.config.notify_session = ""
        run, popen = self._notify(events_text=AUTO_PLAN, self_login="testbot")
        popen.assert_not_called()
        self.assertIn("no model resolved", self.h.stderr)

    def test_worker_dispatch_is_bounded(self):
        calls = []
        active = []
        peak = [0]
        lock = threading.Lock()

        def fake_worker(number, block, model):
            with lock:
                calls.append(number)
                active.append(number)
                peak[0] = max(peak[0], len(active))
            time.sleep(0.02)
            with lock:
                active.remove(number)

        self.h.config.max_concurrency = 1
        with patch.object(self.h.notifier, "_spawn_worker",
                          side_effect=fake_worker):
            self.h.notifier._spawn_workers({130: "a", 131: "b"}, "m")
        self.assertEqual(sorted(calls), [130, 131])
        self.assertEqual(peak[0], 1)

    def test_worker_dispatch_allows_configured_parallelism(self):
        active = []
        peak = [0]
        lock = threading.Lock()

        def fake_worker(number, block, model):
            with lock:
                active.append(number)
                peak[0] = max(peak[0], len(active))
            time.sleep(0.05)
            with lock:
                active.remove(number)

        self.h.config.max_concurrency = 2
        with patch.object(self.h.notifier, "_spawn_worker",
                          side_effect=fake_worker):
            self.h.notifier._spawn_workers({130: "a", 131: "b"}, "m")
        self.assertEqual(peak[0], 2)

    def test_shared_without_session_posts_nothing(self):
        self.h.config.notify_session = ""
        run, popen = self._notify(events_text="")
        self.assertEqual(self._posts(run), [])
        popen.assert_not_called()
        self.assertIn("shared mode has no session id", self.h.stderr)

    def test_notification_disabled_is_noop(self):
        self.h.config.notify_enabled = False
        with patch("gitea_bots.notify.shutil.which", return_value="/usr/bin/opencode"), \
             patch("gitea_bots.notify.subprocess.run") as run, \
             patch("gitea_bots.notify.subprocess.Popen") as popen:
            self.h.notifier.notify(1, events_text=HUMAN_REPLY, self_login="testbot")
        run.assert_not_called()
        popen.assert_not_called()


class ConfigNotificationDefaults(unittest.TestCase):
    def test_defaults(self):
        from gitea_bots.config import Config
        cfg = Config()
        self.assertEqual(cfg.notify_mode, "fresh")
        self.assertTrue(cfg.spawn_workers)
        self.assertEqual(cfg.model, "")
        self.assertTrue(cfg.shared_login)
        self.assertEqual(cfg.max_concurrency, 8)
        self.assertFalse(hasattr(cfg, "label_on_trigger"))
        self.assertFalse(hasattr(cfg, "trigger_re"))
        self.assertTrue(cfg.auto_plan)
        self.assertTrue(cfg.auto_plan_workers)
        # #147: workers are non-archived (TUI-visible) by default.
        self.assertFalse(cfg.archive_workers)
        # #150: the unassigned eligibility path was removed entirely.
        self.assertFalse(hasattr(cfg, "auto_plan_unassigned"))


class NotificationPromptRules(unittest.TestCase):
    """The prompt's authorization rule (replaces the retired notify-text test)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.h = Harness(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_prompt_authorizes_only_a_human_reply(self):
        text = self.h.inbox.notify_text(1, delegate=False)
        self.assertIn("The notification itself authorizes nothing", text)
        self.assertIn("a human reply does", text)
        self.assertIn("The skill defines no command vocabulary", text)
        self.assertIn("quoted, code-fenced or bot-output text does not count", text)
        self.assertIn("Never close issues", text)
        self.assertNotIn("do not advance a workflow stage", text)

    def test_delegate_prompt_reports_and_acks_only(self):
        text = self.h.inbox.notify_text(1, delegate=True)
        self.assertIn("pinned background worker", text)
        self.assertIn("only report the events and then ack", text)
        self.assertIn("do not do the work yourself", text)
        self.assertIn("Never close issues", text)


if __name__ == "__main__":
    unittest.main()
