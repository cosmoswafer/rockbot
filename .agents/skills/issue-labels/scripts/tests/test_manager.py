"""Tests for StatusManager apply/reconcile/migrate."""

import unittest

from issue_labels.labels import (
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    status_label_name,
)
from tests.fakes import comment, issue, make_manager


class ApplyTests(unittest.TestCase):
    def setUp(self):
        self.manager, self.client, self.logger = make_manager()

    def _labels_by_name(self):
        return {label["name"]: label["id"] for label in self.client.labels}

    def test_apply_merges_label_set_and_strips_legacy(self):
        self.client.labels = [
            {"id": 1, "name": "plan"},         # legacy -> stripped
            {"id": 2, "name": "🤖 agent"},      # unrelated -> preserved
        ]
        self.client.issues = [
            issue(8, labels=[{"id": 1, "name": "plan"},
                             {"id": 2, "name": "🤖 agent"}])
        ]
        changed, text = self.manager.apply(8, ["responded", "committed"])
        self.assertTrue(changed)
        self.assertIn("→ committed,responded", text)
        puts = [w for w in self.client.writes if w[0] == "PUT"]
        self.assertEqual(len(puts), 1)
        merged = set(puts[0][2]["labels"])
        by_name = self._labels_by_name()
        self.assertIn(2, merged)                       # unrelated kept
        self.assertIn(by_name[status_label_name("responded")], merged)
        self.assertIn(by_name[status_label_name("committed")], merged)
        self.assertNotIn(1, merged)                    # legacy stripped
        self.assertNotIn(by_name[status_label_name("seeing")], merged)

    def test_apply_is_idempotent(self):
        self.client.issues = [issue(8)]
        self.manager.apply(8, "seeing")
        # The fake updates the issue's labels; a second apply to the same
        # status must not produce another PUT.
        before = len([w for w in self.client.writes if w[0] == "PUT"])
        changed, text = self.manager.apply(8, "seeing")
        self.assertFalse(changed)
        self.assertIn("already", text)
        after = len([w for w in self.client.writes if w[0] == "PUT"])
        self.assertEqual(before, after)

    def test_apply_rejects_unknown_status(self):
        from issue_labels.manager import LabelError
        with self.assertRaises(LabelError):
            self.manager.apply(8, "bogus")

    def test_apply_legacy_alias_resolves_to_full_set(self):
        self.client.issues = [issue(8)]
        changed, text = self.manager.apply(8, "implement")
        self.assertTrue(changed)
        self.assertIn("→ committed,responded", text)

    def test_apply_renames_bare_label_in_place(self):
        self.client.labels = [{"id": 5, "name": "seeing", "color": "1d76db"}]
        self.client.issues = [issue(8, labels=[{"id": 5, "name": "seeing"}])]
        changed, _ = self.manager.apply(8, "seeing")
        self.assertFalse(changed)              # the label set itself is unchanged
        by_name = self._labels_by_name()
        self.assertEqual(by_name[status_label_name("seeing")], 5)
        self.assertNotIn("seeing", by_name)

    def test_apply_repairs_colour_and_description_drift(self):
        self.client.labels = [
            {
                "id": 5,
                "name": status_label_name("seeing"),
                "color": "000000",
                "description": "a new issue is filed, the agent is reading it",
            }
        ]
        self.client.issues = [
            issue(8, labels=[{"id": 5, "name": status_label_name("seeing")}])
        ]
        changed, _ = self.manager.apply(8, "seeing")
        self.assertFalse(changed)              # the label set itself is unchanged
        patches = [w for w in self.client.writes if w[0] == "PATCH"]
        self.assertEqual(len(patches), 1)
        self.assertEqual(patches[0][2]["color"], STATUS_COLORS["seeing"])
        self.assertEqual(
            patches[0][2]["description"], STATUS_DESCRIPTIONS["seeing"]
        )

    def test_apply_metadata_repair_is_idempotent(self):
        self.client.labels = [
            {"id": 5, "name": status_label_name("seeing"), "color": "000000"}
        ]
        self.client.issues = [
            issue(8, labels=[{"id": 5, "name": status_label_name("seeing")}])
        ]
        self.manager.apply(8, "seeing")
        before = len([w for w in self.client.writes if w[0] == "PATCH"])
        self.manager._ctx = None               # simulate a fresh set-status run
        self.manager.apply(8, "seeing")
        after = len([w for w in self.client.writes if w[0] == "PATCH"])
        self.assertEqual(before, after)


