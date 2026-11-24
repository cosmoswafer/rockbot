"""Tests for checklist parsing and body editing."""

import unittest

from gitea_issues import tasks

BODY = """## Summary

The ring shows a stale total after a delete.

## Tasks

- [x] Recompute on add
- [ ] Recompute on delete
  - [ ] Cover the empty day case

```md
- [ ] this example is documentation, not a task
```

## Related

- #615
"""

WITH_ACCEPTANCE = """## Summary

s

## Tasks

- [ ] a

## Acceptance criteria

- [ ] b

### Detail

more text

## Related

- #1
"""


class ParseTests(unittest.TestCase):
    def setUp(self):
        self.items = tasks.parse_tasks(BODY)

    def test_real_tasks_only(self):
        self.assertEqual([item.text for item in self.items], [
            "Recompute on add",
            "Recompute on delete",
            "Cover the empty day case",
        ])

    def test_examples_inside_code_fences_are_not_tasks(self):
        self.assertNotIn(
            "this example is documentation, not a task",
            [item.text for item in self.items],
        )

    def test_progress_counts_done_items(self):
        self.assertEqual(tasks.progress(self.items), "1/3")

    def test_indent_and_marker_survive_the_round_trip(self):
        body, touched = tasks.edit_checkboxes(BODY, ["all"], done=True)
        self.assertIn("  - [x] Cover the empty day case", body)
        self.assertEqual(len(touched), 3)
        self.assertIn("```md\n- [ ] this example", body)
        self.assertIn("- #615", body)

    def test_empty_body_has_no_tasks(self):
        self.assertEqual(tasks.parse_tasks(""), [])


class ResolveTests(unittest.TestCase):
    def setUp(self):
        self.items = tasks.parse_tasks(BODY)

    def test_all(self):
        self.assertEqual(len(tasks.resolve(self.items, "all")), 3)

    def test_single_index(self):
        self.assertEqual([item.index for item in
                         tasks.resolve(self.items, "2")], [2])

    def test_index_list_and_range(self):
        self.assertEqual([item.index for item in
                         tasks.resolve(self.items, "1,3")], [1, 3])
        self.assertEqual([item.index for item in
                         tasks.resolve(self.items, "2-3")], [2, 3])

    def test_out_of_range_is_explained(self):
        with self.assertRaises(tasks.TaskSelection) as caught:
            tasks.resolve(self.items, "9")
        self.assertIn("3 tasks", str(caught.exception))

    def test_unique_phrase_matches(self):
        self.assertEqual([item.index for item in
                         tasks.resolve(self.items, "empty day")], [3])

    def test_ambiguous_phrase_refuses_to_guess(self):
        with self.assertRaises(tasks.TaskSelection) as caught:
            tasks.resolve(self.items, "recompute")
        self.assertIn("matches 2 tasks", str(caught.exception))

    def test_unknown_phrase_lists_the_choices(self):
        with self.assertRaises(tasks.TaskSelection) as caught:
            tasks.resolve(self.items, "zzz")
        self.assertIn("Recompute on add", str(caught.exception))

    def test_no_checklist_at_all(self):
        with self.assertRaises(tasks.TaskSelection):
            tasks.resolve([], "all")


class EditTests(unittest.TestCase):
    def test_tick_then_untick_is_reversible(self):
        ticked, _ = tasks.edit_checkboxes(BODY, ["2,3"], done=True)
        self.assertEqual(ticked.count("[x]"), 3)
        back, _ = tasks.edit_checkboxes(ticked, ["2,3"], done=False)
        self.assertEqual(back, BODY)

    def test_only_the_selected_lines_change(self):
        body, touched = tasks.edit_checkboxes(BODY, ["2"], done=True)
        self.assertEqual([item.index for item in touched], [2])
        self.assertNotIn("- [x] Cover", body)
        self.assertIn("- [x] Recompute on delete", body)

    def test_multiple_targets_combine(self):
        body, touched = tasks.edit_checkboxes(BODY, ["1", "3"], done=True)
        self.assertEqual(sorted(item.index for item in touched), [1, 3])
        self.assertIn("- [x] Recompute on add", body)

    def test_a_body_without_trailing_newline_keeps_it_absent(self):
        trimmed = BODY.rstrip("\n")
        body, _ = tasks.edit_checkboxes(trimmed, ["all"], done=True)
        self.assertFalse(body.endswith("\n"))


class SectionTests(unittest.TestCase):
    def test_find_section_ignores_case_and_hash_marks(self):
        self.assertGreater(tasks.find_section(WITH_ACCEPTANCE, "acceptance criteria"), 0)
        self.assertEqual(tasks.find_section(WITH_ACCEPTANCE, "Nope"), -1)

    def test_drop_section_removes_subsections_too(self):
        body, removed = tasks.drop_section(WITH_ACCEPTANCE, "Acceptance criteria")
        self.assertTrue(removed)
        self.assertNotIn("Acceptance criteria", body)
        self.assertNotIn("- [ ] b", body)
        self.assertNotIn("### Detail", body)
        self.assertIn("- [ ] a", body)
        self.assertIn("## Related", body)

    def test_one_blank_line_is_left_between_sections(self):
        body, _ = tasks.drop_section(WITH_ACCEPTANCE, "Acceptance criteria")
        self.assertIn("- [ ] a\n\n## Related", body)

    def test_dropping_the_last_section_keeps_the_rest(self):
        body, removed = tasks.drop_section(WITH_ACCEPTANCE, "Related")
        self.assertTrue(removed)
        self.assertNotIn("## Related", body)
        self.assertIn("## Acceptance criteria", body)

    def test_dropping_an_absent_section_changes_nothing(self):
        body, removed = tasks.drop_section(WITH_ACCEPTANCE, "Nothing here")
        self.assertFalse(removed)
        self.assertEqual(body, WITH_ACCEPTANCE)

    def test_heading_inside_a_code_fence_is_not_a_section(self):
        body = "## Tasks\n\n```md\n## Related\n```\n\n- [ ] a\n"
        found, removed = tasks.drop_section(body, "Related")
        self.assertFalse(removed)
        self.assertEqual(found, body)


class RenderTests(unittest.TestCase):
    def test_render_number_from_one(self):
        lines = tasks.render_checklist(tasks.parse_tasks(BODY)).splitlines()
        self.assertEqual(lines[0], "tasks 1/3 done")
        self.assertEqual(lines[1], "  1  [x] Recompute on add")

    def test_render_without_tasks(self):
        self.assertEqual(tasks.render_checklist([]),
                         "no checklist items in this body")


if __name__ == "__main__":
    unittest.main()
