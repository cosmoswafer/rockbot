"""Tests for the label vocabulary and the full-set write rule."""

import unittest

from tests import support
from gitea_issues import labels


class NameTests(unittest.TestCase):
    def test_bare_strips_the_emoji_prefix(self):
        self.assertEqual(labels.bare("👀 seeing"), "seeing")
        self.assertEqual(labels.bare("bug"), "bug")
        self.assertEqual(labels.bare("  🔨 working  "), "working")

    def test_is_status_label_covers_legacy_spellings(self):
        for name in ("👀 seeing", "seeing", "plan", "implement", "✅ committed",
                   "done", "questions"):
            self.assertTrue(labels.is_status_label(name), name)

    def test_unrelated_labels_are_not_status_labels(self):
        for name in ("bug", "db-migration", "enhancement", ""):
            self.assertFalse(labels.is_status_label(name), name)


class TokenTests(unittest.TestCase):
    def test_bare_and_emoji_spellings_agree(self):
        self.assertEqual(labels.expand_tokens(["responded"]),
                         labels.expand_tokens(["💬 responded"]))

    def test_legacy_names_expand_to_the_current_set(self):
        self.assertEqual(labels.expand_tokens(["implement"]),
                         ["💬 responded", "✅ committed"])
        self.assertEqual(labels.expand_tokens(["inspect"]), ["👀 seeing"])

    def test_none_clears_every_status(self):
        self.assertEqual(labels.expand_tokens(["none"]), [])
        self.assertEqual(labels.expand_tokens(["committed", "none"]),
                         ["✅ committed"])

    def test_duplicates_are_dropped_and_order_kept(self):
        self.assertEqual(labels.expand_tokens(["committed", "responded",
                                             "✅ committed"]),
                         ["✅ committed", "💬 responded"])

    def test_unknown_status_names_the_choices(self):
        with self.assertRaises(labels.LabelError) as caught:
            labels.expand_tokens(["shipped"])
        self.assertIn("seeing", str(caught.exception))
        self.assertIn("unknown status", str(caught.exception))


class ColorTests(unittest.TestCase):
    def test_hash_prefix_is_stripped_and_case_lowered(self):
        self.assertEqual(labels.normalize_color("#0E8A16", "x"), "0e8a16")
        self.assertEqual(labels.normalize_color("", "x"), labels.DEFAULT_COLOR)

    def test_bad_color_is_rejected_before_the_write(self):
        with self.assertRaises(labels.LabelError):
            labels.normalize_color("green", "x")


class MergeTests(unittest.TestCase):
    def test_unrelated_labels_survive_a_status_write(self):
        current = [{"id": 29, "name": "bug"}, {"id": 62, "name": "💬 responded"}]
        merged = labels.merge_labels(current, [65], [], drop_status=True)
        self.assertEqual(merged, [29, 65])

    def test_legacy_status_labels_are_dropped_when_a_new_set_is_written(self):
        current = [{"id": 7, "name": "plan"}, {"id": 29, "name": "bug"}]
        merged = labels.merge_labels(current, [62], [], drop_status=True)
        self.assertEqual(merged, [29, 62])

    def test_removal_by_id(self):
        current = [{"id": 29, "name": "bug"}, {"id": 56, "name": "db-migration"}]
        self.assertEqual(labels.merge_labels(current, [], [56]), [29])

    def test_ids_are_never_duplicated(self):
        current = [{"id": 29, "name": "bug"}]
        self.assertEqual(labels.merge_labels(current, [29, 62], []), [29, 62])