class ReconcileTests(unittest.TestCase):
    def setUp(self):
        self.manager, self.client, self.logger = make_manager()

    def test_summary_only_without_pickup_writes_no_label(self):
        self.client.issues = [issue(9)]
        changed, _, expect = self.manager.reconcile(9)
        self.assertEqual(expect, frozenset())
        self.assertFalse(changed)

    def test_pickup_seeing_is_preserved_until_a_plan_exists(self):
        self.client.labels = [
            {
                "id": 5,
                "name": status_label_name("seeing"),
                "color": STATUS_COLORS["seeing"],
                "description": STATUS_DESCRIPTIONS["seeing"],
            }
        ]
        self.client.issues = [
            issue(9, labels=[{"id": 5, "name": status_label_name("seeing")}])
        ]
        changed, _, expect = self.manager.reconcile(9)
        self.assertEqual(expect, frozenset({"seeing"}))
        self.assertFalse(changed)

    def test_stage2_plan_drops_seeing(self):
        from issue_labels.labels import derive_status

        def derive(body, comments, labels):
            return derive_status(body, labels, stage2_marker=True)

        manager, client, _ = make_manager(derive=derive)
        client.labels = [
            {
                "id": 5,
                "name": status_label_name("seeing"),
                "color": STATUS_COLORS["seeing"],
                "description": STATUS_DESCRIPTIONS["seeing"],
            }
        ]
        client.issues = [
            issue(9, labels=[{"id": 5, "name": status_label_name("seeing")}])
        ]
        changed, text, expect = manager.reconcile(9)
        self.assertEqual(expect, frozenset({"responded"}))
        self.assertTrue(changed)
        self.assertNotIn("seeing", text)

    def test_body_plan_derives_responded_and_notes(self):
        self.client.issues = [
            issue(9, body="## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n")
        ]
        _, _, expect = self.manager.reconcile(9)
        self.assertEqual(expect, frozenset({"responded"}))
        self.assertTrue(any("unmarked body plan" in w for w in self.logger.warnings))

    def test_injected_derive_receives_comments(self):
        seen = {}

        def derive(body, comments, labels):
            seen["comments"] = comments
            return frozenset({"responded"}), False

        manager, client, _ = make_manager(derive=derive)
        client.issues = [issue(9)]
        client.comments[9] = [comment(900, "## Plan")]
        _, _, expect = manager.reconcile(9)
        self.assertEqual(expect, frozenset({"responded"}))
        self.assertEqual([c["id"] for c in seen["comments"]], [900])

    def test_reconcile_keeps_committed_additively(self):
        self.client.labels = [{"id": 7, "name": "committed", "color": "0e8a16"}]
        self.client.issues = [
            issue(9, labels=[{"id": 7, "name": "committed"}])
        ]
        _, _, expect = self.manager.reconcile(9)
        self.assertEqual(expect, frozenset({"committed"}))

    def test_reconcile_accepts_emoji_prefixed_labels(self):
        self.client.labels = [
            {"id": 7, "name": status_label_name("committed"), "color": "0e8a16"}
        ]
        self.client.issues = [
            issue(9, labels=[{"id": 7, "name": status_label_name("committed")}])
        ]
        _, _, expect = self.manager.reconcile(9)
        self.assertEqual(expect, frozenset({"committed"}))


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.manager, self.client, self.logger = make_manager()

    def test_migrate_renames_legacy_labels_in_place_and_creates_question(self):
        self.client.labels = [
            {"id": 1, "name": "inspect", "color": "1d76db"},
            {"id": 2, "name": "plan", "color": "fbca04"},
            {"id": 3, "name": "working", "color": "5319e7"},
            {"id": 4, "name": "implement", "color": "0e8a16"},
        ]
        summary = self.manager.migrate_labels()
        self.assertIn("inspect→seeing", summary)
        self.assertIn("plan→responded", summary)
        self.assertIn("implement→committed", summary)
        names = {label["name"] for label in self.client.labels}
        self.assertTrue(
            {status_label_name(status) for status in
             ("seeing", "responded", "working", "question", "committed")} <= names
        )
        by_name = {label["name"]: label["id"] for label in self.client.labels}
        self.assertEqual(by_name[status_label_name("seeing")], 1)      # IDs preserved
        self.assertEqual(by_name[status_label_name("responded")], 2)
        self.assertEqual(by_name[status_label_name("committed")], 4)
        patches = [w for w in self.client.writes
                   if w[0] == "PATCH" and "/labels/" in w[1]]
        self.assertEqual(len(patches), 4)      # 3 legacy renames + bare `working`

    def test_migrate_is_idempotent(self):
        self.client.labels = [{"id": 1, "name": "inspect"}]
        self.manager.migrate_labels()
        first = len(self.client.writes)
        summary = self.manager.migrate_labels()
        self.assertIn("nothing to rename", summary)
        self.assertEqual(len(self.client.writes), first)


if __name__ == "__main__":
    unittest.main()
