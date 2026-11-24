"""Tests for milestone resolution and the assignment payload shape."""

import unittest

from tests import support
from gitea_issues import milestones


class ResolveTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()
        self.client = support.make_client(self.transport)

    def test_numeric_id(self):
        self.assertEqual(milestones.resolve(self.client, "87")["title"], "v3.7.19")

    def test_exact_title_case_insensitive(self):
        self.assertEqual(milestones.resolve(self.client, "V3.7.20")["id"], 86)

    def test_unique_fragment(self):
        self.assertEqual(milestones.resolve(self.client, "3.7.19")["id"], 87)

    def test_ambiguous_fragment_is_refused(self):
        with self.assertRaises(milestones.MilestoneError) as caught:
            milestones.resolve(self.client, "v3")
        self.assertIn("ambiguous", str(caught.exception))

    def test_unknown_id_names_the_open_choices(self):
        with self.assertRaises(milestones.MilestoneError) as caught:
            milestones.resolve(self.client, "999")
        self.assertIn("no milestone with id 999", str(caught.exception))
        self.assertIn("v3.7.19", str(caught.exception))

    def test_unknown_title_offers_to_create(self):
        with self.assertRaises(milestones.MilestoneError) as caught:
            milestones.resolve(self.client, "v3.8.0")
        self.assertIn("--create-milestone", str(caught.exception))

    def test_create_flag_opens_the_milestone(self):
        item = milestones.resolve(self.client, "v3.8.0", create=True,
                                 due="2027-01-31")
        self.assertEqual(item["id"], 88)
        call = self.transport.last("POST", "/milestones")
        self.assertEqual(call.payload["due_date"], "2027-01-31")

    def test_empty_reference_is_rejected(self):
        with self.assertRaises(milestones.MilestoneError):
            milestones.resolve(self.client, "")


class LatestTests(unittest.TestCase):
    def test_newest_open_milestone_by_creation_date(self):
        transport = support.demo_transport()
        self.assertEqual(milestones.latest(support.make_client(transport))["id"], 86)

    def test_closed_milestones_are_ignored_by_default(self):
        transport = support.demo_transport()
        self.assertEqual(milestones.latest(support.make_client(transport),
                                          state="all")["id"], 86)

    def test_no_milestone_gives_an_actionable_message(self):
        transport = support.demo_transport()
        transport.route(f"GET {support.REPO_PATH}/milestones", [])
        with self.assertRaises(milestones.MilestoneError) as caught:
            milestones.latest(support.make_client(transport))
        self.assertIn("milestone create", str(caught.exception))


class CreateTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()
        self.client = support.make_client(self.transport)

    def test_existing_title_is_returned_without_a_second_write(self):
        item, created = milestones.create_milestone(self.client, "v3.7.19")
        self.assertFalse(created)
        self.assertEqual(item["id"], 87)
        self.assertEqual(self.transport.find("POST", "/milestones"), [])

    def test_due_date_format_is_validated(self):
        with self.assertRaises(milestones.MilestoneError) as caught:
            milestones.create_milestone(self.client, "v3.8.0", due="31/12/2026")
        self.assertIn("YYYY-MM-DD", str(caught.exception))

    def test_blank_title_is_rejected(self):
        with self.assertRaises(milestones.MilestoneError):
            milestones.create_milestone(self.client, "   ")

    def test_created_milestone_carries_the_description(self):
        milestones.create_milestone(self.client, "v3.8.0",
                                   description="spring cleanup")
        call = self.transport.last("POST", "/milestones")
        self.assertEqual(call.payload["description"], "spring cleanup")


class AssignTests(unittest.TestCase):
    def test_assignment_uses_the_milestone_key_with_a_numeric_id(self):
        transport = support.demo_transport()
        milestones.assign(support.make_client(transport), 687, 86)
        call = transport.last("PATCH", "/issues/687")
        self.assertEqual(call.payload, {"milestone": 86})
        self.assertNotIn("milestone_id", call.payload)

    def test_zero_clears_the_milestone(self):
        transport = support.demo_transport()
        milestones.assign(support.make_client(transport), 687, 0)
        self.assertEqual(transport.last("PATCH", "/issues/687").payload,
                         {"milestone": 0})

    def test_state_write(self):
        transport = support.demo_transport()
        milestones.set_state(support.make_client(transport), 87, "closed")
        self.assertEqual(transport.last("PATCH", "/milestones/87").payload,
                         {"state": "closed"})

    def test_bad_state_is_rejected(self):
        with self.assertRaises(milestones.MilestoneError):
            milestones.set_state(support.make_client(support.demo_transport()),
                                87, "archived")


class SummaryTests(unittest.TestCase):
    def test_rows_carry_the_counters_the_review_needs(self):
        transport = support.demo_transport()
        rows = milestones.summary_rows(
            milestones.list_milestones(support.make_client(transport), "all"))
        self.assertEqual({row["title"] for row in rows},
                         {"v3.7.19", "v3.7.20", "v3.7.18"})
        self.assertEqual(rows[0]["open"], 3)


if __name__ == "__main__":
    unittest.main()
