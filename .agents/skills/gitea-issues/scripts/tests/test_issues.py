"""Tests for issue payloads, list filtering, and the rendered views."""

import unittest

from tests import support
from gitea_issues import issues


class PayloadTests(unittest.TestCase):
    def test_milestone_uses_the_numeric_milestone_key(self):
        payload = issues.create_payload("T", "body", milestone_id=87)
        self.assertEqual(payload["milestone"], 87)
        self.assertNotIn("milestone_id", payload)

    def test_absent_fields_are_left_out_rather_than_emptied(self):
        payload = issues.create_payload("T", "body")
        self.assertEqual(sorted(payload), ["body", "title"])
        self.assertNotIn("assignees", payload)
        self.assertNotIn("labels", payload)
        self.assertNotIn("milestone", payload)

    def test_assignees_and_labels_keep_their_api_shapes(self):
        payload = issues.create_payload("T", "body", assignees=["bot"],
                                       label_ids=[29, "62"])
        self.assertEqual(payload["assignees"], ["bot"])
        self.assertEqual(payload["labels"], [29, 62])

    def test_label_ids_are_re_asserted_when_the_server_ignored_them(self):
        transport = support.demo_transport()
        transport.route(f"POST {support.REPO_PATH}/issues",
                        lambda ctx: dict(ctx["payload"], number=905, labels=[],
                                        state="open"))
        transport.route(f"PUT {support.REPO_PATH}/issues/905/labels",
                        lambda ctx: support.labels_for_ids(ctx["payload"]))
        client = support.make_client(transport)
        created = issues.create_issue(client,
                                     issues.create_payload("T", "B", label_ids=[29]),
                                     expected_label_ids=[29])
        self.assertEqual(transport.last("PUT", "/issues/905/labels").payload,
                         {"labels": [29]})
        self.assertEqual([label["id"] for label in created["labels"]], [29])

    def test_no_extra_write_when_the_labels_landed(self):
        transport = support.demo_transport()
        client = support.make_client(transport)
        issues.create_issue(client, issues.create_payload("T", "B", label_ids=[29]),
                           expected_label_ids=[29])
        self.assertEqual(transport.find("PUT", "/labels"), [])


class RowTests(unittest.TestCase):
    def setUp(self):
        self.row = issues.issue_row(support.ISSUES[0], "https://gitea.test")

    def test_checklist_progress_comes_from_the_body(self):
        self.assertEqual(self.row["tasks"], "1/3")

    def test_names_are_flattened_for_printing(self):
        self.assertEqual(self.row["labels"], ["bug", "💬 responded"])
        self.assertEqual(self.row["assignees"], ["bot"])
        self.assertEqual(self.row["milestone"], "v3.7.19")

    def test_url_falls_back_to_the_repo_url(self):
        row = issues.issue_row({"number": 7, "title": "t"}, "https://gitea.test")
        self.assertEqual(row["url"], "https://gitea.test/issues/7")

    def test_render_includes_the_body_and_the_thread(self):
        text = issues.render_issue(self.row, support.BODY_687,
                                   support.COMMENTS)
        self.assertIn("#687", text)
        self.assertIn("stale total", text)
        self.assertIn("comment 500 by bot", text)
        self.assertIn("https://gitea.test", text)


class ListTests(unittest.TestCase):
    def setUp(self):
        self.client = support.make_client(support.demo_transport())

    def test_pull_requests_are_dropped_by_default(self):
        numbers = [row["number"] for row in issues.list_issues(self.client)]
        self.assertEqual(numbers, [687, 690])

    def test_pull_requests_can_be_kept(self):
        numbers = [row["number"] for row in
                   issues.list_issues(self.client, includes_pulls=True)]
        self.assertIn(701, numbers)

    def test_state_all_includes_closed_issues(self):
        numbers = [row["number"] for row in
                   issues.list_issues(self.client, state="all")]
        self.assertIn(700, numbers)

    def test_label_filter_matches_name_or_bare_form(self):
        rows = issues.list_issues(self.client, labels=["working"])
        self.assertEqual([row["number"] for row in rows], [690])
        rows = issues.list_issues(self.client, labels=["🔨 working"])
        self.assertEqual([row["number"] for row in rows], [690])

    def test_a_label_nobody_carries_matches_nothing(self):
        self.assertEqual(issues.list_issues(self.client, labels=["seeing"]), [])

    def test_milestone_filter_matches_title_or_id(self):
        self.assertEqual([row["number"] for row in
                          issues.list_issues(self.client, milestone="v3.7.20")], [690])
        self.assertEqual([row["number"] for row in
                          issues.list_issues(self.client, milestone="87")], [687])

    def test_limit_caps_the_result(self):
        self.assertEqual(len(issues.list_issues(self.client, state="all",
                                               limit=2)), 2)

    def test_assignee_and_search_are_passed_through_as_query_params(self):
        transport = support.demo_transport()
        issues.list_issues(support.make_client(transport), assignee="bot",
                          search="ring")
        query = transport.last("GET", "/issues").query
        self.assertEqual(query["assignee"], "bot")
        self.assertEqual(query["q"], "ring")


class FetchTests(unittest.TestCase):
    def test_fetch_issue_returns_the_object(self):
        client = support.make_client(support.demo_transport())
        self.assertEqual(issues.fetch_issue(client, 690)["state"], "open")

    def test_a_non_issue_response_is_an_error(self):
        transport = support.demo_transport()
        transport.route(f"GET {support.REPO_PATH}/issues/1234",
                        {"message": "not found"})
        with self.assertRaises(LookupError):
            issues.fetch_issue(support.make_client(transport), 1234)

    def test_comments_are_returned_in_order(self):
        client = support.make_client(support.demo_transport())
        self.assertEqual([item["id"] for item in
                          issues.fetch_comments(client, 687)], [500, 501])


class CommentTests(unittest.TestCase):
    def test_blank_comments_are_refused_before_the_write(self):
        transport = support.demo_transport()
        with self.assertRaises(ValueError):
            issues.post_comment(support.make_client(transport), 687, "   ")
        self.assertEqual(transport.find("POST", "/comments"), [])

    def test_state_values_are_validated(self):
        with self.assertRaises(ValueError):
            issues.set_state(support.make_client(support.demo_transport()),
                            687, "archived")


class AssigneeTests(unittest.TestCase):
    def test_add_and_remove_in_one_pass(self):
        self.assertEqual(issues.merge_assignees(["bot", "jimmy"], ["kate"],
                                                ["bot"]), ["jimmy", "kate"])

    def test_no_duplicates_are_added(self):
        self.assertEqual(issues.merge_assignees(["bot"], ["bot"], []), ["bot"])


class RelatedTests(unittest.TestCase):
    def test_references_are_appended_to_an_existing_section(self):
        body = issues.append_related(support.BODY_687, ["620"])
        self.assertIn("- #615\n- #620", body)

    def test_a_missing_section_is_created_at_the_end(self):
        body = issues.append_related("## Summary\n\nx\n\n## Tasks\n\n- [ ] a\n",
                                     ["#615"])
        self.assertTrue(body.rstrip().endswith("- #615"))
        self.assertIn("## Related", body)

    def test_no_references_changes_nothing(self):
        self.assertEqual(issues.append_related("body", []), "body")


if __name__ == "__main__":
    unittest.main()
