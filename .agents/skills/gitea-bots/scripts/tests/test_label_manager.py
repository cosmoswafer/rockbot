"""Tests for StatusManager apply/reconcile/migrate."""

import unittest

from issue_labels.gitea import GiteaError
from issue_labels.labels import (
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    status_label_name,
)
from tests.label_fakes import comment, FakeClient, issue, make_manager


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


class EmojiHostileServerTests(unittest.TestCase):
    """Label writes must survive a Gitea whose store rejects emoji names.

    Such a server answers any label write whose name carries an emoji with a
    5xx (the 4-byte characters do not fit), which used to abort every status
    label write. The manager must degrade to the bare canonical spelling.
    """

    def setUp(self):
        self.client = FakeClient(reject_emoji=True)
        self.manager, self.client, self.logger = make_manager(client=self.client)

    def _by_name(self):
        return {label["name"]: label for label in self.client.labels}

    def test_create_falls_back_to_the_bare_spelling(self):
        self.client.issues = [issue(8)]
        changed, text = self.manager.apply(8, "seeing")
        self.assertTrue(changed)
        self.assertIn("→ seeing", text)
        by_name = self._by_name()
        self.assertNotIn(status_label_name("seeing"), by_name)
        self.assertIn("seeing", by_name)
        self.assertEqual(by_name["seeing"]["color"], STATUS_COLORS["seeing"])
        self.assertEqual(
            by_name["seeing"]["description"], STATUS_DESCRIPTIONS["seeing"]
        )
        self.assertTrue(any("bare spelling" in w for w in self.logger.warnings))

    def test_half_succeeded_create_is_adopted_without_duplicates(self):
        self.client.reject_emoji = False
        self.client.strip_on_reject = True
        self.client.issues = [issue(8)]
        changed, _ = self.manager.apply(8, "seeing")
        self.assertTrue(changed)
        names = [label["name"] for label in self.client.labels]
        self.assertEqual(len(names), len(set(names)))  # the stripped label was
        self.assertIn("seeing", names)                 # adopted, not re-created
        puts = [w for w in self.client.writes if w[0] == "PUT"]
        by_id = {label["id"]: label for label in self.client.labels}
        self.assertEqual(
            [by_id[i]["name"] for i in puts[0][2]["labels"]], ["seeing"]
        )

    def test_existing_bare_label_is_kept_not_renamed(self):
        self.client.labels = [
            {
                "id": 5,
                "name": "seeing",
                "color": STATUS_COLORS["seeing"],
                "description": STATUS_DESCRIPTIONS["seeing"],
            }
        ]
        self.client.issues = [issue(8, labels=[{"id": 5, "name": "seeing"}])]
        changed, text = self.manager.apply(8, "seeing")
        self.assertFalse(changed)
        self.assertIn("already", text)
        self.assertEqual(self._by_name()["seeing"]["id"], 5)

    def test_metadata_drift_is_repaired_with_the_bare_name(self):
        self.client.labels = [
            {"id": 5, "name": "seeing", "color": "000000", "description": "stale"}
        ]
        self.client.issues = [issue(8, labels=[{"id": 5, "name": "seeing"}])]
        self.manager.apply(8, "seeing")
        label = self._by_name()["seeing"]
        self.assertEqual(label["color"], STATUS_COLORS["seeing"])
        self.assertEqual(label["description"], STATUS_DESCRIPTIONS["seeing"])

    def test_drifted_display_label_is_healed_down_to_bare(self):
        emoji = status_label_name("seeing")
        self.client.labels = [
            {
                "id": 5,
                "name": emoji,
                "color": "000000",
                "description": STATUS_DESCRIPTIONS["seeing"],
            }
        ]
        self.client.issues = [issue(8, labels=[{"id": 5, "name": emoji}])]
        changed, _ = self.manager.apply(8, "seeing")
        self.assertFalse(changed)  # the issue's label set was already correct
        by_name = self._by_name()
        self.assertNotIn(emoji, by_name)
        self.assertEqual(by_name["seeing"]["id"], 5)  # id survived the heal
        self.assertEqual(by_name["seeing"]["color"], STATUS_COLORS["seeing"])

    def test_degradation_skips_later_emoji_attempts_in_the_same_run(self):
        self.client.issues = [issue(8)]
        self.manager.apply(8, "committed")
        posts = [w for w in self.client.writes if w[0] == "POST"]
        emoji_posts = [
            w for w in posts
            if any(ord(ch) > 0xFFFF for ch in w[2]["name"])
        ]
        # seeing (which trips the 5xx) is the only emoji attempt; once the run
        # degrades, responded/working/question/committed POST bare names only.
        self.assertEqual(len(emoji_posts), 1)

    def test_canonical_question_wins_over_the_stock_default_label(self):
        self.client.reject_emoji = False
        self.client.strip_on_reject = True
        self.client.labels = [
            {"id": 34, "name": "question", "color": "cc317c",
             "description": "More information is needed"},  # Gitea default
            {"id": 64, "name": "question", "color": STATUS_COLORS["question"],
             "description": STATUS_DESCRIPTIONS["question"]},  # canonical
        ]
        self.client.issues = [issue(8)]
        self.manager.apply(8, "question")
        puts = [w for w in self.client.writes if w[0] == "PUT"]
        self.assertEqual(puts[0][2]["labels"], [64])
        patches = [w for w in self.client.writes if w[0] == "PATCH"]
        self.assertEqual(patches, [])  # the stock label is left alone

    def test_client_errors_are_not_degraded_away(self):
        self.client.reject_status = 422
        self.client.issues = [issue(8)]
        with self.assertRaises(GiteaError):
            self.manager.apply(8, "seeing")
        self.assertTrue(self.manager._emoji_ok)  # no unjust bare-name fallback
        self.assertEqual(self.client.labels, [])

    def test_reconcile_writes_labels_on_a_hostile_server(self):
        self.client.issues = [
            issue(9, body="## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n")
        ]
        changed, _, expect = self.manager.reconcile(9)
        self.assertTrue(changed)
        self.assertEqual(expect, frozenset({"responded"}))
        puts = [w for w in self.client.writes if w[0] == "PUT"]
        by_id = {label["id"]: label["name"] for label in self.client.labels}
        self.assertEqual([by_id[i] for i in puts[0][2]["labels"]], ["responded"])


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
