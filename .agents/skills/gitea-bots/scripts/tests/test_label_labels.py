"""Tests for the pure label vocabulary, body helpers and derivation."""

import unittest

from issue_labels.labels import (
    LEGACY_NAMES,
    STATUS_ALIASES,
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    STATUS_LABEL_NAMES,
    STATUS_NAMES,
    derive_status,
    has_summary,
    has_tasks,
    label_status,
    normalize_status,
    normalize_status_set,
    status_label_name,
)

class VocabularyTests(unittest.TestCase):
    def test_canonical_set_is_general_one_word_labels(self):
        self.assertEqual(
            STATUS_NAMES, ("seeing", "responded", "working", "question", "committed")
        )
        self.assertEqual(STATUS_COLORS["question"], "d93f0b")
        # Legacy/retired names are still owned by the label set (so reconcile
        # can find and strip them from issues) but are not canonical.
        for name in ("inspect", "plan", "implement", "questions", "done"):
            self.assertIn(name, STATUS_LABEL_NAMES)
            self.assertNotIn(name, STATUS_NAMES)
        self.assertEqual(set(LEGACY_NAMES), {"inspect", "plan", "implement"})

    def test_status_label_name_prefixes_each_canonical_label(self):
        self.assertEqual(status_label_name("seeing"), "👀 seeing")
        self.assertEqual(status_label_name("committed"), "✅ committed")
        for name in STATUS_NAMES:
            self.assertTrue(status_label_name(name).endswith(" " + name))

    def test_seeing_describes_a_pickup_not_a_filing(self):
        self.assertEqual(
            STATUS_DESCRIPTIONS["seeing"],
            "a background agent has picked the issue up and is working on it",
        )

    def test_label_status_strips_the_emoji_prefix(self):
        for name in STATUS_NAMES:
            self.assertEqual(label_status(status_label_name(name)), name)
            self.assertEqual(label_status(name), name)          # bare spelling
        self.assertEqual(label_status("inspect"), "inspect")    # legacy, still owned
        self.assertIsNone(label_status("🤖 agent"))            # unrelated label
        self.assertIsNone(label_status(""))
        self.assertIsNone(label_status(None))


class BodySectionTests(unittest.TestCase):
    def test_body_sections(self):
        body = "## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n"
        self.assertTrue(has_summary(body))
        self.assertTrue(has_tasks(body))
        self.assertFalse(has_tasks("## Summary only\n"))
        # trailing whitespace on the heading is accepted
        self.assertTrue(has_tasks("## Tasks   \n\n- [ ] x\n"))


class DerivationTests(unittest.TestCase):
    """The derived status is a *set* of general-purpose labels."""

    def test_stage3_marker_yields_responded_and_committed(self):
        self.assertEqual(
            derive_status("anything", [], stage3_marker=True),
            (frozenset({"responded", "committed"}), False),
        )

    def test_stage2_marker(self):
        self.assertEqual(
            derive_status("anything", [], stage2_marker=True),
            (frozenset({"responded"}), False),
        )

    def test_summary_only_never_invents_seeing(self):
        # `seeing` means a background agent picked the issue up; the projection
        # must not label an unpicked issue.
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", []),
            (frozenset(), False),
        )
        # A pickup label is preserved while the issue is still unplanned ...
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["seeing"]),
            (frozenset({"seeing"}), False),
        )
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["👀 seeing"]),
            (frozenset({"seeing"}), False),
        )
        # ... and dropped once a plan exists.
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["seeing"], stage2_marker=True),
            (frozenset({"responded"}), False),
        )
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["seeing"], stage3_marker=True),
            (frozenset({"responded", "committed"}), False),
        )

    def test_no_markers_no_summary_is_empty_set(self):
        self.assertEqual(derive_status("random", []), (frozenset(), False))

    def test_unmarked_body_plan_is_flagged(self):
        body = "## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n"
        self.assertEqual(
            derive_status(body, []), (frozenset({"responded"}), True)
        )
        # A plan exists, so it also drops a stale pickup label.
        self.assertEqual(
            derive_status(body, ["seeing"]), (frozenset({"responded"}), True)
        )
        # A stage-2 marker makes the body plan canonical (no NOTE).
        self.assertEqual(
            derive_status(body, [], stage2_marker=True),
            (frozenset({"responded"}), False),
        )

    def test_committed_is_additive_and_sticky(self):
        # No pickup label to preserve: `seeing` is not invented.
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["committed"]),
            (frozenset({"committed"}), False),
        )
        # An existing pickup label survives alongside `committed`.
        self.assertEqual(
            derive_status("## Summary\n\nHi.\n", ["seeing", "committed"]),
            (frozenset({"seeing", "committed"}), False),
        )
        self.assertEqual(
            derive_status("anything", [], stage3_marker=True),
            (frozenset({"responded", "committed"}), False),
        )

    def test_working_is_sticky_while_stage3_in_flight(self):
        self.assertEqual(
            derive_status("body", ["working"], stage2_marker=True),
            (frozenset({"working"}), False),
        )
        # Emoji-prefixed repo labels are normalized before deriving the set.
        self.assertEqual(
            derive_status("body", ["🔨 working", "🤖 agent"], stage2_marker=True),
            (frozenset({"working"}), False),
        )
        # A stage-3 marker ends the in-flight state.
        self.assertEqual(
            derive_status("body", ["working"], stage3_marker=True),
            (frozenset({"responded", "committed"}), False),
        )

    def test_question_is_transient_and_sticky_until_replaced(self):
        self.assertEqual(
            derive_status("body", ["question"], stage2_marker=True),
            (frozenset({"question"}), False),
        )
        self.assertEqual(normalize_status_set(["responded"]),
                         frozenset({"responded"}))


class NormalizeTests(unittest.TestCase):
    def test_normalize_status_set_canonical_and_multiple(self):
        self.assertEqual(normalize_status_set("responded"), frozenset({"responded"}))
        self.assertEqual(
            normalize_status_set(["responded", "committed"]),
            frozenset({"responded", "committed"}),
        )
        self.assertEqual(
            normalize_status_set("responded,committed"),
            frozenset({"responded", "committed"}),
        )
        self.assertEqual(normalize_status_set("none"), frozenset())
        self.assertEqual(normalize_status_set(frozenset({"seeing"})),
                         frozenset({"seeing"}))
        self.assertIsNone(normalize_status_set("bogus"))

    def test_legacy_aliases_resolve_to_full_label_sets(self):
        self.assertEqual(normalize_status_set("inspect"), frozenset({"seeing"}))
        self.assertEqual(normalize_status_set("plan"), frozenset({"responded"}))
        self.assertEqual(
            normalize_status_set("implement"),
            frozenset({"responded", "committed"}),
        )
        self.assertEqual(normalize_status_set("questions"), frozenset({"question"}))
        self.assertEqual(
            normalize_status_set("done"), frozenset({"responded", "committed"})
        )
        self.assertEqual(
            set(STATUS_ALIASES),
            {"inspect", "plan", "implement", "questions", "done"},
        )

    def test_normalize_status_is_backward_compatible_alias(self):
        self.assertEqual(normalize_status("responded"), normalize_status_set("responded"))
        self.assertIsNone(normalize_status("bogus"))


if __name__ == "__main__":
    unittest.main()
