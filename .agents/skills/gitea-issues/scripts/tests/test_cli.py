"""End-to-end CLI tests: arguments in, requests out, exit codes and text.

Everything runs against :mod:`tests.support`, so the assertions are about the
exact API calls made — the parts that silently break in real use (the
``milestone`` key, numeric label IDs, the close guard, the template check).
"""

import json
import tempfile
import unittest
from pathlib import Path

from tests import support
from gitea_issues import cli


class DoctorTests(unittest.TestCase):
    def test_a_healthy_setup_exits_zero(self):
        result = support.run_cli(["doctor"], support.demo_transport())
        self.assertEqual(result.code, 0)
        self.assertIn("PASS  repo access", result.stdout)
        self.assertIn("GITEA_TOKEN: set", result.stdout)
        self.assertIn("value hidden", result.stdout)

    def test_the_token_value_is_never_printed(self):
        result = support.run_cli(["doctor"], support.demo_transport())
        self.assertNotIn(support.TOKEN, result.stdout + result.stderr)

    def test_a_rejected_token_is_reported_as_a_failure(self):
        transport = support.demo_transport()
        transport.route(f"GET {support.REPO_PATH}",
                        (401, {"message": "invalid username, password or token"}))
        result = support.run_cli(["doctor"], transport)
        self.assertEqual(result.code, 1)
        self.assertIn("FAIL", result.stdout)

    def test_a_missing_token_stops_before_any_request(self):
        transport = support.demo_transport()
        result = support.run_cli(["doctor"], transport,
                                settings=support.make_settings(token=""))
        self.assertEqual(result.code, 1)
        self.assertIn("GITEA_TOKEN", result.stderr)
        self.assertEqual(transport.calls, [])


class ReadTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_me_prefers_the_user_endpoint(self):
        result = support.run_cli(["me"], self.transport)
        self.assertEqual(result.stdout.strip(), "bot  (from /user)")

    def test_me_falls_back_and_says_so(self):
        result = support.run_cli(["me"], support.demo_transport(user_scope=False))
        self.assertIn("jimmy", result.stdout)
        self.assertIn("inferred", result.stdout)

    def test_list_skips_pull_requests_by_default(self):
        result = support.run_cli(["list"], self.transport)
        self.assertIn("#687", result.stdout)
        self.assertNotIn("#701", result.stdout)
        self.assertIn("1/3", result.stdout)

    def test_list_can_include_pull_requests(self):
        self.assertIn("#701", support.run_cli(["list", "--include-pulls"],
                                             self.transport).stdout)

    def test_list_state_all_shows_closed_issues(self):
        self.assertIn("#700", support.run_cli(["list", "--state", "all"],
                                             self.transport).stdout)

    def test_list_filter_by_milestone(self):
        result = support.run_cli(["list", "--milestone", "v3.7.20"], self.transport)
        self.assertIn("#690", result.stdout)
        self.assertNotIn("#687", result.stdout)

    def test_list_json_output_is_machine_readable(self):
        result = support.run_cli(["--json", "list", "--limit", "2"], self.transport)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["count"], 2)
        self.assertEqual(payload["issues"][0]["number"], 687)

    def test_list_columns_are_selectable(self):
        result = support.run_cli(["list", "--columns", "number,state"],
                                self.transport)
        self.assertNotIn("stale total", result.stdout)

    def test_view_shows_the_body_and_the_thread(self):
        result = support.run_cli(["view", "687", "--comments"], self.transport)
        self.assertIn("#687", result.stdout)
        self.assertIn("comment 500 by bot", result.stdout)
        self.assertIn("## Tasks", result.stdout)

    def test_comments_command(self):
        result = support.run_cli(["comments", "687"], self.transport)
        self.assertIn("Committed in 1a2b3c4", result.stdout)

    def test_view_json_has_the_structured_issue(self):
        result = support.run_cli(["--json", "view", "690"], self.transport)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["issue"]["number"], 690)
        self.assertEqual(payload["issue"]["tasks"], "0/3")

    def test_a_missing_issue_is_an_error(self):
        result = support.run_cli(["view", "1234"], self.transport)
        self.assertEqual(result.code, 1)


class CreateTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_generated_body_follows_the_template(self):
        result = support.run_cli(
            ["create", "--title", "Ring total", "--summary", "Stale totals.",
             "--task", "Recompute on delete", "--task", "Add a test",
             "--related", "615"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        payload = self.transport.last("POST", "/issues").payload
        self.assertEqual(payload["title"], "Ring total")
        for fragment in ("## Summary", "## Tasks", "- [ ] Recompute on delete",
                         "## Related", "- #615"):
            self.assertIn(fragment, payload["body"])
        self.assertIn("#900 created", result.stdout)

    def test_milestone_is_sent_as_a_numeric_id_under_the_right_key(self):
        support.run_cli(["create", "--title", "T", "--summary", "s",
                        "--task", "a", "--milestone", "v3.7.19"], self.transport)
        payload = self.transport.last("POST", "/issues").payload
        self.assertEqual(payload["milestone"], 87)
        self.assertNotIn("milestone_id", payload)

    def test_labels_are_resolved_to_numeric_ids(self):
        support.run_cli(["create", "--title", "T", "--summary", "s", "--task", "a",
                        "--label", "bug", "--label", "committed"], self.transport)
        self.assertEqual(sorted(self.transport.last("POST", "/issues").payload["labels"]),
                         [29, 65])

    def test_assign_me_uses_the_resolved_login(self):
        support.run_cli(["create", "--title", "T", "--summary", "s", "--task", "a",
                        "--assign-me"], self.transport)
        self.assertEqual(self.transport.last("POST", "/issues").payload["assignees"],
                         ["bot"])

    def test_a_prose_body_without_a_checklist_is_refused(self):
        result = support.run_cli(["create", "--title", "T",
                                 "--body", "just some prose"], self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("template", result.stderr)
        self.assertEqual(self.transport.find("POST", "/issues"), [])

    def test_acceptance_criteria_are_refused(self):
        body = "## Tasks\n\n- [ ] a\n\n## Acceptance criteria\n\n- [ ] b\n"
        result = support.run_cli(["create", "--title", "T", "--body", body],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("acceptance", result.stderr.lower())

    def test_force_files_a_noncompliant_body_anyway(self):
        result = support.run_cli(["create", "--title", "T",
                                 "--body", "just some prose", "--force"],
                                self.transport)
        self.assertEqual(result.code, 0)
        self.assertTrue(self.transport.find("POST", "/issues"))

    def test_nothing_to_file_is_a_usage_error(self):
        result = support.run_cli(["create", "--title", "T"], self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("--summary", result.stderr)

    def test_body_from_a_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "issue.md"
            path.write_text("## Summary\n\nx\n\n## Tasks\n\n- [ ] a\n", "utf-8")
            result = support.run_cli(["create", "--title", "T",
                                     "--body-file", str(path)], self.transport)
            self.assertEqual(result.code, 0, result.stderr)
            self.assertIn("- [ ] a",
                          self.transport.last("POST", "/issues").payload["body"])

    def test_dry_run_sends_no_write(self):
        result = support.run_cli(["--dry-run", "create", "--title", "T",
                                 "--summary", "s", "--task", "a"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        self.assertEqual(self.transport.writes(), [])
        self.assertIn("DRY-RUN POST", result.stderr)
        self.assertIn("would create", result.stdout)

    def test_json_output_carries_the_number_and_url(self):
        result = support.run_cli(["--json", "create", "--title", "T",
                                 "--summary", "s", "--task", "a"], self.transport)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["number"], 900)
        self.assertIn("/issues/900", payload["html_url"])


class CommentTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_comment_body_is_posted_verbatim(self):
        result = support.run_cli(["comment", "687", "--body", "Fixed in 1a2b3c4."],
                                self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        self.assertEqual(self.transport.last("POST", "/comments").payload["body"],
                         "Fixed in 1a2b3c4.")

    def test_comment_can_move_the_status_label_in_the_same_call(self):
        support.run_cli(["comment", "687", "--body", "done",
                        "--status", "committed"], self.transport)
        self.assertTrue(self.transport.find("PUT", "/issues/687/labels"))

    def test_an_empty_comment_is_refused(self):
        result = support.run_cli(["comment", "687", "--body", "   "], self.transport)
        self.assertEqual(result.code, 1)
        self.assertEqual(self.transport.find("POST", "/comments"), [])


class TasksCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_show_lists_numbered_tasks(self):
        result = support.run_cli(["tasks", "687"], self.transport)
        self.assertIn("tasks 1/3 done", result.stdout)
        self.assertIn("1  [x] Recompute the total when a record is added",
                      result.stdout)

    def test_tick_all_writes_the_body_back(self):
        result = support.run_cli(["tasks", "687", "tick", "all"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        body = self.transport.last("PATCH", "/issues/687").payload["body"]
        self.assertNotIn("- [ ]", body)
        self.assertIn("1/3 -> 3/3", result.stdout)

    def test_code_fence_examples_are_never_ticked(self):
        support.run_cli(["tasks", "690", "tick", "all"], self.transport)
        body = self.transport.last("PATCH", "/issues/690").payload["body"]
        self.assertIn("```md\n- [ ] this example line is documentation", body)
        self.assertIn("- [x] Read the camera pipeline", body)

    def test_tick_by_index_list(self):
        support.run_cli(["tasks", "687", "tick", "2,3"], self.transport)
        self.assertEqual(self.transport.last("PATCH", "/issues/687")
                         .payload["body"].count("- [x]"), 3)

    def test_untick(self):
        support.run_cli(["tasks", "687", "untick", "1"], self.transport)
        self.assertNotIn("- [x] Recompute the total when a record is added",
                         self.transport.last("PATCH", "/issues/687").payload["body"])

    def test_drop_acceptance_criteria(self):
        result = support.run_cli(["tasks", "690", "tick", "all",
                                 "--drop", "Acceptance criteria"], self.transport)
        body = self.transport.last("PATCH", "/issues/690").payload["body"]
        self.assertNotIn("## Acceptance criteria", body)
        self.assertIn("dropped section(s): Acceptance criteria", result.stdout)

    def test_an_out_of_range_index_is_explained(self):
        result = support.run_cli(["tasks", "687", "tick", "99"], self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("does not exist", result.stderr)

    def test_json_mode_reports_the_checklist(self):
        result = support.run_cli(["--json", "tasks", "687"], self.transport)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["progress"], "1/3")
        self.assertEqual(len(payload["tasks"]), 3)


class LabelCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_list(self):
        result = support.run_cli(["label", "list"], self.transport)
        self.assertIn("db-migration", result.stdout)

    def test_status_write_keeps_unrelated_labels(self):
        result = support.run_cli(["label", "status", "687", "responded",
                                 "committed"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        written = self.transport.last("PUT", "/issues/687/labels").payload["labels"]
        self.assertEqual(sorted(written), [29, 62, 65])

    def test_status_clears_the_other_status_labels(self):
        support.run_cli(["label", "status", "687", "working"], self.transport)
        written = self.transport.last("PUT", "/issues/687/labels").payload["labels"]
        self.assertEqual(sorted(written), [29, 63])

    def test_clear_removes_every_status_label(self):
        support.run_cli(["label", "clear", "687"], self.transport)
        written = self.transport.last("PUT", "/issues/687/labels").payload["labels"]
        self.assertEqual(written, [29])

    def test_unknown_status_is_rejected(self):
        result = support.run_cli(["label", "status", "687", "shipped"],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("unknown status", result.stderr)

    def test_add_an_existing_label(self):
        support.run_cli(["label", "add", "687", "db-migration"], self.transport)
        written = self.transport.last("PUT", "/issues/687/labels").payload["labels"]
        self.assertIn(56, written)

    def test_add_an_unknown_label_needs_create(self):
        result = support.run_cli(["label", "add", "687", "🧪 needs test"],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("--create", result.stderr)

    def test_remove_a_label(self):
        support.run_cli(["label", "remove", "687", "bug"], self.transport)
        written = self.transport.last("PUT", "/issues/687/labels").payload["labels"]
        self.assertNotIn(29, written)

    def test_create_is_idempotent(self):
        result = support.run_cli(["label", "create", "bug", "--color", "ee0701"],
                                self.transport)
        self.assertEqual(result.code, 0)
        self.assertIn("already present", result.stdout)
        self.assertEqual(self.transport.find("POST", "/labels"), [])

    def test_bootstrap_creates_the_five_canonical_labels_when_missing(self):
        transport = support.demo_transport()
        transport.route(f"GET {support.REPO_PATH}/labels", [
            {"id": 29, "name": "bug", "color": "ee0701"}])
        result = support.run_cli(["label", "bootstrap"], transport)
        self.assertEqual(result.code, 0, result.stderr)
        created = [call.payload["name"] for call in transport.find("POST", "/labels")]
        self.assertEqual(len(created), 5)
        self.assertIn("👀 seeing", created)

    def test_show_lists_the_issue_labels(self):
        result = support.run_cli(["label", "show", "687"], self.transport)
        self.assertIn("bug", result.stdout)


class MilestoneCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_list_and_latest(self):
        self.assertIn("v3.7.19", support.run_cli(["milestone", "list"],
                                                self.transport).stdout)
        self.assertIn("86", support.run_cli(["milestone", "latest"],
                                           self.transport).stdout)

    def test_set_by_title_sends_the_numeric_id(self):
        result = support.run_cli(["milestone", "set", "687", "v3.7.20"],
                                self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        self.assertEqual(self.transport.last("PATCH", "/issues/687").payload,
                         {"milestone": 86})

    def test_unset_sends_zero(self):
        support.run_cli(["milestone", "unset", "687"], self.transport)
        self.assertEqual(self.transport.last("PATCH", "/issues/687").payload,
                         {"milestone": 0})

    def test_an_unknown_reference_lists_the_choices(self):
        result = support.run_cli(["milestone", "set", "687", "v9.9.9"],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("no milestone matching", result.stderr)
        self.assertIn("v3.7.19", result.stderr)

    def test_create_milestone_with_a_due_date(self):
        result = support.run_cli(["milestone", "create", "v3.8.0",
                                 "--due", "2026-12-31"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        payload = self.transport.last("POST", "/milestones").payload
        self.assertEqual(payload, {"title": "v3.8.0", "description": "",
                                  "due_date": "2026-12-31"})

    def test_create_is_idempotent(self):
        result = support.run_cli(["milestone", "create", "v3.7.19"], self.transport)
        self.assertIn("already exists", result.stdout)
        self.assertEqual(self.transport.find("POST", "/milestones"), [])

    def test_report_shows_progress_per_issue(self):
        result = support.run_cli(["milestone", "report", "v3.7.19"], self.transport)
        self.assertIn("687", result.stdout)
        self.assertIn("1/3", result.stdout)
        self.assertNotIn("690", result.stdout)

    def test_create_milestone_flag_on_set_opens_it(self):
        support.run_cli(["milestone", "set", "687", "v3.9.0",
                        "--create-milestone"], self.transport)
        self.assertTrue(self.transport.find("POST", "/milestones"))

    def test_state_close(self):
        result = support.run_cli(["milestone", "state", "v3.7.19", "closed"],
                                self.transport)
        self.assertEqual(result.code, 0, result.stderr)


class StateCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_close_without_yes_is_refused_and_sends_nothing(self):
        result = support.run_cli(["close", "687"], self.transport)
        self.assertEqual(result.code, cli.POLICY_EXIT)
        self.assertIn("human decision", result.stderr)
        self.assertEqual(self.transport.writes(), [])

    def test_close_with_yes_patches_the_state(self):
        result = support.run_cli(["close", "687", "--yes"], self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        self.assertEqual(self.transport.last("PATCH", "/issues/687").payload,
                         {"state": "closed"})

    def test_close_can_post_a_note_first(self):
        support.run_cli(["close", "687", "--yes", "--note", "verified by QA"],
                        self.transport)
        self.assertEqual(self.transport.find("POST", "/comments")[0]
                         .payload["body"], "verified by QA")
        self.assertEqual(self.transport.writes()[-1].method, "PATCH")

    def test_reopen(self):
        support.run_cli(["reopen", "700"], self.transport)
        self.assertEqual(self.transport.last("PATCH", "/issues/700").payload,
                         {"state": "open"})


class EditCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_title_only_change(self):
        result = support.run_cli(["edit", "687", "--title", "New title"],
                                self.transport)
        self.assertEqual(result.code, 0, result.stderr)
        self.assertEqual(self.transport.last("PATCH", "/issues/687").payload,
                         {"title": "New title"})

    def test_related_references_are_appended_to_the_body(self):
        support.run_cli(["edit", "687", "--related", "620"], self.transport)
        body = self.transport.last("PATCH", "/issues/687").payload["body"]
        self.assertIn("- #615", body)
        self.assertIn("- #620", body)

    def test_assignees_are_written_as_a_full_list(self):
        support.run_cli(["edit", "687", "--unassign", "bot", "--assign", "kate"],
                        self.transport)
        self.assertEqual(
            self.transport.last("PATCH", "/issues/687").payload["assignees"],
            ["kate"])

    def test_labels_move_without_touching_the_issue_patch(self):
        support.run_cli(["edit", "687", "--label-add", "db-migration"],
                        self.transport)
        self.assertTrue(self.transport.find("PUT", "/issues/687/labels"))

    def test_a_noncompliant_new_body_is_refused(self):
        result = support.run_cli(["edit", "687", "--body", "prose only"],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertEqual(self.transport.find("PATCH", "/issues/687"), [])

    def test_edit_without_changes_is_a_usage_error(self):
        self.assertEqual(support.run_cli(["edit", "687"], self.transport).code, 1)


class ApiCommandTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()

    def test_raw_get_prints_json(self):
        result = support.run_cli(
            ["api", "GET", f"repos/{support.REPO}/issues/687"], self.transport)
        self.assertEqual(json.loads(result.stdout)["number"], 687)

    def test_raw_post_sends_the_payload(self):
        support.run_cli(["api", "POST", f"repos/{support.REPO}/issues",
                        "--data", '{"title":"x"}'], self.transport)
        self.assertEqual(self.transport.last("POST", "/issues").payload,
                         {"title": "x"})

    def test_invalid_json_is_reported(self):
        result = support.run_cli(["api", "POST", f"repos/{support.REPO}/issues",
                                 "--data", "{not json}"], self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("not valid JSON", result.stderr)

    def test_a_404_from_an_unknown_path_is_an_error(self):
        result = support.run_cli(["api", "GET", "repos/Atom/demo/nope"],
                                self.transport)
        self.assertEqual(result.code, 1)
        self.assertIn("hint", result.stderr)


class TemplateCommandTests(unittest.TestCase):
    def test_prints_the_standard_body(self):
        result = support.run_cli(["template"], support.demo_transport())
        self.assertIn("## Tasks", result.stdout)
        self.assertIn("- [ ]", result.stdout)

    def test_writes_a_file_and_refuses_to_clobber_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "sub" / "issue.md"
            first = support.run_cli(["template", "--out", str(path)],
                                    support.demo_transport())
            self.assertEqual(first.code, 0)
            self.assertTrue(path.is_file())
            second = support.run_cli(["template", "--out", str(path)],
                                    support.demo_transport())
            self.assertEqual(second.code, 1)
            forced = support.run_cli(["template", "--out", str(path), "--force"],
                                    support.demo_transport())
            self.assertEqual(forced.code, 0)


class UsageTests(unittest.TestCase):
    def test_no_command_prints_help(self):
        result = support.run_cli([], support.demo_transport())
        self.assertEqual(result.code, 2)
        self.assertIn("usage:", result.stdout.lower())

    def test_unknown_command_is_rejected_by_argparse(self):
        with self.assertRaises(SystemExit):
            support.run_cli(["destroy"], support.demo_transport())

    def test_server_side_error_carries_a_hint(self):
        transport = support.demo_transport()
        transport.unroute(f"GET {support.REPO_PATH}/issues/687")
        result = support.run_cli(["view", "687"], transport)
        self.assertEqual(result.code, 1)
        self.assertIn("case-sensitive", result.stderr)


if __name__ == "__main__":
    unittest.main()