class IndexTests(unittest.TestCase):
    def setUp(self):
        self.index = labels.label_index(support.LABELS)

    def test_exact_case_and_emoji_insensitive_lookup(self):
        self.assertEqual(labels.find_label_id(self.index, "bug"), 29)
        self.assertEqual(labels.find_label_id(self.index, "BUG"), 29)
        self.assertEqual(labels.find_label_id(self.index, "👀 seeing"), 61)
        self.assertEqual(labels.find_label_id(self.index, "seeing"), 61)
        self.assertIsNone(labels.find_label_id(self.index, "nope"))

    def test_ensure_labels_is_idempotent_for_existing_names(self):
        transport = support.demo_transport()
        client = support.make_client(transport)
        result = labels.ensure_labels(client, [("bug", "ee0701", "a bug")])
        self.assertEqual(result["ids"]["bug"], 29)
        self.assertEqual(result["existed"], ["bug"])
        self.assertEqual(transport.find("POST", "/labels"), [])

    def test_ensure_labels_creates_what_is_missing(self):
        transport = support.demo_transport()
        client = support.make_client(transport)
        result = labels.ensure_labels(client, [("🚀 shipped", "00ff00", "done")])
        self.assertEqual(result["created"], ["🚀 shipped"])
        self.assertEqual(result["ids"]["🚀 shipped"], 99)
        call = transport.last("POST", "/labels")
        self.assertEqual(call.payload["color"], "00ff00")
        self.assertEqual(call.payload["name"], "🚀 shipped")

    def test_dry_run_creates_nothing(self):
        transport = support.demo_transport()
        client = support.make_client(transport)
        result = labels.ensure_labels(client, ["🚀 shipped"], dry_run=True)
        self.assertEqual(result["created"], ["🚀 shipped"])
        self.assertTrue(result["dry_run"])
        self.assertEqual(transport.find("POST", "/labels"), [])


class ResolveTests(unittest.TestCase):
    def setUp(self):
        self.client = support.make_client(support.demo_transport())

    def test_missing_label_reports_the_available_names(self):
        with self.assertRaises(labels.LabelError) as caught:
            labels.resolve_label_ids(self.client, ["nope"])
        self.assertIn("no such label", str(caught.exception))
        self.assertIn("bug", str(caught.exception))

    def test_create_flag_makes_the_label(self):
        result = labels.resolve_label_ids(self.client, ["🧪 needs test"],
                                        create=True)
        self.assertEqual(result["ids"], [99])

    def test_empty_input_is_not_an_error(self):
        self.assertEqual(labels.resolve_label_ids(self.client, [])["ids"], [])


class StatusWriteTests(unittest.TestCase):
    def setUp(self):
        self.transport = support.demo_transport()
        self.client = support.make_client(self.transport)

    def test_set_status_writes_the_full_set_through_the_labels_endpoint(self):
        result = labels.set_status(self.client, 687, ["responded", "committed"])
        call = self.transport.last("PUT", "/issues/687/labels")
        self.assertEqual(call.payload["labels"], [29, 62, 65])
        self.assertIn("✅ committed", result["applied"])

    def test_status_none_keeps_only_unrelated_labels(self):
        labels.set_status(self.client, 687, ["none"])
        call = self.transport.last("PUT", "/issues/687/labels")
        self.assertEqual(call.payload["labels"], [29])

    def test_legacy_status_on_the_issue_is_replaced(self):
        self.transport.route(f"GET {support.REPO_PATH}/issues/690/labels",
                             [{"id": 29, "name": "bug"},
                              {"id": 12, "name": "implement"}])
        labels.set_status(self.client, 690, ["working"])
        call = self.transport.last("PUT", "/issues/690/labels")
        self.assertEqual(call.payload["labels"], [29, 63])

    def test_status_creation_happens_when_the_repo_lacks_the_label(self):
        self.transport.route(f"GET {support.REPO_PATH}/labels", [
            {"id": 29, "name": "bug", "color": "ee0701"}])
        labels.set_status(self.client, 687, ["committed"])
        self.assertTrue(self.transport.find("POST", "/labels"))

    def test_issue_labels_are_read_from_the_issue_endpoint(self):
        names = [item["name"] for item in labels.issue_labels(self.client, 687)]
        self.assertEqual(names, ["bug", "💬 responded"])


if __name__ == "__main__":
    unittest.main()
