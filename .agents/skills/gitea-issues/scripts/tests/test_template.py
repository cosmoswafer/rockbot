"""Tests for the standard issue body rules."""

import unittest

from gitea_issues import template


class ValidateTests(unittest.TestCase):
    def test_the_shipped_template_validates(self):
        self.assertEqual(template.validate_issue_body(template.ISSUE_TEMPLATE), [])

    def test_a_body_without_a_checklist_is_rejected(self):
        problems = template.validate_issue_body(
            "## Summary\n\nThe ring shows a stale total.\n"
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("checklist", problems[0])

    def test_acceptance_criteria_are_rejected(self):
        problems = template.validate_issue_body(
            "## Summary\n\nx\n\n## Tasks\n\n- [ ] do it\n\n"
            "## Acceptance criteria\n\n- [ ] it works\n"
        )
        self.assertTrue(any("acceptance" in problem.casefold()
                            for problem in problems))

    def test_translated_task_heading_is_accepted(self):
        body = "## 摘要\n\n圓環顯示舊值。\n\n## 待辦\n\n- [ ] 修正\n"
        self.assertEqual(template.validate_issue_body(body), [])

    def test_a_checklist_without_a_tasks_heading_is_flagged(self):
        problems = template.validate_issue_body("- [ ] do it\n- [ ] verify it\n")
        self.assertTrue(any("## Tasks" in problem for problem in problems))

    def test_empty_body(self):
        self.assertEqual(
            template.validate_issue_body(""),
            ["the body is empty — file issues with real content."],
        )

    def test_tasks_can_be_optional_for_a_pure_investigation_note(self):
        self.assertEqual(
            template.validate_issue_body("just prose", require_tasks=False),
            [],
        )


class HeadingTests(unittest.TestCase):
    def test_headings_are_collected_lowercased(self):
        self.assertEqual(
            template.headings("## Summary\n### Detail Two\n# Tasks\n"),
            ["summary", "detail two", "tasks"],
        )

    def test_has_task_section_tolerates_a_prefix(self):
        self.assertTrue(template.has_task_section("## Tasks for this week\n"))
        self.assertFalse(template.has_task_section("## Related work\n"))

    def test_banned_heading_reported_by_name(self):
        self.assertEqual(
            template.has_banned_section("## Acceptance criteria\n- ok\n"),
            "acceptance criteria",
        )


if __name__ == "__main__":
    unittest.main()
